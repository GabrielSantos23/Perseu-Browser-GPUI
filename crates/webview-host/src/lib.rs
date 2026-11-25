//! WebView2 host for perseu, built on Wry.
//!
//! Creates, positions, shows, hides and navigates native webviews that live
//! inside a parent window owned by the GPUI app.

mod downloads;
mod events;
pub mod extensions;
mod guard;
#[cfg(test)]
mod hardening_tests;
mod keys;
mod region;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::{Context as _, Result};
use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_PROFILE, COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_DENY,
    COREWEBVIEW2_PREFERRED_COLOR_SCHEME_AUTO, COREWEBVIEW2_PREFERRED_COLOR_SCHEME_DARK,
    COREWEBVIEW2_PREFERRED_COLOR_SCHEME_LIGHT, GetAvailableCoreWebView2BrowserVersionString,
    ICoreWebView2, ICoreWebView2_3, ICoreWebView2_8, ICoreWebView2_13, ICoreWebView2Environment,
    ICoreWebView2Profile2, ICoreWebView2Settings8,
};
use webview2_com::{
    CallDevToolsProtocolMethodCompletedHandler, ClearBrowsingDataCompletedHandler, take_pwstr,
};
use windows::core::{HSTRING, Interface, PCWSTR, PWSTR, w};
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::{Rect, WebContext, WebViewBuilder, WebViewBuilderExtWindows, WebViewExtWindows};

// Replaces wry's defaults, which also disable SmartScreen (msSmartScreenProtection)
// and allow autoplay with sound. Every view must use the same arguments.
const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI --enable-features=OverlayScrollbar,msOverlayScrollbarWinStyle:scrollbar_mode/full_mode,msOverlayScrollbarWinStyleAnimation";

/// Version of the installed Evergreen WebView2 Runtime, or `None` when it is
/// missing. perseu never bundles a fixed-version runtime.
pub fn runtime_version() -> Option<String> {
    let mut version = PWSTR::null();
    unsafe { GetAvailableCoreWebView2BrowserVersionString(PCWSTR::null(), &mut version) }.ok()?;
    let version = take_pwstr(version);
    (!version.is_empty()).then_some(version)
}

