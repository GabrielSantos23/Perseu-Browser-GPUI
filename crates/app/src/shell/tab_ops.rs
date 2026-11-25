use std::path::PathBuf;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use browser_core::omnibox::BLANK_PAGE;
use browser_core::tabs::{CloseOutcome, OpenPosition, Section, TabId, Timestamp};
use futures::channel::mpsc::UnboundedSender;
use gpui::{Context, Window};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use webview_host::{BrowserView, BrowserViewOptions, EventSink, LinkModifier, ShortcutFilter};

use super::{Shell, TabEvent};
use crate::actions::is_browser_shortcut;
use crate::page::{self, TabRuntime};
use crate::theme::{Theme, to_rgba8};

pub(super) fn now() -> Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as Timestamp)
}

impl Shell {
    pub(super) fn open_blank_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_tab(BLANK_PAGE.to_string(), OpenPosition::End, window, cx);
    }

    pub(super) fn open_tab(
        &mut self,
        url: String,
        position: OpenPosition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> TabId {
        let id = self.browser.open(url, position, now());
        self.runtime.insert(id, TabRuntime::default());
        self.show_active_tab(window, cx);
        id
    }

    /// A page's new window (`window.open`, an extension's `windows.create`)
    /// as a tab whose view the opener gets back as that window, so sign-in
    /// popups can reach `window.opener`.
    pub(super) fn open_popup_tab(
        &mut self,
        url: String,
        request: Option<u64>,
        position: OpenPosition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = self.browser.open(url, position, now());
        self.runtime.insert(
            id,
            TabRuntime {
                adopt: request,
                ..TabRuntime::default()
            },
        );
        self.show_active_tab(window, cx);
    }

    pub(super) fn activate(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        if self.browser.select(id, now()) {
            self.show_active_tab(window, cx);
        }
    }

    pub(super) fn show_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_page = None;
        self.history_page = None;
        self.take_keyboard_from_pages();
        if let Some(super::fullscreen::Fullscreen::Page(tab)) = self.fullscreen
            && self.browser.active_tab_id() != Some(tab)
        {
            self.leave_fullscreen(window, cx);
        }
        let shown = [self.browser.active_tab_id(), self.active_partner()];
        for id in shown.into_iter().flatten() {
            self.ensure_view(id, window, cx);
        }
        self.show_only_active_view();
        self.refresh_window_title(window);
        cx.notify();
    }

    pub(super) fn navigate_active(
        &mut self,
        url: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.browser.active_tab_id() else {
            return;
        };
        if let Some(view) = self.active_view() {
            view.navigate(&url);
            view.focus();
        } else {
            self.update_tab(id, |tab| tab.url = url);
            self.update_runtime(id, |runtime| runtime.focus_when_ready = true);
            self.ensure_view(id, window, cx);
        }
        cx.notify();
    }

    pub(super) fn close_tab(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        self.deny_prompts_of(id);
        match self.browser.close(id, now()) {
            Some(CloseOutcome::Removed) => {
                self.runtime.remove(&id);
            }
            Some(CloseOutcome::Unloaded) => self.unload(id),
            None => return,
        }
        if self.browser.active_tab_id().is_some() {
            self.show_active_tab(window, cx);
        } else {
            self.open_blank_tab(window, cx);
        }
    }

    pub(super) fn close_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.browser.active_tab_id() {
            self.close_tab(id, window, cx);
        }
    }

    pub(super) fn reset_pinned_tab(&mut self, id: TabId, cx: &mut Context<Self>) {
        let Some(tab) = self.browser.tab_mut(id) else {
            return;
        };
        tab.reset_to_pinned();
        let url = tab.url.clone();
        if let Some(view) = self
            .runtime
            .get(&id)
            .and_then(|runtime| runtime.view.clone())
        {
            view.navigate(&url);
        }
        cx.notify();
    }

    pub(super) fn reopen_closed_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.browser.space_mut().tabs.reopen_closed(now()) {
            self.runtime.insert(id, TabRuntime::default());
            self.activate(id, window, cx);
        }
    }

    pub(super) fn cycle_tab(&mut self, offset: isize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.browser.space_mut().tabs.select_relative(offset, now()) {
            self.activate(id, window, cx);
        }
    }

    pub(super) fn select_tab_at(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Ctrl+N counts the essentials first, as the sidebar shows them.
        let essentials: Vec<TabId> = if self.browser.private_shown() {
            Vec::new()
        } else {
            self.browser.essentials().iter().map(|tab| tab.id).collect()
        };
        if let Some(&id) = essentials.get(index) {
            self.activate(id, window, cx);
            return;
        }
        let index = index - essentials.len();
        if let Some(id) = self.browser.space_mut().tabs.select_index(index, now()) {
            self.activate(id, window, cx);
        }
    }

    pub(super) fn select_last_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.browser.space_mut().tabs.select_last(now()) {
            self.activate(id, window, cx);
        }
    }

    pub(super) fn drop_tab_before(&mut self, id: TabId, target: TabId, cx: &mut Context<Self>) {
        self.leave_essentials(id);
        if self.browser.space_mut().tabs.move_before(id, target) {
            cx.notify();
        }
    }

    pub(super) fn drop_tab_at_end(&mut self, id: TabId, section: Section, cx: &mut Context<Self>) {
        self.leave_essentials(id);
        if self
            .browser
            .space_mut()
            .tabs
            .move_tab(id, section, usize::MAX)
        {
            cx.notify();
        }
    }

    pub(super) fn drop_on_essentials(
        &mut self,
        id: TabId,
        target: Option<TabId>,
        cx: &mut Context<Self>,
    ) {
        let changed = match target {
            Some(target) if self.browser.is_essential(id) => {
                self.browser.move_essential_before(id, target)
            }
            _ if self.browser.is_essential(id) => false,
            Some(target) => {
                self.browser.make_essential(id, now())
                    && self.browser.move_essential_before(id, target)
            }
            None => self.browser.make_essential(id, now()),
        };
        if changed {
            cx.notify();
        }
    }

    fn leave_essentials(&mut self, id: TabId) {
        if self.browser.is_essential(id) {
            self.browser.remove_essential(id, usize::MAX, now());
        }
    }

    fn unload(&mut self, id: TabId) {
        if let Some(runtime) = self.runtime.get_mut(&id) {
            *runtime = TabRuntime {
                favicon: runtime.favicon.take(),
                ..TabRuntime::default()
            };
        }
    }

    /// Loads the essentials in the background, so web apps kept there (mail,
    /// chat) receive messages and notify without being opened first.
    pub(super) fn load_essentials(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.browser.private_shown() {
            return;
        }
        let ids: Vec<TabId> = self.browser.essentials().iter().map(|tab| tab.id).collect();
        for id in ids {
            self.ensure_view(id, window, cx);
        }
    }

    fn ensure_view(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(url) = self.browser.tab(id).map(|tab| tab.url.clone()) else {
            return;
        };
        let runtime = self.runtime.entry(id).or_default();
        if runtime.view.is_some() || runtime.view_pending || page::is_blank(&url) {
            return;
        }
        let adopt = runtime.adopt.take();
        let parent = match window.window_handle() {
            Ok(handle) => handle.as_raw(),
            Err(error) => {
                tracing::error!("main window has no native handle: {error}");
                return;
            }
        };
        runtime.view_pending = true;
        let page = url.clone();
        let request = ViewRequest {
            id,
            url: if adopt.is_some() { String::new() } else { url },
            parent,
            background: to_rgba8(Theme::of(cx).colors.surface),
            events: self.events.clone(),
            intercept: Rc::new(is_browser_shortcut),
            peek_modifier: Some(self.link_modifier()),
            data_directory: self.paths.webview_data(),
            download_folder: self.effective_downloads(),
            extensions_enabled: true,
            reputation_checks: self.settings.download_safety_checks,
            private: self.browser.is_private(id),
        };
        cx.spawn_in(window, async move |shell, cx| {
            let view = request.create();
            match (&view, adopt) {
                (Some(view), Some(adopt)) => {
                    if !view.adopt_new_window(adopt) {
                        view.navigate(&page);
                    }
                }
                (None, Some(adopt)) => webview_host::decline_new_window(adopt),
                _ => {}
            }
            shell
                .update_in(cx, |shell, window, cx| {
                    shell.attach_view(id, view, window, cx)
                })
                .ok();
        })
        .detach();
    }

    fn attach_view(
        &mut self,
        id: TabId,
        view: Option<Rc<BrowserView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(runtime) = self.runtime.get_mut(&id) else {
            return;
        };
        if !runtime.view_pending {
            return;
        }
        runtime.view_pending = false;
        if let Some(view) = &view {
            view.set_color_scheme(self.page_scheme());
        }
        let Some(runtime) = self.runtime.get_mut(&id) else {
            return;
        };
        runtime.view = view;
        // The WebView2 profile exists from the first attached view on, so this
        // is where the session's extensions load into it.
        if let Some(attached) = self
            .runtime
            .get(&id)
            .and_then(|runtime| runtime.view.clone())
        {
            self.load_session_extensions(attached, window, cx);
        }
        if std::mem::take(&mut self.pending_profile_clear)
            && let Some(view) = self
                .runtime
                .get(&id)
                .and_then(|runtime| runtime.view.clone())
        {
            view.clear_browsing_data();
        }
        let Some(runtime) = self.runtime.get_mut(&id) else {
            return;
        };
        let focus_page = std::mem::take(&mut runtime.focus_when_ready);
        self.show_only_active_view();
        if focus_page && self.browser.active_tab_id() == Some(id) {
            window.focus(&self.focus_handle, cx);
            if let Some(view) = self.active_view() {
                view.focus();
            }
        }
        cx.notify();
    }

    pub(super) fn show_only_active_view(&self) {
        let shown = if self.fullscreen.is_some() {
            [self.fullscreen_tab(), None]
        } else if self.settings_page.is_some() || self.history_page.is_some() {
            [None, None]
        } else {
            [self.browser.active_tab_id(), self.active_partner()]
        };
        for (id, runtime) in &self.runtime {
            if let Some(view) = &runtime.view {
                view.set_visible(shown.contains(&Some(*id)));
            }
        }
    }
}

