//! Chromium extensions: what the user installed, loaded into the WebView2
//! profile, opened and removed. The hosting itself is the runtime's; see
//! `webview_host::extensions`. The popup of an extension opens as a peek
//! over the page, like a link preview.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use browser_core::extensions::{Installed, uninstall};
use browser_core::tabs::TabId;
use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, PathPromptOptions,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder,
};
use webview_host::BrowserView;

use super::Shell;
use super::persist::log_failure;
use crate::icons;
use crate::theme::Theme;
use crate::tooltip::tooltip;

const PANEL_TITLE: &str = "Extensions";
const STAGING_FOLDER: &str = "perseu-webstore";
const ADDING_EXTENSION: &str = "Adding the extension…";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DOWNLOAD_TIMEOUT_SECS: &str = "120";

/// Fetches a store package with Windows' own curl (the store answers plain
/// HTTPS GETs) and unpacks it into `staging`, returning that folder.
pub(super) fn download_and_unpack(url: &str, staging: &Path) -> anyhow::Result<PathBuf> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("curl.exe")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            DOWNLOAD_TIMEOUT_SECS,
            url,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| anyhow::anyhow!("could not start the download: {error}"))?;
    anyhow::ensure!(
        output.status.success(),
        "the download failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    if staging.exists() {
        std::fs::remove_dir_all(staging)?;
    }
    browser_core::webstore::unpack_crx(&output.stdout, staging)?;
    Ok(staging.to_path_buf())
}
const EMPTY_EXTENSIONS: &str = "Add unpacked extensions from the settings";

pub(super) struct ExtensionHost {
    /// Folder → runtime id, for the `chrome-extension://` addresses of popups.
    session: HashMap<String, String>,
    /// Whether this session already reconciled the runtime's profile; a
    /// profile only exists once the first page view does.
    loaded: bool,
    /// What is on disk, refreshed whenever it changes; drives the lists.
    installed: Vec<Installed>,
}

impl ExtensionHost {
    pub(super) fn new() -> Self {
        Self {
            session: HashMap::new(),
            loaded: false,
            installed: Vec::new(),
        }
    }

    pub(super) fn refresh(&mut self, dir: &Path, enabled: &BTreeMap<String, bool>) {
        self.installed = browser_core::extensions::scan(dir, enabled);
    }

    pub(super) fn installed(&self) -> &[Installed] {
        &self.installed
    }

    pub(super) fn runtime_id(&self, folder: &str) -> Option<&str> {
        self.session.get(folder).map(String::as_str)
    }
}

impl Shell {
    /// Rescans the extensions folder against the enabled flags in settings.
    pub(super) fn refresh_extension_list(&mut self, cx: &mut Context<Self>) {
        self.extensions
            .refresh(&self.paths.extensions(), &self.settings.extensions);
        cx.notify();
    }

