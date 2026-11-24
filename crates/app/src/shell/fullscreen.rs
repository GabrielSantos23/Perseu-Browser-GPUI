//! Fullscreen: F11 for the browser, or a page element (a video player)
//! asking for it. Either way the window fills the screen with the page alone.

use browser_core::tabs::TabId;
use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, Window, canvas, div};

use super::Shell;
use super::content::sync_webview_bounds;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Fullscreen {
    /// F11: the active tab, or the settings or history page.
    Browser,
    /// The page of this tab put one of its elements in fullscreen.
    Page(TabId),
}

impl Shell {
    pub(super) fn toggle_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.fullscreen {
            Some(_) => self.leave_fullscreen(window, cx),
            None => self.enter_fullscreen(Fullscreen::Browser, window, cx),
        }
    }

    pub(super) fn on_page_fullscreen(
        &mut self,
        tab: TabId,
        on: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match (on, self.fullscreen) {
            (true, None | Some(Fullscreen::Browser)) => {
                self.enter_fullscreen(Fullscreen::Page(tab), window, cx)
            }
            (false, Some(Fullscreen::Page(shown))) if shown == tab => {
                self.leave_fullscreen(window, cx)
            }
            _ => {}
        }
    }

    fn enter_fullscreen(&mut self, mode: Fullscreen, window: &mut Window, cx: &mut Context<Self>) {
        self.fullscreen = Some(mode);
        if !window.is_fullscreen() {
            window.toggle_fullscreen();
        }
        self.show_only_active_view();
        cx.notify();
    }

    pub(super) fn leave_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(Fullscreen::Page(tab)) = self.fullscreen.take()
            && let Some(view) = self
                .runtime
                .get(&tab)
                .and_then(|runtime| runtime.view.clone())
        {
            view.exit_fullscreen();
        }
        if window.is_fullscreen() {
            window.toggle_fullscreen();
        }
        self.show_only_active_view();
        cx.notify();
    }

    /// The tab whose page fills the screen, if one does.
    pub(super) fn fullscreen_tab(&self) -> Option<TabId> {
        match self.fullscreen? {
            Fullscreen::Page(tab) => Some(tab),
            Fullscreen::Browser if self.settings_page.is_some() || self.history_page.is_some() => {
                None
            }
            Fullscreen::Browser => self.browser.active_tab_id(),
        }
    }

    pub(super) fn render_fullscreen(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(tab) = self.fullscreen_tab() else {
            return self.render_content(cx).into_any_element();
        };
        let view = self
            .runtime
            .get(&tab)
            .and_then(|runtime| runtime.view.clone());
        div()
            .size_full()
            .child(
                canvas(
                    move |bounds, window, _| {
                        sync_webview_bounds(view, bounds, 0.0, 0.0, 0.0, window)
                    },
                    |_, _, _, _| {},
                )
                .size_full(),
            )
            .into_any_element()
    }
}