/// Something that happened in a webview, reported on the UI thread.
#[derive(Debug, Clone)]
pub enum WebViewEvent {
    /// The main frame started a navigation.
    LoadStarted,
    /// The main frame finished a navigation, successfully or not.
    LoadFinished,
    /// The main frame committed a new URL.
    UrlChanged(String),
    /// The document title changed.
    TitleChanged(String),
    /// The favicon changed. `None` means the page has no favicon.
    FaviconChanged(Option<Vec<u8>>),
    /// Back/forward availability changed.
    HistoryChanged {
        /// Whether there is a page to go back to.
        can_go_back: bool,
        /// Whether there is a page to go forward to.
        can_go_forward: bool,
    },
    /// The webview took keyboard focus, for example because the user clicked the page.
    FocusGained,
    /// The page asked for a new window (`window.open`, `target=_blank`). The
    /// window was not opened; the host decides whether to open the URL elsewhere.
    NewWindowRequested {
        /// The URL the page wanted to open.
        url: String,
        /// Whether a click or key press caused it, rather than a script alone.
        user_initiated: bool,
        /// Width and height the opener asked for, in CSS pixels, when it did
        /// (`window.open` features, an extension's `windows.create`).
        size: Option<(f64, f64)>,
        /// Set unless the request became a peek: the opener waits for a view
        /// to show it in, handed over with [`BrowserView::adopt_new_window`]
        /// or refused with [`decline_new_window`]. One of them must follow.
        request: Option<u64>,
    },
    /// The page asked to close its window (`window.close()`, an extension's
    /// `windows.remove`). Nothing was closed; the host decides.
    CloseRequested,
    /// An element of the page entered (`true`) or left fullscreen, as a
    /// video player does. The host decides how to show it.
    FullscreenChanged(bool),
    /// A key chord accepted by [`BrowserViewOptions::intercept`] was pressed
    /// while the page had focus. The page did not receive it.
    Shortcut(KeyChord),
    /// The user opened an http(s) link in a new window (Shift+click,
    /// Ctrl+click) while holding exactly [`BrowserViewOptions::peek_modifier`].
    PeekRequested(String),
    /// A page asked for a permission. Its request waits until
    /// [`answer_permission`] is called; WebView2 does not remember the answer.
    PermissionRequested {
        /// Identifier for [`answer_permission`].
        id: u64,
        /// URL of the frame that asked.
        url: String,
        /// What it asked for.
        kind: PermissionKind,
    },
    /// A page tried to open a URL in another app (`mailto:`, `zoommtg:`). The
    /// launch was cancelled; the host may ask the user and launch it itself.
    ExternalSchemeRequested {
        /// The URL to hand to the other app.
        uri: String,
        /// Origin of the page that asked.
        origin: String,
        /// Whether a click or key press caused it.
        user_initiated: bool,
    },
    /// The page zoom changed, for example by Ctrl+scroll; `1.0` is 100%.
    ZoomChanged(f64),
    /// Chromium's security state of the main frame changed.
    SecurityChanged(SecurityState),
    /// The page started or stopped playing sound, or was muted or unmuted.
    AudioChanged {
        /// Whether the page is producing sound, even while muted.
        playing: bool,
        /// Whether the page's sound is muted.
        muted: bool,
    },
    /// A download started. WebView2's own download UI is hidden.
    DownloadStarted {
        /// Identifier for later events and [`cancel_download`].
        id: u64,
        /// Where it comes from.
        url: String,
        /// Where it is being saved.
        path: PathBuf,
        /// Expected size, when the server said.
        total: Option<u64>,
    },
    /// A download received more bytes. Reported at most four times a second.
    DownloadProgress {
        /// Download identifier.
        id: u64,
        /// Bytes received so far.
        received: u64,
        /// Expected size, when known.
        total: Option<u64>,
    },
    /// A download ended.
    DownloadFinished {
        /// Download identifier.
        id: u64,
        /// How it ended.
        outcome: DownloadOutcome,
    },
}

/// How a download ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadOutcome {
    /// The file was saved.
    Completed,
    /// The user cancelled it.
    Cancelled,
    /// A network, disk, server or safety problem stopped it, with the
    /// runtime's own word for what happened.
    Failed(&'static str),
}

pub use downloads::cancel_download;
pub use guard::{answer_permission, decline_new_window};

/// The connection security of a page, as Chromium judges it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityState {
    /// HTTPS with a valid certificate and no mixed content.
    Secure,
    /// Plain HTTP, broken HTTPS or a known-dangerous page.
    Insecure,
    /// Neither, such as internal or blank pages.
    Neutral,
}

/// A permission a page can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionKind {
    /// Microphone.
    Microphone,
    /// Camera.
    Camera,
    /// Location.
    Geolocation,
    /// Notifications.
    Notifications,
    /// Motion and other sensors.
    Sensors,
    /// Reading the clipboard.
    ClipboardRead,
    /// Downloading several files at once.
    AutomaticDownloads,
    /// Editing files and folders.
    FileSystem,
    /// Playing media with sound.
    Autoplay,
    /// Installed fonts.
    LocalFonts,
    /// MIDI device control.
    Midi,
    /// Placing windows on screens.
    WindowManagement,
    /// Something this version does not know.
    Other,
}