pub(super) struct ViewRequest {
    pub(super) id: TabId,
    pub(super) url: String,
    pub(super) parent: RawWindowHandle,
    pub(super) background: (u8, u8, u8, u8),
    pub(super) events: UnboundedSender<TabEvent>,
    pub(super) intercept: ShortcutFilter,
    pub(super) peek_modifier: Option<LinkModifier>,
    pub(super) data_directory: PathBuf,
    pub(super) download_folder: Option<PathBuf>,
    pub(super) extensions_enabled: bool,
    pub(super) reputation_checks: bool,
    pub(super) private: bool,
}

impl ViewRequest {
    pub(super) fn create(self) -> Option<Rc<BrowserView>> {
        let Self {
            id,
            url,
            parent,
            background,
            events,
            intercept,
            peek_modifier,
            data_directory,
            download_folder,
            extensions_enabled,
            reputation_checks,
            private,
        } = self;
        let sink: EventSink = Rc::new(move |event| {
            let _ = events.unbounded_send((id, event));
        });
        // A private tab starts on the private page rather than a blank one.
        let private_page = private && url == BLANK_PAGE;
        let options = BrowserViewOptions {
            url: if private_page { "" } else { &url },
            background,
            intercept,
            peek_modifier,
            data_directory: Some(&data_directory),
            download_folder: download_folder.as_deref(),
            extensions_enabled,
            reputation_checks,
            private,
        };
        match BrowserView::new(parent, options, sink) {
            Ok(view) => {
                if private_page {
                    view.load_html(super::private::PRIVATE_PAGE);
                }
                Some(Rc::new(view))
            }
            Err(error) => {
                tracing::error!("{error:#}");
                None
            }
        }
    }
}
