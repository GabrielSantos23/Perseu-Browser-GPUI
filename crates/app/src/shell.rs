mod archive;
mod command;
mod compact;
mod content;
mod downloads_panel;
mod essentials;
mod extensions;
mod favorites;
mod folders;
mod fullscreen;
mod history_page;
mod identify;
mod media_player;
mod menus;
mod nav;
mod panel_popup;
mod peek;
mod persist;
mod private;
mod row_motion;
mod settings_page;
mod sidebar;
mod site_guard;
mod site_menu;
mod site_title;
mod spaces;
mod split;
mod store_button;
mod tab_list;
mod tab_ops;
mod theme_picker;
mod toasts;

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use browser_core::browser::Browser;
use browser_core::downloads::Downloads;
use browser_core::history::History;
use browser_core::permissions::SitePermissions;
use browser_core::session::Session;
use browser_core::settings::Settings;
use browser_core::tabs::{OpenPosition, Tab, TabId};
use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui::{
    AppContext, Bounds, Context, Entity, FocusHandle, InteractiveElement, IntoElement, MouseButton,
    MouseMoveEvent, ParentElement, Pixels, Render, Styled, Subscription, Task, Window, div,
    prelude::FluentBuilder,
};
use gpui_kit::component::input::InputState;
use webview_host::{BrowserView, WebViewEvent};

use crate::actions::{self, keystroke_for};
use crate::page::{self, TabRuntime};
use crate::paths::Paths;
use crate::site_menu::SiteMenuEvent;
use crate::theme::{SpacePalette, Theme};
use crate::title_bar::TitleBar;
use gpui::StyledImage as _;

pub(crate) use history_page::bind_keys as bind_history_keys;
pub(crate) use peek::bind_keys as bind_peek_keys;
pub(crate) use private::PRIVATE_FLAG;
pub(crate) use settings_page::bind_keys as bind_settings_keys;

const URL_PLACEHOLDER: &str = "Search or enter address";
const ESCAPE_KEY: &str = "escape";
const APP_TITLE: &str = browser_core::APP_NAME;
const GRAIN_ALPHA: f32 = 0.18;
const POPUP_SETTLE: std::time::Duration = std::time::Duration::from_millis(30);
const ESSENTIALS_HINT: std::time::Duration = std::time::Duration::from_millis(180);
/// The first check runs each time the browser opens, once startup settles.
const UPDATE_FIRST_CHECK: std::time::Duration = std::time::Duration::from_secs(3);
const UPDATE_CHECK_EVERY: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

type TabEvent = (TabId, WebViewEvent);