impl PermissionKind {
    /// Stable name for storing decisions.
    pub fn key(self) -> &'static str {
        match self {
            Self::Microphone => "microphone",
            Self::Camera => "camera",
            Self::Geolocation => "geolocation",
            Self::Notifications => "notifications",
            Self::Sensors => "sensors",
            Self::ClipboardRead => "clipboard-read",
            Self::AutomaticDownloads => "automatic-downloads",
            Self::FileSystem => "file-system",
            Self::Autoplay => "autoplay",
            Self::LocalFonts => "local-fonts",
            Self::Midi => "midi",
            Self::WindowManagement => "window-management",
            Self::Other => "other",
        }
    }

    /// What the page wants, phrased for a prompt: "use your camera".
    pub fn description(self) -> &'static str {
        match self {
            Self::Microphone => "use your microphone",
            Self::Camera => "use your camera",
            Self::Geolocation => "know your location",
            Self::Notifications => "show notifications",
            Self::Sensors => "use motion sensors",
            Self::ClipboardRead => "read your clipboard",
            Self::AutomaticDownloads => "download multiple files",
            Self::FileSystem => "edit files on your device",
            Self::Autoplay => "play sound automatically",
            Self::LocalFonts => "see your installed fonts",
            Self::Midi => "control MIDI devices",
            Self::WindowManagement => "manage windows on your screens",
            Self::Other => "use a device feature",
        }
    }
}

/// The modifier key that turns a link click into a peek request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkModifier {
    /// Shift.
    Shift,
    /// Control.
    Ctrl,
}

/// A key press with its modifiers, as seen by the webview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyChord {
    /// Control was held.
    pub ctrl: bool,
    /// Shift was held.
    pub shift: bool,
    /// Alt was held.
    pub alt: bool,
    /// Lowercase key name in GPUI keymap style: `"t"`, `"1"`, `"tab"`, `"f5"`.
    pub key: String,
}

/// Decides, synchronously, whether a key chord belongs to the browser rather than the page.
pub type ShortcutFilter = Rc<dyn Fn(&KeyChord) -> bool>;

/// Receives webview events. Called on the UI thread, possibly re-entrantly
/// from inside a webview method call, so it should only queue work.
pub type EventSink = Rc<dyn Fn(WebViewEvent)>;

/// Where the webview sits inside its parent window, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContentBounds {
    /// Left edge relative to the parent's client area.
    pub x: f32,
    /// Top edge relative to the parent's client area.
    pub y: f32,
    /// Width of the webview.
    pub width: f32,
    /// Height of the webview.
    pub height: f32,
    /// Corner radius used to clip the webview.
    pub corner_radius: f32,
    /// Parent window scale factor (1.0 = 96 DPI).
    pub scale_factor: f32,
    /// Width of an overlay covering the webview's left edge, in logical
    /// pixels. That rounded strip is cut out of the webview so the parent
    /// window's content shows through. 0 means no cutout.
    pub cutout_width: f32,
    /// Height of an overlay covering the webview's top edge, in logical
    /// pixels. The webview below it keeps rounded top corners. 0 means no
    /// cutout.
    pub cutout_top: f32,
}

impl ContentBounds {
    fn same_geometry(&self, other: &Self) -> bool {
        (self.x, self.y, self.width, self.height, self.scale_factor)
            == (
                other.x,
                other.y,
                other.width,
                other.height,
                other.scale_factor,
            )
    }
}

/// Options for creating a [`BrowserView`].
pub struct BrowserViewOptions<'a> {
    /// First URL to load. Empty loads nothing, for a view that will take
    /// over a new window ([`BrowserView::adopt_new_window`]).
    pub url: &'a str,
    /// Color shown before the first paint, as RGBA bytes.
    pub background: (u8, u8, u8, u8),
    /// Key chords to take away from the page and report as [`WebViewEvent::Shortcut`].
    pub intercept: ShortcutFilter,
    /// Link clicks with this modifier held are reported as
    /// [`WebViewEvent::PeekRequested`]. `None` leaves link clicks to the page.
    pub peek_modifier: Option<LinkModifier>,
    /// WebView2 user data folder (cookies, cache, storage). Every view of
    /// the app should use the same one. `None` uses WebView2's default.
    pub data_directory: Option<&'a Path>,
    /// Folder downloads are saved to. `None` leaves the profile's setting alone.
    pub download_folder: Option<&'a Path>,
    /// Let the runtime host Chromium extensions. The capability is fixed when
    /// the profile's environment is created, so every view of the app should
    /// pass the same value; which extensions load stays with the host.
    pub extensions_enabled: bool,
    /// Check downloads with SmartScreen. With it on, a file the check flags
    /// is held by the runtime and never saved unless the app shows the
    /// runtime's own download dialog — this app does not, so the download
    /// simply never finishes. Every view sharing a data folder should pass
    /// the same value.
    pub reputation_checks: bool,
    /// Run InPrivate: nothing the page stores outlives the last private view.
    pub private: bool,
}