    /// Loads the enabled extensions into the profile, once per session, on
    /// the first attached view: the profile exists from then on. Extensions
    /// the runtime kept from an earlier session are matched by their saved
    /// runtime id (name second) and reconciled with the saved enabled flags.
    pub(super) fn load_session_extensions(
        &mut self,
        view: Rc<BrowserView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.extensions.loaded {
            return;
        }
        self.extensions.loaded = true;
        let dir = self.paths.extensions();
        let wanted: Vec<(String, String)> = self
            .extensions
            .installed
            .iter()
            .map(|item| (item.folder.clone(), item.manifest.name.clone()))
            .collect();
        let enabled: HashSet<String> = self
            .extensions
            .installed
            .iter()
            .filter(|item| item.enabled)
            .map(|item| item.folder.clone())
            .collect();
        if wanted.is_empty() {
            return;
        }
        let mut changed_workers = HashSet::new();
        for (folder, name) in &wanted {
            match browser_core::extensions::adapt(&dir.join(folder)) {
                Ok(true) => {
                    changed_workers.insert(folder.clone());
                }
                Ok(false) => {}
                Err(error) => tracing::warn!("{name} may not work fully: {error:#}"),
            }
        }
        tracing::info!(
            "loading {} extension folder(s) into the profile",
            wanted.len()
        );
        cx.spawn_in(window, async move |shell, cx| {
            let saved_ids = browser_core::extensions::load_ids(&dir).unwrap_or_default();
            let mut present: HashMap<String, webview_host::extensions::ExtensionInfo> =
                HashMap::new();
            match webview_host::extensions::installed(&view) {
                Ok(list) => {
                    tracing::info!("the profile already holds {} extension(s)", list.len());
                    for info in list {
                        present.insert(info.id.clone(), info);
                    }
                }
                Err(error) => tracing::error!("could not list extensions: {error:#}"),
            }
            let by_name = |name: &str| {
                present
                    .values()
                    .find(|info| info.name == name && !name.is_empty())
            };
            let mut session = HashMap::new();
            let mut added = false;
            for (folder, name) in &wanted {
                let wanted_enabled = enabled.contains(folder);
                let existing = saved_ids
                    .get(folder)
                    .and_then(|id| present.get(id))
                    .or_else(|| by_name(name));
                match existing {
                    Some(info) => {
                        // Switching it off and on makes the runtime load the
                        // worker from disk again; the extension keeps its data.
                        if info.enabled
                            && wanted_enabled
                            && changed_workers.contains(folder)
                            && let Err(error) =
                                webview_host::extensions::set_enabled(&view, &info.id, false)
                                    .and_then(|()| {
                                        webview_host::extensions::set_enabled(&view, &info.id, true)
                                    })
                        {
                            tracing::error!("could not reload the extension {name}: {error:#}");
                        }
                        if info.enabled != wanted_enabled
                            && let Err(error) = webview_host::extensions::set_enabled(
                                &view,
                                &info.id,
                                wanted_enabled,
                            )
                        {
                            tracing::error!(
                                "could not {} the extension {name}: {error:#}",
                                if wanted_enabled { "enable" } else { "disable" }
                            );
                        }
                        session.insert(folder.clone(), info.id.clone());
                    }
                    None if wanted_enabled => {
                        match webview_host::extensions::add(&view, &dir.join(folder)) {
                            Ok(info) => {
                                tracing::info!("extension {name} loaded as {}", info.id);
                                session.insert(folder.clone(), info.id.clone());
                                added = true;
                            }
                            Err(error) => {
                                tracing::error!("could not load the extension {name}: {error:#}")
                            }
                        }
                    }
                    None => {}
                }
            }
            let reload = added;
            shell
                .update_in(cx, move |shell, _, cx| {
                    shell.extensions.session = session;
                    shell.persist_extension_ids();
                    if reload && let Some(view) = shell.active_view() {
                        view.reload();
                    }
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    /// Merges the session's folder → runtime ids into the persisted map, so
    /// the next session recognizes what this one loaded.
    fn persist_extension_ids(&self) {
        let mut ids =
            browser_core::extensions::load_ids(&self.paths.extensions()).unwrap_or_default();
        for (folder, id) in &self.extensions.session {
            ids.insert(folder.clone(), id.clone());
        }
        ids.retain(|folder, _| self.paths.extensions().join(folder).exists());
        if let Err(error) = browser_core::extensions::save_ids(&self.paths.extensions(), &ids) {
            tracing::error!("could not save extension ids: {error:#}");
        }
    }

    /// Persists the choice, then applies it to the runtime: enabling adds the
    /// extension if the profile does not have it yet, disabling turns it off.
    pub(super) fn set_extension_enabled(
        &mut self,
        folder: String,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_settings(
            |settings| {
                settings.extensions.insert(folder.clone(), enabled);
            },
            cx,
        );
        self.refresh_extension_list(cx);
        let Some(view) = self.any_view() else {
            // No page view yet: the session load applies it when one exists.
            return;
        };
        let path = self.paths.extensions().join(&folder);
        let id = self.extensions.runtime_id(&folder).map(str::to_string);
        cx.spawn_in(window, async move |shell, cx| {
            let outcome = match id.as_deref() {
                Some(id) => {
                    webview_host::extensions::set_enabled(&view, id, enabled).map(|()| None)
                }
                None if enabled => {
                    webview_host::extensions::add(&view, &path).map(|info| Some(info.id))
                }
                None => Ok(None),
            };
            let reload = outcome.is_ok() && enabled;
            let new_id = outcome.as_ref().ok().and_then(|id| id.clone());
            shell
                .update_in(cx, move |shell, window, cx| {
                    match &outcome {
                        Ok(_) => {
                            if let Some(id) = new_id {
                                shell.extensions.session.insert(folder.clone(), id);
                                shell.persist_extension_ids();
                            }
                            if reload && let Some(view) = shell.active_view() {
                                view.reload();
                            }
                        }
                        Err(error) => shell.show_toast(
                            format!("Extension change failed: {error:#}"),
                            window,
                            cx,
                        ),
                    }
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    /// Installs an unpacked extension folder the user picked, enabled.
    pub(super) fn install_extension(
        &mut self,
        picked: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match browser_core::extensions::install(&picked, &self.paths.extensions()) {
            Ok(folder) => self.set_extension_enabled(folder, true, window, cx),
            Err(error) => self.show_toast(
                format!("Could not add the extension: {error:#}"),
                window,
                cx,
            ),
        }
    }

    /// The extension a download is for when it is the Web Store's own
    /// "Add to Chrome" package, which would fail: the store refuses this
    /// browser. A failed redirect lands on `permanently-removed.invalid`.
    pub(super) fn store_package(&self, tab: TabId, url: &str) -> Option<String> {
        browser_core::webstore::package_download_id(url).or_else(|| {
            let page = &self.browser.tab(tab)?.url;
            let from_store = url.contains("permanently-removed.invalid")
                || url.to_ascii_lowercase().ends_with(".crx");
            from_store
                .then(|| browser_core::webstore::extension_id(page))
                .flatten()
        })
    }

    /// Downloads extension `id` from the Chrome Web Store, unpacks it and
    /// installs it, enabled.
    pub(super) fn install_from_webstore(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(version) = webview_host::runtime_version() else {
            self.show_toast(
                "Could not add the extension: WebView2 is missing",
                window,
                cx,
            );
            return;
        };
        let url = browser_core::webstore::crx_url(&id, &version);
        if self.store_adding.is_some() {
            return;
        }
        let staging = std::env::temp_dir().join(STAGING_FOLDER).join(&id);
        self.store_adding = Some(id.clone());
        self.show_toast(ADDING_EXTENSION, window, cx);
        cx.spawn_in(window, async move |shell, cx| {
            let unpacked = cx
                .background_executor()
                .spawn(async move { download_and_unpack(&url, &staging) })
                .await;
            shell
                .update_in(cx, |shell, window, cx| match unpacked {
                    Ok(folder) => shell.offer_extension(id, folder, window, cx),
                    Err(error) => {
                        shell.store_adding = None;
                        shell.show_toast(
                            format!("Could not add the extension: {error:#}"),
                            window,
                            cx,
                        )
                    }
                })
                .ok();
        })
        .detach();
    }

    /// Asks for an unpacked extension folder and installs it.
    pub(super) fn choose_extension_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add this extension".into()),
        });
        cx.spawn_in(window, async move |shell, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(folder) = paths.into_iter().next() else {
                return;
            };
            shell
                .update_in(cx, |shell, window, cx| {
                    shell.install_extension(folder, window, cx)
                })
                .ok();
        })
        .detach();
    }

    /// Removes an extension from the settings, the folder and the runtime.
    pub(super) fn uninstall_extension(
        &mut self,
        folder: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.extensions.session.remove(&folder);
        self.update_settings(
            |settings| {
                settings.extensions.remove(&folder);
            },
            cx,
        );
        self.persist_extension_ids();
        log_failure(uninstall(&self.paths.extensions(), &folder));
        self.refresh_extension_list(cx);
        if let (Some(id), Some(view)) = (id, self.any_view()) {
            cx.spawn(async move |shell, cx| {
                if let Err(error) = webview_host::extensions::remove(&view, &id) {
                    tracing::error!("could not remove the extension: {error:#}");
                }
                shell
                    .update(cx, |shell, cx| shell.refresh_extension_list(cx))
                    .ok();
            })
            .detach();
        }
    }

    /// Opens an extension's toolbar popup as a peek over the page.
    pub(super) fn open_extension_popup(
        &mut self,
        folder: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.extensions.runtime_id(folder).map(str::to_string) else {
            self.show_toast("The extension is not loaded yet", window, cx);
            return;
        };
        let url = self
            .extensions
            .installed
            .iter()
            .find(|item| item.folder == folder)
            .and_then(|item| item.popup(&id));
        match url {
            Some(url) => self.open_peek(url, window, cx),
            None => self.show_toast("This extension has no popup", window, cx),
        }
    }

    /// The extensions button beside the address: one enabled extension
    /// with a popup opens it, several open a menu to pick from.
    pub(super) fn open_extensions_menu(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let with_popup: Vec<(String, String)> = self
            .extensions
            .installed()
            .iter()
            .filter(|item| item.enabled && item.manifest.action.is_some())
            .map(|item| (item.folder.clone(), item.manifest.name.clone()))
            .collect();
        match with_popup.as_slice() {
            [] => self.show_toast("No extension with a menu is enabled", window, cx),
            [(folder, _)] => {
                let folder = folder.clone();
                self.open_extension_popup(&folder, window, cx)
            }
            _ => {
                let entries = with_popup
                    .iter()
                    .enumerate()
                    .map(|(index, (_, name))| {
                        crate::context_menu::MenuEntry::item(name.clone(), index)
                            .with_icon(Some(icons::PUZZLE))
                    })
                    .collect();
                let below = gpui::point(position.x, position.y + Theme::of(cx).metrics.radius_md);
                self.open_menu(
                    entries,
                    below,
                    window,
                    cx,
                    move |shell, index, window, cx| {
                        if let Some((folder, _)) = with_popup.get(index) {
                            shell.open_extension_popup(folder, window, cx)
                        }
                    },
                );
            }
        }
    }

    pub(super) fn has_extension_menus(&self) -> bool {
        self.extensions
            .installed()
            .iter()
            .any(|item| item.enabled && item.manifest.action.is_some())
    }

    /// A view of the regular profile, never a private one.
    pub(super) fn any_view(&self) -> Option<Rc<BrowserView>> {
        self.runtime
            .iter()
            .filter(|(id, _)| !self.browser.is_private(**id))
            .find_map(|(_, runtime)| runtime.view.clone())
            .or_else(|| self.download_hosts.values().next().cloned())
    }

    pub(super) fn render_extensions_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let rows: Vec<AnyElement> = self
            .extensions
            .installed()
            .iter()
            .enumerate()
            .map(|(index, item)| extension_row(item, index, cx).into_any_element())
            .collect();
        let empty = rows.is_empty();
        let theme = Theme::of(cx);
        let hover_bg = theme.colors.surface_hover;

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .id("extensions-back")
                    .flex()
                    .flex_row()
                    .flex_shrink_0()
                    .items_center()
                    .gap(theme.metrics.radius_md)
                    .h(theme.metrics.row_height)
                    .px(theme.metrics.radius_md)
                    .rounded(theme.metrics.radius_md)
                    .text_size(theme.metrics.text_sm)
                    .text_color(theme.colors.text_muted)
                    .hover(move |style| style.bg(hover_bg))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.extensions_open = false;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .font_family(theme.fonts.icons.clone())
                            .text_size(theme.metrics.nav_glyph)
                            .child(icons::BACK),
                    )
                    .child(PANEL_TITLE),
            )
            .child(
                div()
                    .id("extensions-list")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows)
                    .when(empty, |list| {
                        list.child(
                            div()
                                .p(theme.metrics.radius_md)
                                .text_size(theme.metrics.text_sm)
                                .text_color(theme.colors.text_muted)
                                .child(EMPTY_EXTENSIONS),
                        )
                    }),
            )
    }
}

fn extension_row(item: &Installed, index: usize, cx: &mut Context<Shell>) -> impl IntoElement {
    let theme = Theme::of(cx);
    let hover_bg = theme.colors.surface_hover;
    let folder = item.folder.clone();
    let has_popup = item
        .manifest
        .action
        .as_ref()
        .and_then(|action| action.default_popup.as_deref())
        .is_some();
    let tip = if item.enabled && has_popup {
        "Open popup"
    } else if item.enabled {
        "No popup"
    } else {
        "Disabled"
    };

    div()
        .id(("extension", index))
        .flex()
        .flex_col()
        .flex_shrink_0()
        .justify_center()
        .py(theme.metrics.radius_sm)
        .px(theme.metrics.radius_md)
        .rounded(theme.metrics.radius_md)
        .hover(move |style| style.bg(hover_bg))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.open_extension_popup(&folder, window, cx);
        }))
        .tooltip(tooltip(tip))
        .when(!item.enabled, |row| row.text_color(theme.colors.text_muted))
        .child(div().truncate().child(item.manifest.name.clone()))
        .child(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .gap(theme.metrics.radius_md)
                .text_size(theme.metrics.text_sm)
                .text_color(theme.colors.text_muted)
                .child(div().min_w_0().truncate().child(format!(
                    "{} · {}",
                    item.manifest.version,
                    item.manifest.generation()
                )))
                .child(
                    div()
                        .flex_shrink_0()
                        .font_family(theme.fonts.icons.clone())
                        .child(if item.enabled {
                            icons::CHECK
                        } else {
                            icons::CLOSE
                        }),
                ),
        )
}