pub struct Shell {
    focus_handle: FocusHandle,
    browser: Browser,
    runtime: HashMap<TabId, TabRuntime>,
    events: UnboundedSender<TabEvent>,
    history: History,
    command_bar: Option<command::OpenCommandBar>,
    content_bounds: Rc<Cell<Bounds<Pixels>>>,
    compact: bool,
    fullscreen: Option<fullscreen::Fullscreen>,
    reveal: crate::motion::Toggle,
    compact_hover: compact::HoverTimers,
    title_reveal: crate::motion::Toggle,
    title_hover: compact::HoverTimers,
    compact_poll: Option<Task<()>>,
    split_selection: Vec<TabId>,
    resizing_split: bool,
    peek: Option<peek::OpenPeek>,
    menu: Option<menus::OpenMenu>,
    toast: Option<toasts::OpenToast>,
    reload_hold: Option<Task<()>>,
    site_menu: Option<site_menu::OpenSiteMenu>,
    site_menu_closed: Option<std::time::Instant>,
    unpinned_scroll: gpui::ScrollHandle,
    row_motion: std::cell::RefCell<row_motion::RowMotion>,
    essentials_hint: crate::motion::Toggle,
    main_window: gpui::AnyWindowHandle,
    panel: Option<panel_popup::OpenPanel>,
    panel_closed: Option<(panel_popup::Panel, std::time::Instant)>,
    folder_toggled: HashMap<String, std::time::Instant>,
    download_flight: Option<(u64, std::time::Instant)>,
    command_query: String,
    window_placement: Option<browser_core::session::SavedWindow>,
    site_zoom: std::collections::BTreeMap<String, f64>,
    /// Tabs on a Web Store listing whose "Add to perseu" is watched.
    store_watch: std::collections::HashSet<TabId>,
    /// The store extension being downloaded or waiting on its card.
    store_adding: Option<String>,
    store_offer: Option<store_button::StoreOffer>,
    tab_drag: Option<row_motion::TabDrag>,
    page_had_focus: Cell<bool>,
    space_name_input: Entity<InputState>,
    renaming_space: bool,
    folder_name_input: Entity<InputState>,
    renaming_folder: Option<String>,
    collapsed_folders: std::collections::HashSet<String>,
    media: media_player::MediaState,
    theme_picker: Option<theme_picker::OpenThemePicker>,
    space_slide_from_right: bool,
    archive_open: bool,
    downloads: Downloads,
    downloads_open: bool,
    extensions: extensions::ExtensionHost,
    extensions_open: bool,
    download_hosts: HashMap<u64, Rc<BrowserView>>,
    download_confirm: Option<u64>,
    prompts: Vec<site_guard::SitePrompt>,
    permissions: SitePermissions,
    pending_profile_clear: bool,
    paths: Paths,
    settings: Settings,
    settings_page: Option<settings_page::SettingsPage>,
    history_page: Option<history_page::HistoryPage>,
    /// A newer release, once the update check found one.
    update: Option<crate::updater::Release>,
    /// The newer release's installer, once downloaded; it runs on quit.
    update_download: Option<std::path::PathBuf>,
    update_check: UpdateCheck,
    saved_session: Option<Session>,
    session_save: Option<Task<()>>,
    sidebar_width: Pixels,
    resizing_sidebar: bool,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    /// `private` makes this the private window: no saved session is
    /// restored and nothing is recorded or saved.
    pub fn new(
        targets: Vec<String>,
        private: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (events, receiver) = unbounded();
        let space_name_input = cx.new(|cx| InputState::new(window, cx));
        let folder_name_input = cx.new(|cx| InputState::new(window, cx));
        let media = media_player::MediaState::start(window, cx);
        let mut state = persist::load_state();
        if private {
            state.session = None;
            state.history = History::new();
        }
        Theme::set_accent(state.settings.accent, cx);
        actions::bind_shortcuts(cx, &state.settings.shortcuts);
        let subscriptions = vec![
            cx.subscribe_in(&space_name_input, window, Self::on_space_name_event),
            cx.subscribe_in(&folder_name_input, window, Self::on_folder_name_event),
            cx.observe_self(|shell, cx| shell.schedule_session_save(cx)),
            cx.observe_window_bounds(window, |shell, window, cx| {
                shell.window_placement = Some(persist::placement(window));
                cx.notify();
            }),
            cx.observe_window_activation(window, |shell, window, cx| {
                if window.is_window_active() && shell.page_had_focus.get() {
                    shell.focus_active_page();
                }
                // Back from Windows Settings, the default-browser row reads again.
                if window.is_window_active() && shell.settings_page.is_some() {
                    cx.notify();
                }
            }),
            cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
            cx.on_app_quit(|shell, _| {
                shell.save_session_now();
                if let Some(installer) = shell.update_download.take()
                    && let Err(error) = crate::updater::run_installer(&installer, false)
                {
                    tracing::warn!("could not install the update on quit: {error:#}");
                }
                async {}
            }),
        ];
        forward_webview_events(receiver, window, cx);
        if !private {
            receive_forwarded_targets(window, cx);
        }
        let mut browser = state
            .session
            .as_ref()
            .map_or_else(Browser::new, Browser::from_session);
        browser.archive_after_ms = archive::archive_after_ms(&state.settings);
        if private {
            browser.show_private();
        }
        let mut extensions = extensions::ExtensionHost::new();
        extensions.refresh(&state.paths.extensions(), &state.settings.extensions);
        let layout = state
            .session
            .map(|session| session.layout)
            .unwrap_or_default();
        let metrics = &Theme::of(cx).metrics;
        let sidebar_width = layout.sidebar_width.map_or(metrics.sidebar_width, |width| {
            gpui::px(width).clamp(metrics.sidebar_min_width, metrics.sidebar_max_width)
        });

        let mut shell = Self {
            focus_handle: cx.focus_handle(),
            browser,
            runtime: HashMap::new(),
            events,
            history: state.history,
            command_bar: None,
            content_bounds: Rc::default(),
            compact: false,
            fullscreen: None,
            reveal: crate::motion::Toggle::new(compact::SLIDE),
            compact_hover: compact::HoverTimers::default(),
            title_reveal: crate::motion::Toggle::new(compact::SLIDE),
            title_hover: compact::HoverTimers::default(),
            compact_poll: None,
            split_selection: Vec::new(),
            resizing_split: false,
            peek: None,
            menu: None,
            toast: None,
            reload_hold: None,
            site_menu: None,
            site_menu_closed: None,
            unpinned_scroll: gpui::ScrollHandle::new(),
            row_motion: Default::default(),
            essentials_hint: crate::motion::Toggle::new(ESSENTIALS_HINT),
            main_window: window.window_handle(),
            panel: None,
            panel_closed: None,
            folder_toggled: HashMap::new(),
            download_flight: None,
            command_query: String::new(),
            window_placement: layout.window.or(Some(persist::placement(window))),
            site_zoom: layout.site_zoom.clone(),
            store_watch: Default::default(),
            store_adding: None,
            store_offer: None,
            tab_drag: None,
            page_had_focus: Cell::new(false),
            space_name_input,
            renaming_space: false,
            folder_name_input,
            renaming_folder: None,
            collapsed_folders: std::collections::HashSet::new(),
            media,
            theme_picker: None,
            space_slide_from_right: true,
            archive_open: false,
            downloads: Downloads::new(),
            downloads_open: false,
            extensions,
            extensions_open: false,
            download_hosts: HashMap::new(),
            download_confirm: None,
            prompts: Vec::new(),
            permissions: state.permissions,
            pending_profile_clear: false,
            paths: state.paths,
            settings: state.settings,
            settings_page: None,
            history_page: None,
            update: None,
            update_download: None,
            update_check: UpdateCheck::Idle,
            saved_session: None,
            session_save: None,
            sidebar_width,
            resizing_sidebar: false,
            _subscriptions: subscriptions,
        };
        if shell.browser.active_tab_id().is_some() {
            shell.show_active_tab(window, cx);
        } else {
            shell.open_blank_tab(window, cx);
            if targets.is_empty() {
                cx.defer_in(window, |shell, window, cx| shell.new_tab(window, cx));
            }
        }
        // After the shown tab, so it loads first.
        shell.load_essentials(window, cx);
        if layout.compact {
            shell.toggle_compact(window, cx);
        }
        shell.sync_hover_poll(window, cx);
        shell.open_targets(targets, window, cx);
        shell.start_archive_timer(window, cx);
        if !private {
            shell.watch_for_updates(window, cx);
        }
        shell
    }