thread_local! {
    /// The WebView2 environment of the first view, which every later view
    /// shares: a view handed to an opener as its new window must come from
    /// the opener's environment, or the runtime crashes. Every view of the
    /// app uses the same data folder and options, so one is enough.
    static SHARED_ENVIRONMENT: RefCell<Option<ICoreWebView2Environment>> =
        const { RefCell::new(None) };
}

/// One WebView2 instance hosted as a child of a GPUI window.
pub struct BrowserView {
    webview: wry::WebView,
    applied_bounds: Cell<Option<ContentBounds>>,
}

impl Drop for BrowserView {
    fn drop(&mut self) {
        guard::decline_windows_of(&self.webview.webview());
    }
}

impl BrowserView {
    /// Creates a webview as a child of `parent` and starts loading `options.url`.
    ///
    /// Creation runs a nested message loop until WebView2 is ready, so call it
    /// while no UI framework state is borrowed. `parent` must outlive the view.
    pub fn new(
        parent: RawWindowHandle,
        options: BrowserViewOptions<'_>,
        sink: EventSink,
    ) -> Result<Self> {
        let title_sink = sink.clone();
        let mut context = WebContext::new(options.data_directory.map(Path::to_path_buf));
        let mut builder = WebViewBuilder::new_with_web_context(&mut context)
            .with_additional_browser_args(BROWSER_ARGS)
            .with_background_color(options.background)
            .with_focused(false)
            .with_incognito(options.private)
            .with_hotkeys_zoom(true)
            .with_bounds(Rect {
                position: LogicalPosition::new(0.0, 0.0).into(),
                size: LogicalSize::new(0.0, 0.0).into(),
            })
            .with_document_title_changed_handler(move |title| {
                title_sink(WebViewEvent::TitleChanged(title))
            });
        if !options.url.is_empty() {
            builder = builder.with_url(options.url);
        }
        if let Some(environment) = SHARED_ENVIRONMENT.with(|shared| shared.borrow().clone()) {
            builder = builder.with_environment(environment);
        }
        if options.extensions_enabled {
            builder = builder.with_browser_extensions_enabled(true);
        }
        let webview = builder
            .build_as_child(&ParentWindow(parent))
            .context("failed to create WebView2")?;

        SHARED_ENVIRONMENT.with(|shared| {
            shared
                .borrow_mut()
                .get_or_insert_with(|| webview.environment());
        });
        if let Err(error) = webview.set_theme(wry::Theme::Dark) {
            tracing::warn!("WebView2 runtime does not support theming: {error}");
        }
        require_reputation_checks(&webview.webview(), options.reputation_checks);
        events::subscribe(
            &webview.controller(),
            &webview.webview(),
            sink.clone(),
            options.intercept,
        )
        .context("failed to subscribe to WebView2")?;
        guard::subscribe(&webview.webview(), sink, options.peek_modifier)
            .context("failed to guard WebView2")?;

        let view = Self {
            webview,
            applied_bounds: Cell::new(None),
        };
        if let Some(folder) = options.download_folder {
            view.set_download_folder(folder);
        }
        Ok(view)
    }

    /// The WebView2 COM interface, for profile-wide hosts such as the
    /// extension manager. Cloned; the view keeps its own reference.
    pub(crate) fn com_webview(&self) -> ICoreWebView2 {
        self.webview.webview()
    }

    /// Sets the folder downloads are saved to. The setting belongs to the
    /// WebView2 profile, so it applies to every view sharing the data folder.
    pub fn set_download_folder(&self, folder: &Path) {
        let result = unsafe {
            self.webview
                .webview()
                .cast::<ICoreWebView2_13>()
                .and_then(|webview| webview.Profile())
                .and_then(|profile| {
                    profile.SetDefaultDownloadFolderPath(&HSTRING::from(folder.as_os_str()))
                })
        };
        log_failure("set download folder", result);
    }

    /// Turns SmartScreen download checks on or off. The setting belongs to
    /// the WebView2 profile and applies to every view sharing the data
    /// folder on their next navigation.
    pub fn set_reputation_checks(&self, required: bool) {
        let result = unsafe {
            self.webview
                .webview()
                .Settings()
                .and_then(|settings| settings.cast::<ICoreWebView2Settings8>())
                .and_then(|settings| settings.SetIsReputationCheckingRequired(required))
        };
        log_failure("set SmartScreen checks", result);
    }

    /// Moves and resizes the webview. Repeated calls with the same bounds are ignored.
    pub fn set_bounds(&self, bounds: ContentBounds) {
        let previous = self.applied_bounds.replace(Some(bounds));
        if previous == Some(bounds) {
            return;
        }
        if previous.is_some_and(|previous| previous.same_geometry(&bounds)) {
            region::clip(self.webview.hwnd(), bounds);
            return;
        }

        let rect = Rect {
            position: LogicalPosition::new(bounds.x, bounds.y).into(),
            size: LogicalSize::new(bounds.width, bounds.height).into(),
        };
        if let Err(error) = self.webview.set_bounds(rect) {
            tracing::warn!("failed to set webview bounds: {error}");
            return;
        }
        region::clip(self.webview.hwnd(), bounds);
    }

    /// Shows or hides the webview. Hidden webviews keep running.
    pub fn set_visible(&self, visible: bool) {
        log_failure(
            "change webview visibility",
            self.webview.set_visible(visible),
        );
    }

    /// Loads `url` in the main frame.
    pub fn navigate(&self, url: &str) {
        log_failure("navigate", self.webview.load_url(url));
    }

    /// Goes back one entry in session history.
    /// Serves `folder` to this view at `https://{host}/`, for pages of the
    /// app's own that need a real origin. Other sites cannot read it.
    pub fn serve_folder(&self, host: &str, folder: &Path) -> Result<()> {
        let webview: ICoreWebView2_3 = self
            .webview
            .webview()
            .cast()
            .context("WebView2 cannot serve folders")?;
        unsafe {
            webview.SetVirtualHostNameToFolderMapping(
                &HSTRING::from(host),
                &HSTRING::from(folder.as_os_str()),
                COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_DENY,
            )
        }
        .context("serving the folder")
    }

    /// The WebView2 browser process hosting this view.
    pub fn browser_process_id(&self) -> Option<u32> {
        let mut id = 0;
        unsafe { self.webview.webview().BrowserProcessId(&mut id) }.ok()?;
        Some(id)
    }

    /// Shows `html` as the page, with no address of its own.
    pub fn load_html(&self, html: &str) {
        if let Err(error) = self.webview.load_html(html) {
            tracing::warn!("failed to show page: {error}");
        }
    }

    pub fn go_back(&self) {
        log_failure("go back", self.webview.go_back());
    }

    /// Goes forward one entry in session history.
    pub fn go_forward(&self) {
        log_failure("go forward", self.webview.go_forward());
    }

    /// Reloads the current page.
    pub fn reload(&self) {
        log_failure("reload", self.webview.reload());
    }