    /// Looks for a newer release each time the browser opens and then a few
    /// times a day. A newer one downloads and installs when the app closes.
    fn watch_for_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !crate::updater::enabled() {
            return;
        }
        cx.spawn_in(window, async move |shell, cx| {
            let mut wait = UPDATE_FIRST_CHECK;
            loop {
                cx.background_executor().timer(wait).await;
                wait = UPDATE_CHECK_EVERY;
                let alive = shell.update_in(cx, |shell, window, cx| {
                    shell.check_for_updates(false, window, cx)
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// Asks GitHub for a newer release; `asked` reports the outcome even
    /// when there is nothing new, for the button in settings.
    pub(super) fn check_for_updates(
        &mut self,
        asked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.update_check == UpdateCheck::Checking {
            return;
        }
        self.update_check = UpdateCheck::Checking;
        cx.notify();
        cx.spawn_in(window, async move |shell, cx| {
            let found = cx
                .background_executor()
                .spawn(async { crate::updater::check() })
                .await;
            shell
                .update_in(cx, |shell, window, cx| {
                    shell.update_check = match &found {
                        Ok(_) => UpdateCheck::Done,
                        Err(error) => {
                            tracing::info!("update check failed: {error:#}");
                            UpdateCheck::Failed(format!("{error:#}"))
                        }
                    };
                    match found {
                        Ok(Some(release)) => shell.found_update(release, window, cx),
                        Ok(None) if asked => shell.show_toast("perseu is up to date", window, cx),
                        _ => {}
                    }
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    fn found_update(
        &mut self,
        release: crate::updater::Release,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.update.as_ref() == Some(&release) {
            return;
        }
        self.show_toast(
            format!("Version {} installs when you close perseu", release.version),
            window,
            cx,
        );
        self.update = Some(release.clone());
        self.update_download = None;
        cx.spawn_in(window, async move |shell, cx| {
            let wanted = release.clone();
            let downloaded = cx
                .background_executor()
                .spawn(async move { crate::updater::download(&wanted) })
                .await;
            shell
                .update_in(cx, |shell, _, cx| match downloaded {
                    Ok(path) if shell.update.as_ref() == Some(&release) => {
                        shell.update_download = Some(path);
                        cx.notify();
                    }
                    Ok(_) => {}
                    Err(error) => tracing::warn!("update download failed: {error:#}"),
                })
                .ok();
        })
        .detach();
    }

    /// Installs the newer release now: downloads it if needed, starts the
    /// installer and quits so it can replace us and reopen.
    pub(super) fn install_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(release) = self.update.clone() else {
            return;
        };
        let ready = self.update_download.take();
        if ready.is_none() {
            self.show_toast(
                format!("Downloading version {}\u{2026}", release.version),
                window,
                cx,
            );
        }
        cx.spawn_in(window, async move |shell, cx| {
            let installed = cx
                .background_executor()
                .spawn(async move {
                    let installer = match ready {
                        Some(path) => path,
                        None => crate::updater::download(&release)?,
                    };
                    crate::updater::run_installer(&installer, true)
                })
                .await;
            shell
                .update_in(cx, |shell, window, cx| match installed {
                    Ok(()) => {
                        shell.save_session_now();
                        cx.quit();
                    }
                    Err(error) => {
                        shell.show_toast(format!("Could not update: {error:#}"), window, cx)
                    }
                })
                .ok();
        })
        .detach();
    }

    fn palette(&self, cx: &Context<Self>) -> SpacePalette {
        let space = self.browser.space();
        Theme::of(cx).space_palette(space.theme, space.intensity)
    }

    /// Layers drawn over a space-colored surface, under its content: the
    /// third theme color and the texture grain.
    fn space_backdrop(&self, cx: &Context<Self>) -> Vec<gpui::AnyElement> {
        let space = self.browser.space();
        let mut layers = Vec::new();
        if let Some(overlay) = self.palette(cx).overlay {
            layers.push(div().absolute().size_full().bg(overlay).into_any_element());
        }
        if space.texture > 0.0
            && let Some(image) = crate::grain::texture(space.texture_kind)
        {
            layers.push(
                gpui::img(image)
                    .absolute()
                    .size_full()
                    .object_fit(gpui::ObjectFit::Cover)
                    .opacity(space.texture * GRAIN_ALPHA)
                    .into_any_element(),
            );
        }
        layers
    }

    fn active_runtime(&self) -> Option<&TabRuntime> {
        self.browser
            .active_tab_id()
            .and_then(|id| self.runtime.get(&id))
    }

    fn active_view(&self) -> Option<Rc<BrowserView>> {
        self.active_runtime()
            .and_then(|runtime| runtime.view.clone())
    }

    fn active_url(&self) -> &str {
        self.browser.active_tab().map_or("", |tab| tab.url.as_str())
    }

    fn on_webview_event(
        &mut self,
        id: TabId,
        event: WebViewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_peek(id) {
            self.on_peek_page_event(event, cx);
            return;
        }
        let is_active = self.browser.active_tab_id() == Some(id);
        match event {
            WebViewEvent::UrlChanged(url) => {
                self.forget_page_state(id);
                if !self.browser.is_private(id) {
                    persist::log_failure(self.history.record_visit(&url, tab_ops::now()));
                }
                let previous = self.browser.tab(id).map(|tab| tab.url.clone());
                self.restore_zoom(id, previous.as_deref().unwrap_or(""), &url);
                let listing = browser_core::webstore::extension_id(&url).is_some();
                self.update_tab(id, |tab| tab.url = url);
                if listing {
                    self.watch_store_listing(id, cx);
                }
            }
            WebViewEvent::TitleChanged(title) => {
                if let Some(tab) = self.browser.tab(id)
                    && !self.browser.is_private(id)
                {
                    persist::log_failure(self.history.set_title(&tab.url, &title));
                }
                self.update_tab(id, |tab| tab.title = title);
            }
            WebViewEvent::FaviconChanged(png) => {
                if let Some(runtime) = self.runtime.get_mut(&id) {
                    runtime.set_favicon(png.as_deref());
                }
                self.update_tab(id, |tab| tab.favicon = png.map(Arc::from));
            }
            WebViewEvent::LoadStarted => self.update_runtime(id, |runtime| {
                runtime.loading = true;
                runtime.security = None;
            }),
            WebViewEvent::LoadFinished => {
                self.update_runtime(id, |runtime| runtime.loading = false)
            }
            WebViewEvent::ZoomChanged(factor) => {
                self.remember_zoom(id, factor);
                self.update_runtime(id, |runtime| runtime.zoom = Some(factor))
            }
            WebViewEvent::HistoryChanged {
                can_go_back,
                can_go_forward,
            } => self.update_runtime(id, |runtime| {
                runtime.can_go_back = can_go_back;
                runtime.can_go_forward = can_go_forward;
            }),
            WebViewEvent::FocusGained if is_active => {
                self.page_had_focus.set(true);
                window.focus(&self.focus_handle, cx);
            }
            WebViewEvent::FocusGained if self.active_partner() == Some(id) => {
                self.page_had_focus.set(true);
                self.browser.select(id, tab_ops::now());
                window.focus(&self.focus_handle, cx);
                self.refresh_window_title(window);
            }
            WebViewEvent::FocusGained | WebViewEvent::Shortcut(_) => {}
            WebViewEvent::NewWindowRequested { url, request, .. }
                if url.starts_with("chrome-extension:") =>
            {
                self.open_popup_tab(url, request, OpenPosition::After(id), window, cx)
            }
            WebViewEvent::NewWindowRequested {
                url,
                user_initiated,
                request,
                ..
            } => self.on_new_window_request(id, url, user_initiated, request, window, cx),
            WebViewEvent::CloseRequested => self.close_tab(id, window, cx),
            WebViewEvent::FullscreenChanged(on) => self.on_page_fullscreen(id, on, window, cx),
            WebViewEvent::PermissionRequested {
                id: request,
                url,
                kind,
            } => self.on_permission_request(id, request, &url, kind),
            WebViewEvent::ExternalSchemeRequested {
                uri,
                origin,
                user_initiated,
            } => self.on_external_scheme_request(id, uri, origin, user_initiated),
            WebViewEvent::SecurityChanged(state) => self.on_security_change(id, state),
            WebViewEvent::AudioChanged { playing, muted } => {
                let was_playing = self
                    .runtime
                    .get(&id)
                    .is_some_and(|runtime| runtime.playing_audio);
                self.update_runtime(id, |runtime| {
                    runtime.playing_audio = playing;
                    runtime.muted = muted;
                });
                self.on_audio_changed(id, was_playing, playing, cx);
            }
            WebViewEvent::PeekRequested(url) => self.open_peek(url, window, cx),
            event @ (WebViewEvent::DownloadStarted { .. }
            | WebViewEvent::DownloadProgress { .. }
            | WebViewEvent::DownloadFinished { .. }) => {
                match &event {
                    WebViewEvent::DownloadStarted {
                        id: download, url, ..
                    } if let Some(extension) = self.store_package(id, url) => {
                        webview_host::cancel_download(*download);
                        self.install_from_webstore(extension, window, cx);
                        return;
                    }
                    WebViewEvent::DownloadStarted {
                        id: download, url, ..
                    } => {
                        self.download_flight = Some((*download, std::time::Instant::now()));
                        self.reveal_sidebar(cx);
                        if self.is_untouched_tab_for(id, url) {
                            self.hide_download_tab(id, *download, window, cx)
                        }
                    }
                    WebViewEvent::DownloadFinished { id: download, .. } => {
                        self.download_hosts.remove(download);
                    }
                    _ => {}
                }
                self.on_download_event(event, cx);
            }
        }
        if is_active {
            self.refresh_window_title(window);
        }
        cx.notify();
    }

    fn is_untouched_tab_for(&self, id: TabId, url: &str) -> bool {
        let never_showed_a_page = self
            .runtime
            .get(&id)
            .is_none_or(|runtime| !runtime.can_go_back);
        self.browser
            .tab(id)
            .is_some_and(|tab| !tab.is_pinned() && tab.title.is_empty() && tab.url == url)
            && never_showed_a_page
    }

    fn update_tab(&mut self, id: TabId, edit: impl FnOnce(&mut Tab)) {
        if let Some(tab) = self.browser.tab_mut(id) {
            edit(tab);
        }
    }

    fn update_runtime(&mut self, id: TabId, edit: impl FnOnce(&mut TabRuntime)) {
        if let Some(runtime) = self.runtime.get_mut(&id) {
            edit(runtime);
        }
    }

    fn refresh_window_title(&self, window: &mut Window) {
        let title = self
            .browser
            .active_tab()
            .map_or(APP_TITLE.into(), page::display_title);
        window.set_window_title(&title);
    }

    fn release_webview_focus(
        &mut self,
        _: &gpui::MouseDownEvent,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        self.take_keyboard_from_pages();
    }

    fn focus_active_page(&self) {
        if let Some(view) = self.active_view() {
            view.focus();
        }
    }

    // Foreground the main window before removing a popup, or Windows activates another app.
    fn dismiss_popup(
        &self,
        popup: gpui::AnyWindowHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let main = raw_window_handle::HasWindowHandle::window_handle(window)
            .ok()
            .map(|handle| handle.as_raw());
        cx.spawn_in(window, async move |shell, cx| {
            if let Some(main) = main {
                crate::native::bring_to_front(main);
            }
            cx.background_executor().timer(POPUP_SETTLE).await;
            popup.update(cx, |_, popup, _| popup.remove_window()).ok();
            cx.background_executor().timer(POPUP_SETTLE).await;
            shell
                .update_in(cx, |shell, window, cx| then(shell, window, cx))
                .ok();
        })
        .detach();
    }

    /// Like `dismiss_popup`, but runs `then` first and keeps the popup on
    /// screen for `fade` while it fades itself out.
    fn fade_out_popup(
        &self,
        popup: gpui::AnyWindowHandle,
        fade: std::time::Duration,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let main = raw_window_handle::HasWindowHandle::window_handle(window)
            .ok()
            .map(|handle| handle.as_raw());
        cx.spawn_in(window, async move |shell, cx| {
            if let Some(main) = main {
                crate::native::bring_to_front(main);
            }
            cx.background_executor().timer(POPUP_SETTLE).await;
            shell
                .update_in(cx, |shell, window, cx| then(shell, window, cx))
                .ok();
            cx.background_executor()
                .timer(fade.saturating_sub(POPUP_SETTLE))
                .await;
            popup.update(cx, |_, popup, _| popup.remove_window()).ok();
        })
        .detach();
    }

    fn open_targets(&mut self, targets: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        for url in targets {
            if page::is_blank(self.active_url()) {
                self.navigate_active(url, window, cx);
            } else {
                let id = self.open_tab(url, OpenPosition::End, window, cx);
                self.update_runtime(id, |runtime| runtime.focus_when_ready = true);
            }
        }
    }

    fn take_keyboard_from_pages(&self) {
        self.page_had_focus.set(false);
        if let Some(view) = self
            .runtime
            .values()
            .find_map(|runtime| runtime.view.as_ref())
        {
            view.focus_parent();
        }
    }

    fn start_sidebar_resize(
        &mut self,
        _: &gpui::MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.resizing_sidebar = true;
        cx.stop_propagation();
    }

    fn drag_sidebar_edge(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.resizing_sidebar {
            return;
        }
        let metrics = &Theme::of(cx).metrics;
        self.sidebar_width = event
            .position
            .x
            .clamp(metrics.sidebar_min_width, metrics.sidebar_max_width);
        cx.notify();
    }

    fn finish_sidebar_resize(
        &mut self,
        _: &gpui::MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.resizing_sidebar {
            self.resizing_sidebar = false;
            cx.notify();
        }
    }
}

fn receive_forwarded_targets(window: &mut Window, cx: &mut Context<Shell>) {
    let (sender, mut receiver) = unbounded::<Vec<String>>();
    crate::instance::listen(sender);
    let main = raw_window_handle::HasWindowHandle::window_handle(window)
        .ok()
        .map(|handle| handle.as_raw());
    cx.spawn_in(window, async move |shell, cx| {
        while let Some(targets) = receiver.next().await {
            if let Some(main) = main {
                crate::native::restore_and_focus(main);
            }
            let opened = shell.update_in(cx, |shell, window, cx| {
                shell.open_targets(targets, window, cx)
            });
            if opened.is_err() {
                break;
            }
        }
    })
    .detach();
}

fn forward_webview_events(
    mut receiver: UnboundedReceiver<TabEvent>,
    window: &mut Window,
    cx: &mut Context<Shell>,
) {
    cx.spawn_in(window, async move |shell, cx| {
        while let Some((id, event)) = receiver.next().await {
            let in_peek = shell
                .read_with(cx, |shell, _| shell.is_peek(id))
                .unwrap_or(false);
            let delivered = match event {
                WebViewEvent::Shortcut(_) if in_peek => shell
                    .update_in(cx, |shell, window, cx| shell.close_peek(window, cx))
                    .is_ok(),
                WebViewEvent::Shortcut(chord) => cx
                    .update(|window, cx| {
                        window.dispatch_keystroke(keystroke_for(&chord), cx);
                    })
                    .is_ok(),
                event => shell
                    .update_in(cx, |shell, window, cx| {
                        shell.on_webview_event(id, event, window, cx)
                    })
                    .is_ok(),
            };
            if !delivered {
                break;
            }
        }
    })
    .detach();
}

/// The last update check, for the settings page.
#[derive(Clone, PartialEq)]
pub(super) enum UpdateCheck {
    Idle,
    Checking,
    Done,
    Failed(String),
}

impl Shell {
    fn render_body(&self, reveal: f32, cx: &mut Context<Self>) -> gpui::AnyElement {
        let body = div()
            .relative()
            .flex()
            .flex_row()
            .flex_1()
            .min_h_0()
            .when(self.settings.hide_title_bar, |body| {
                body.pt(Theme::of(cx).metrics.frame_inset)
            });
        if !self.compact {
            return body
                .child(self.render_sidebar(cx))
                .child(self.render_content(cx))
                .into_any_element();
        }
        let sidebar = self.render_compact_sidebar(reveal, cx);
        body.child(self.render_content(cx))
            .children(sidebar)
            .into_any_element()
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        Theme::sync_scheme(self.wants_light(window), cx);
        if self.tab_drag.is_some() && !cx.has_active_drag() {
            self.tab_drag = None;
        }
        let hint = cx.has_active_drag() && self.browser.essentials().is_empty();
        if hint != self.essentials_hint.shown() {
            self.essentials_hint.set(hint);
        }
        if self.essentials_hint.advance() {
            window.request_animation_frame();
        }
        self.sync_panel(window, cx);
        let reveal = self.advance_reveal(window);
        let body = match self.fullscreen {
            Some(_) => self.render_fullscreen(cx),
            None => self.render_body(reveal, cx),
        };
        if self.row_motion.borrow().animating() {
            window.request_animation_frame();
        }
        let mut title_bar = TitleBar::new(self.render_nav_bar(cx));
        if !self.settings.site_title_in_sidebar {
            title_bar = title_bar.center(self.render_site_title(cx));
        }
        let favorites_bar = self.render_favorites_bar(cx);
        let (title_bar, favorites_bar, title_overlay) = if self.fullscreen.is_some() {
            (None, None, None)
        } else if self.settings.hide_title_bar {
            (
                None,
                None,
                self.render_title_overlay(title_bar, favorites_bar, window.viewport_size(), cx),
            )
        } else {
            (Some(title_bar), favorites_bar, None)
        };
        let title_loading = self.render_title_loading_bar(cx);
        let download_flight = self.render_download_flight(window.viewport_size(), cx);
        let theme = Theme::of(cx);
        let palette = self.palette(cx);

        div()
            .id("shell")
            .relative()
            .track_focus(&self.focus_handle)
            .key_context("Browser")
            .flex()
            .flex_col()
            .size_full()
            .bg(palette.sidebar)
            .font_family(theme.fonts.ui.clone())
            .font_weight(theme.fonts.ui_weight)
            .text_size(theme.metrics.text_md)
            .text_color(theme.colors.text)
            .when(self.resizing_sidebar, |root| root.cursor_col_resize())
            .capture_any_mouse_down(|_, window, _| crate::tooltip::clicked(window))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::release_webview_focus))
            .when(self.resizing_split, |root| {
                match self.active_split().map(|split| split.orientation) {
                    Some(browser_core::tabs::SplitOrientation::Vertical) => {
                        root.cursor_row_resize()
                    }
                    _ => root.cursor_col_resize(),
                }
            })
            .on_mouse_move(cx.listener(Self::drag_sidebar_edge))
            .on_mouse_move(cx.listener(Self::drag_split_divider))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::finish_sidebar_resize))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::finish_split_resize))
            .on_action(
                cx.listener(|this, _: &actions::NewTab, window, cx| this.new_tab(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &actions::NewPrivateWindow, window, cx| {
                    this.new_private_window(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &actions::ShowHistory, window, cx| {
                this.open_history(window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::CloseTab, window, cx| {
                this.close_active_tab(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &actions::ReopenClosedTab, window, cx| {
                    this.reopen_closed_tab(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &actions::NextTab, window, cx| this.cycle_tab(1, window, cx)),
            )
            .on_action(cx.listener(|this, _: &actions::PreviousTab, window, cx| {
                this.cycle_tab(-1, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::SelectTab1, window, cx| {
                this.select_tab_at(0, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::SelectTab2, window, cx| {
                this.select_tab_at(1, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::SelectTab3, window, cx| {
                this.select_tab_at(2, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::SelectTab4, window, cx| {
                this.select_tab_at(3, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::SelectTab5, window, cx| {
                this.select_tab_at(4, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::SelectTab6, window, cx| {
                this.select_tab_at(5, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::SelectTab7, window, cx| {
                this.select_tab_at(6, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::SelectTab8, window, cx| {
                this.select_tab_at(7, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::SelectLastTab, window, cx| {
                this.select_last_tab(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &actions::FocusAddressBar, window, cx| {
                    this.edit_current_address(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &actions::ZoomIn, window, cx| {
                this.on_site_menu_event(&SiteMenuEvent::ZoomIn, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::ZoomOut, window, cx| {
                this.on_site_menu_event(&SiteMenuEvent::ZoomOut, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::ResetZoom, window, cx| {
                this.on_site_menu_event(&SiteMenuEvent::ZoomReset, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::CopyUrl, window, cx| {
                this.copy_active_url(window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::GoBack, _, _| {
                if let Some(view) = this.active_view() {
                    view.go_back();
                }
            }))
            .on_action(cx.listener(|this, _: &actions::GoForward, _, _| {
                if let Some(view) = this.active_view() {
                    view.go_forward();
                }
            }))
            .on_action(cx.listener(|this, _: &actions::Reload, _, _| {
                if let Some(view) = this.active_view() {
                    view.reload();
                }
            }))
            .on_action(cx.listener(|this, _: &actions::HardReload, _, _| {
                if let Some(view) = this.active_view() {
                    view.hard_reload();
                }
            }))
            .on_action(cx.listener(|this, _: &actions::SwitchSpace1, window, cx| {
                this.switch_space(0, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::SwitchSpace2, window, cx| {
                this.switch_space(1, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::SwitchSpace3, window, cx| {
                this.switch_space(2, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::SwitchSpace4, window, cx| {
                this.switch_space(3, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::SwitchSpace5, window, cx| {
                this.switch_space(4, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::SwitchSpace6, window, cx| {
                this.switch_space(5, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::SwitchSpace7, window, cx| {
                this.switch_space(6, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::SwitchSpace8, window, cx| {
                this.switch_space(7, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::SwitchSpace9, window, cx| {
                this.switch_space(8, window, cx)
            }))
            .children(self.space_backdrop(cx))
            .children(title_bar)
            .on_action(
                cx.listener(|this, _: &actions::ToggleCompactMode, window, cx| {
                    this.toggle_compact(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &actions::ToggleFullscreen, window, cx| {
                    this.toggle_fullscreen(window, cx)
                }),
            )
            .children(favorites_bar)
            .child(body)
            .children(title_overlay)
            .children(title_loading)
            .children(download_flight)
    }
}