    /// Calls `done` on the UI thread with the size, in CSS pixels, of the
    /// current document's content (never smaller than the view). Read
    /// through the DevTools protocol; nothing runs in the page.
    pub fn measure_content(&self, done: impl FnOnce(f64, f64) + 'static) {
        let completed =
            CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, json| {
                if result.is_ok()
                    && let Some((width, height)) = content_size(&json)
                {
                    done(width, height);
                }
                Ok(())
            }));
        log_failure("measure the page", unsafe {
            self.webview.webview().CallDevToolsProtocolMethod(
                w!("Page.getLayoutMetrics"),
                w!("{}"),
                &completed,
            )
        });
    }

    /// Runs `expression` in the page once, through the DevTools protocol
    /// (nothing stays registered), and calls `done` on the UI thread with
    /// its value when that is a string, awaiting it when it is a promise.
    pub fn evaluate_string(&self, expression: &str, done: impl FnOnce(String) + 'static) {
        let params =
            serde_json::json!({ "expression": expression, "returnByValue": true, "awaitPromise": true })
                .to_string();
        let completed =
            CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, json| {
                if result.is_ok()
                    && let Some(value) = evaluated_string(&json)
                {
                    done(value);
                }
                Ok(())
            }));
        log_failure("run a page script", unsafe {
            self.webview.webview().CallDevToolsProtocolMethod(
                w!("Runtime.evaluate"),
                &HSTRING::from(params),
                &completed,
            )
        });
    }

    /// Shows the new window reported with `request` in this view, which
    /// must not have loaded anything yet. The opener gets this view as its
    /// window: it can message and close it. False when the request is gone
    /// (its opener closed first); the view should then load the page itself.
    pub fn adopt_new_window(&self, request: u64) -> bool {
        guard::adopt_new_window(request, &self.webview.webview())
    }

    /// Reloads the current page without using cached files.
    pub fn hard_reload(&self) {
        hard_reload(&self.webview.webview());
    }

    /// Pops the page's playing (or first) video out into picture-in-picture,
    /// or back in. Runs as if the user clicked, which the browser requires.
    pub fn toggle_picture_in_picture(&self) {
        const SCRIPT: &str = "(()=>{if(document.pictureInPictureElement){return document.exitPictureInPicture()}const v=[...document.querySelectorAll('video')];const t=v.find(e=>!e.paused)||v[0];return t&&t.requestPictureInPicture()})()";
        let params = serde_json::json!({ "expression": SCRIPT, "userGesture": true }).to_string();
        let completed =
            CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|result, _| {
                log_failure("picture-in-picture", result);
                Ok(())
            }));
        log_failure("picture-in-picture", unsafe {
            self.webview.webview().CallDevToolsProtocolMethod(
                w!("Runtime.evaluate"),
                &HSTRING::from(params),
                &completed,
            )
        });
    }

    /// Asks pages of this view's profile for a light (`Some(true)`), dark
    /// or system-matching (`None`) look, through `prefers-color-scheme`.
    pub fn set_color_scheme(&self, light: Option<bool>) {
        let scheme = match light {
            None => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_AUTO,
            Some(true) => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_LIGHT,
            Some(false) => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_DARK,
        };
        let result = self
            .webview
            .webview()
            .cast::<ICoreWebView2_13>()
            .and_then(|webview| unsafe { webview.Profile()?.SetPreferredColorScheme(scheme) });
        if let Err(error) = result {
            tracing::warn!("setting the page color scheme failed: {error}");
        }
    }

    /// Takes the page's fullscreen element, if any, out of fullscreen.
    pub fn exit_fullscreen(&self) {
        if let Err(error) = self
            .webview
            .evaluate_script("document.fullscreenElement && document.exitFullscreen()")
        {
            tracing::warn!("leaving page fullscreen failed: {error}");
        }
    }

    /// Empties the profile's HTTP cache, then reloads without it.
    pub fn clear_cache_and_reload(&self) {
        let webview = self.webview.webview();
        let reload_target = webview.clone();
        let completed =
            CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, _| {
                log_failure("clear the cache", result);
                hard_reload(&reload_target);
                Ok(())
            }));
        log_failure("clear the cache", unsafe {
            webview.CallDevToolsProtocolMethod(
                w!("Network.clearBrowserCache"),
                w!("{}"),
                &completed,
            )
        });
    }

    /// Stops the current navigation.
    pub fn stop(&self) {
        log_failure("stop", unsafe { self.webview.webview().Stop() });
    }

    /// Sets the page zoom; `1.0` is 100%.
    pub fn set_zoom(&self, factor: f64) {
        log_failure("zoom", unsafe {
            self.webview.controller().SetZoomFactor(factor)
        });
    }

    /// Deletes cookies, storage, cache and service workers of `origin`
    /// (`https://example.com`), then reloads the page.
    pub fn clear_site_data(&self, origin: &str) {
        let webview = self.webview.webview();
        let reload_target = webview.clone();
        let params = serde_json::json!({ "origin": origin, "storageTypes": "all" }).to_string();
        let completed =
            CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, _| {
                log_failure("clear site data", result);
                log_failure("reload", unsafe { reload_target.Reload() });
                Ok(())
            }));
        log_failure("clear site data", unsafe {
            webview.CallDevToolsProtocolMethod(
                w!("Storage.clearDataForOrigin"),
                &HSTRING::from(params),
                &completed,
            )
        });
    }

    /// Mutes or unmutes the page's sound.
    pub fn set_muted(&self, muted: bool) {
        let result = self
            .webview
            .webview()
            .cast::<ICoreWebView2_8>()
            .and_then(|webview| unsafe { webview.SetIsMuted(muted) });
        log_failure("mute", result);
    }

    /// Gives keyboard focus to the page.
    pub fn focus(&self) {
        log_failure("focus webview", self.webview.focus());
    }

    /// Moves the webview into another window, keeping the loaded page.
    /// Call [`BrowserView::set_bounds`] afterwards to place it there.
    pub fn reparent(&self, parent: RawWindowHandle) {
        let RawWindowHandle::Win32(handle) = parent else {
            tracing::warn!("cannot reparent webview to a non-Win32 window");
            return;
        };
        self.applied_bounds.set(None);
        log_failure("reparent webview", self.webview.reparent(handle.hwnd.get()));
    }

    /// Deletes everything the WebView2 profile stores: cookies, cache, site
    /// storage, its history, autofill and saved passwords. The profile is
    /// shared, so this clears it for every view.
    pub fn clear_browsing_data(&self) {
        let completed = ClearBrowsingDataCompletedHandler::create(Box::new(|result| {
            if let Err(error) = result {
                tracing::error!("failed to clear browsing data: {error}");
            }
            Ok(())
        }));
        let result = unsafe {
            self.webview
                .webview()
                .cast::<ICoreWebView2_13>()
                .and_then(|webview| webview.Profile())
                .and_then(|profile| profile.cast::<ICoreWebView2Profile2>())
                .and_then(|profile| {
                    profile
                        .ClearBrowsingData(COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_PROFILE, &completed)
                })
        };
        log_failure("clear browsing data", result);
    }

    /// Returns keyboard focus to the parent window.
    pub fn focus_parent(&self) {
        // SetFocus reports an "error" with a success code when nothing had focus before.
        let _ = self.webview.focus_parent();
    }
}

fn require_reputation_checks(
    webview: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
    required: bool,
) {
    let result = unsafe {
        webview
            .Settings()
            .and_then(|settings| settings.cast::<ICoreWebView2Settings8>())
            .and_then(|settings| settings.SetIsReputationCheckingRequired(required))
    };
    log_failure("set SmartScreen checks", result);
}

struct ParentWindow(RawWindowHandle);

impl HasWindowHandle for ParentWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        Ok(unsafe { WindowHandle::borrow_raw(self.0) })
    }
}

fn hard_reload(webview: &ICoreWebView2) {
    let completed = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|result, _| {
        log_failure("hard reload", result);
        Ok(())
    }));
    log_failure("hard reload", unsafe {
        webview.CallDevToolsProtocolMethod(
            w!("Page.reload"),
            w!(r#"{"ignoreCache":true}"#),
            &completed,
        )
    });
}

fn evaluated_string(json: &str) -> Option<String> {
    let reply: serde_json::Value = serde_json::from_str(json).ok()?;
    Some(reply.pointer("/result/value")?.as_str()?.to_string())
}

fn content_size(json: &str) -> Option<(f64, f64)> {
    let metrics: serde_json::Value = serde_json::from_str(json).ok()?;
    let size = metrics.get("cssContentSize")?;
    Some((size.get("width")?.as_f64()?, size.get("height")?.as_f64()?))
}

fn log_failure<E: std::fmt::Display>(action: &str, result: Result<(), E>) {
    if let Err(error) = result {
        tracing::warn!("failed to {action}: {error}");
    }
}
