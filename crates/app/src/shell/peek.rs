use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use browser_core::tabs::{OpenPosition, Tab, TabId};
use gpui::{
    AnyElement, AnyWindowHandle, App, AppContext, Bounds, Context, Entity, EventEmitter,
    FocusHandle, InteractiveElement, IntoElement, KeyBinding, MouseButton, ParentElement, Pixels,
    Render, SharedString, StatefulInteractiveElement, Styled, Subscription, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, actions, canvas, div,
    point, size,
};
use raw_window_handle::HasWindowHandle;
use webview_host::{BrowserView, KeyChord, ShortcutFilter, WebViewEvent};

use super::Shell;
use super::content::sync_webview_bounds;
use super::persist::log_failure;
use super::tab_ops::{ViewRequest, now};
use crate::actions::is_browser_shortcut;
use crate::icons;
use crate::native;
use crate::page::{self, TabRuntime};
use crate::theme::{Theme, to_rgba8};
use crate::tooltip::tooltip;

actions!(peek, [Dismiss]);

const KEY_CONTEXT: &str = "Peek";
const ESCAPE_KEY: &str = "escape";
const OPEN_AS_TAB: &str = "Open as tab";
const MAX_MARGIN_SHARE: f32 = 0.125;
/// Chrome's limits for a toolbar popup, which grows to fit its page.
const POPUP_MIN: (f32, f32) = (320., 120.);
const POPUP_MAX: (f32, f32) = (800., 600.);
const POPUP_REMEASURE: Duration = Duration::from_millis(200);

/// An extension popup's card: anchored at the top of the page area, on the
/// side the extensions button is, as large as the popup page needs.
struct Fit {
    area: Bounds<Pixels>,
    from_right: bool,
    page: (f32, f32),
}

pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(ESCAPE_KEY, Dismiss, Some(KEY_CONTEXT))]);
}

fn is_plain_escape(chord: &KeyChord) -> bool {
    chord.key == ESCAPE_KEY && !chord.ctrl && !chord.alt && !chord.shift
}

pub(super) struct OpenPeek {
    window: AnyWindowHandle,
    view: Entity<PeekView>,
    tab: Tab,
    runtime: TabRuntime,
    promoted: Rc<Cell<bool>>,
    _events: Subscription,
}

pub(super) enum PeekEvent {
    Close,
    Promote,
}

pub(super) struct PeekView {
    focus_handle: FocusHandle,
    card: Bounds<Pixels>,
    title: SharedString,
    host: SharedString,
    page: Option<Rc<BrowserView>>,
    fit: Option<Fit>,
    _activation: Subscription,
}

impl EventEmitter<PeekEvent> for PeekView {}

impl PeekView {
    fn new(
        card: Bounds<Pixels>,
        fit: Option<Fit>,
        tab: &Tab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        let activation = cx.observe_window_activation(window, |_, window, cx| {
            if !window.is_window_active() {
                cx.emit(PeekEvent::Close);
            }
        });
        Self {
            focus_handle,
            card,
            title: page::display_title(tab),
            host: page::host(&tab.url),
            page: None,
            fit,
            _activation: activation,
        }
    }

    /// The card of an extension popup, sized around its page.
    fn fitted_card(&self, cx: &App) -> Option<Bounds<Pixels>> {
        let fit = self.fit.as_ref()?;
        let metrics = &Theme::of(cx).metrics;
        let border = gpui::px(2.);
        let width = gpui::px(fit.page.0) + metrics.frame_inset * 2. + border;
        let height = gpui::px(fit.page.1)
            + metrics.row_height
            + metrics.radius_md
            + metrics.frame_inset
            + border;
        let gap = metrics.radius_md;
        let width = width.min(fit.area.size.width - gap * 2.);
        let height = height.min(fit.area.size.height - gap * 2.);
        let x = if fit.from_right {
            fit.area.origin.x + fit.area.size.width - gap - width
        } else {
            fit.area.origin.x + gap
        };
        Some(Bounds::new(
            point(x, fit.area.origin.y + gap),
            size(width, height),
        ))
    }

    /// Grows an extension popup's card to the size its page reports, up to
    /// Chrome's popup limits, and keeps checking while the popup is open.
    fn keep_fitting(&self, cx: &mut Context<Self>) {
        let measured: Rc<Cell<Option<(f64, f64)>>> = Rc::default();
        cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor().timer(POPUP_REMEASURE).await;
                let open = view.update(cx, |view, cx| {
                    if let Some((width, height)) = measured.take()
                        && let Some(fit) = view.fit.as_mut()
                    {
                        let page = (
                            fit.page.0.max(width as f32).min(POPUP_MAX.0),
                            fit.page.1.max(height as f32).min(POPUP_MAX.1),
                        );
                        if page != fit.page {
                            fit.page = page;
                            cx.notify();
                        }
                    }
                    if let Some(page) = &view.page {
                        let measured = measured.clone();
                        page.measure_content(move |width, height| {
                            measured.set(Some((width, height)))
                        });
                    }
                });
                if open.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn show_tab(&mut self, tab: &Tab, cx: &mut Context<Self>) {
        self.title = page::display_title(tab);
        self.host = page::host(&tab.url);
        cx.notify();
    }

    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx);
        let metrics = &theme.metrics;
        let hover_bg = theme.colors.surface_hover;
        let text = theme.colors.text;
        let host = (self.host != self.title && self.fit.is_none()).then(|| self.host.clone());
        let icon = |glyph: icons::Icon| {
            div()
                .font_family(theme.fonts.icons.clone())
                .text_size(metrics.nav_glyph)
                .child(glyph)
        };

        div()
            .flex()
            .flex_row()
            .flex_shrink_0()
            .items_center()
            .gap(metrics.radius_md)
            .h(metrics.row_height + metrics.radius_md)
            .pl(metrics.radius_md * 2.)
            .pr(metrics.frame_inset)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(metrics.radius_md)
                    .child(div().min_w_0().truncate().child(self.title.clone()))
                    .children(host.map(|host| {
                        div()
                            .flex_shrink_0()
                            .text_size(metrics.text_sm)
                            .text_color(theme.colors.text_muted)
                            .child(host)
                    })),
            )
            .child(
                div()
                    .id("peek-promote")
                    .flex()
                    .flex_row()
                    .flex_shrink_0()
                    .items_center()
                    .gap(metrics.radius_md)
                    .h(metrics.nav_button_size)
                    .px(metrics.radius_md * 1.5)
                    .rounded(metrics.radius_md)
                    .text_color(theme.colors.text_muted)
                    .hover(move |style| style.bg(hover_bg).text_color(text))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(PeekEvent::Promote)))
                    .child(icon(icons::OPEN_IN_TAB))
                    .child(OPEN_AS_TAB),
            )
            .child(
                div()
                    .id("peek-close")
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .justify_center()
                    .size(metrics.nav_button_size)
                    .rounded(metrics.radius_md)
                    .text_color(theme.colors.text_muted)
                    .hover(move |style| style.bg(hover_bg).text_color(text))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(PeekEvent::Close)))
                    .tooltip(tooltip("Close preview"))
                    .child(icon(icons::CLOSE)),
            )
            .into_any_element()
    }
}

impl Render for PeekView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.render_header(cx);
        let theme = Theme::of(cx);
        let metrics = &theme.metrics;
        let page = self.page.clone();
        let corner_radius = f32::from(metrics.radius_frame) - 1.0;
        let card = self.fitted_card(cx).unwrap_or(self.card);
        let scrim = if self.fit.is_some() {
            gpui::transparent_black()
        } else {
            theme.colors.scrim
        };

        div()
            .id("peek-backdrop")
            .track_focus(&self.focus_handle)
            .key_context(KEY_CONTEXT)
            .size_full()
            .bg(scrim)
            .font_family(theme.fonts.ui.clone())
            .font_weight(theme.fonts.ui_weight)
            .text_size(metrics.text_md)
            .text_color(theme.colors.text)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(PeekEvent::Close)),
            )
            .on_action(cx.listener(|_, _: &Dismiss, _, cx| cx.emit(PeekEvent::Close)))
            .child(
                div()
                    .id("peek-card")
                    .absolute()
                    .left(card.origin.x)
                    .top(card.origin.y)
                    .w(card.size.width)
                    .h(card.size.height)
                    .flex()
                    .flex_col()
                    .rounded(metrics.radius_peek)
                    .bg(theme.colors.surface_raised)
                    .border_1()
                    .border_color(theme.colors.border)
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(header)
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .mx(metrics.frame_inset)
                            .mb(metrics.frame_inset)
                            .rounded(metrics.radius_frame)
                            .bg(theme.colors.surface)
                            .child(
                                canvas(
                                    move |bounds, window, _| {
                                        sync_webview_bounds(
                                            page,
                                            bounds,
                                            corner_radius,
                                            0.0,
                                            0.0,
                                            window,
                                        )
                                    },
                                    |_, _, _, _| {},
                                )
                                .size_full(),
                            ),
                    ),
            )
    }
}

impl Shell {
    pub(super) fn is_peek(&self, id: TabId) -> bool {
        self.peek.as_ref().is_some_and(|peek| peek.tab.id == id)
    }

    pub(super) fn open_peek(&mut self, url: String, window: &mut Window, cx: &mut Context<Self>) {
        self.close_peek(window, cx);
        let tab = Tab::new(url, now());
        let (popup_bounds, card) = self.peek_bounds(window, cx);
        let fit = tab.url.starts_with("chrome-extension:").then(|| Fit {
            area: self.beside_sidebar(window, cx),
            from_right: !self.settings.site_title_in_sidebar,
            page: POPUP_MIN,
        });
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(popup_bounds)),
            titlebar: None,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Transparent,
            ..WindowOptions::default()
        };

        let mut opened = None;
        let result = cx.open_window(options, |popup, cx| {
            let parent = popup.window_handle().ok().map(|handle| handle.as_raw());
            if let Some(parent) = parent {
                native::remove_frame_shadow_and_border(parent);
            }
            let view = cx.new(|cx| PeekView::new(card, fit, &tab, popup, cx));
            opened = parent.map(|parent| (view.clone(), parent));
            view
        });
        let (handle, (view, parent)) = match (result, opened) {
            (Ok(handle), Some(opened)) => (handle, opened),
            (Ok(handle), None) => {
                tracing::error!("peek window has no native handle");
                handle.update(cx, |_, popup, _| popup.remove_window()).ok();
                return;
            }
            (Err(error), _) => {
                tracing::error!("failed to open peek window: {error:#}");
                return;
            }
        };

        let promoted = Rc::new(Cell::new(false));
        let flag = promoted.clone();
        let intercept: ShortcutFilter = Rc::new(move |chord: &KeyChord| {
            if flag.get() {
                is_browser_shortcut(chord)
            } else {
                is_plain_escape(chord)
            }
        });
        let id = tab.id;
        let request = ViewRequest {
            id,
            url: tab.url.clone(),
            parent,
            background: to_rgba8(Theme::of(cx).colors.surface),
            events: self.events.clone(),
            intercept,
            peek_modifier: Some(self.link_modifier()),
            data_directory: self.paths.webview_data(),
            download_folder: self.effective_downloads(),
            extensions_enabled: true,
            reputation_checks: self.settings.download_safety_checks,
            private: self.browser.private_shown(),
        };
        self.peek = Some(OpenPeek {
            window: handle.into(),
            view: view.clone(),
            tab,
            runtime: TabRuntime::default(),
            promoted,
            _events: cx.subscribe_in(&view, window, Self::on_peek_view_event),
        });

        cx.spawn_in(window, async move |shell, cx| {
            let Some(page) = request.create() else {
                return;
            };
            let attached = shell
                .update(cx, |shell, cx| shell.attach_peek_page(id, page.clone(), cx))
                .unwrap_or(false);
            if attached {
                page.focus();
            }
        })
        .detach();
    }

    fn attach_peek_page(
        &mut self,
        id: TabId,
        page: Rc<BrowserView>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(peek) = self.peek.as_ref().filter(|peek| peek.tab.id == id) else {
            return false;
        };
        peek.view.update(cx, |view, cx| {
            view.page = Some(page);
            if view.fit.is_some() {
                view.keep_fitting(cx);
            }
            cx.notify();
        });
        true
    }

    /// Where the page shows inside the window, or the whole window before
    /// the first layout.
    fn content_area(&self, window: &Window) -> Bounds<Pixels> {
        let content = self.content_bounds.get();
        if content.size.width > Pixels::ZERO {
            content
        } else {
            Bounds::new(point(Pixels::ZERO, Pixels::ZERO), window.bounds().size)
        }
    }

    pub(super) fn extension_popup_open(&self) -> bool {
        self.peek
            .as_ref()
            .is_some_and(|peek| peek.tab.url.starts_with("chrome-extension:"))
    }

    /// The page area minus the sidebar, which in compact mode floats over
    /// the page: popups open beside it, as the downloads panel does.
    fn beside_sidebar(&self, window: &Window, cx: &Context<Self>) -> Bounds<Pixels> {
        let area = self.content_area(window);
        let sidebar_left = if self.compact {
            Theme::of(cx).metrics.frame_inset
        } else {
            Pixels::ZERO
        };
        let left = area.origin.x.max(sidebar_left + self.sidebar_width);
        Bounds::new(
            point(left, area.origin.y),
            size(area.size.width - (left - area.origin.x), area.size.height),
        )
    }

    fn peek_bounds(&self, window: &Window, cx: &Context<Self>) -> (Bounds<Pixels>, Bounds<Pixels>) {
        let popup = window.bounds();
        let content = self.content_area(window);
        let margin = Theme::of(cx)
            .metrics
            .peek_margin
            .min(content.size.width * MAX_MARGIN_SHARE)
            .min(content.size.height * MAX_MARGIN_SHARE);
        let card = Bounds::new(
            point(content.origin.x + margin, content.origin.y + margin),
            size(
                content.size.width - margin * 2.,
                content.size.height - margin * 2.,
            ),
        );
        (popup, card)
    }

    fn on_peek_view_event(
        &mut self,
        _: &Entity<PeekView>,
        event: &PeekEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            PeekEvent::Close => self.close_peek(window, cx),
            PeekEvent::Promote => self.promote_peek(window, cx),
        }
    }

    pub(super) fn on_peek_page_event(&mut self, event: WebViewEvent, cx: &mut Context<Self>) {
        let Some(peek) = self.peek.as_mut() else {
            return;
        };
        match event {
            WebViewEvent::UrlChanged(url) => {
                log_failure(self.history.record_visit(&url, now()));
                peek.tab.url = url;
            }
            WebViewEvent::TitleChanged(title) => {
                log_failure(self.history.set_title(&peek.tab.url, &title));
                peek.tab.title = title;
            }
            WebViewEvent::FaviconChanged(png) => {
                peek.runtime.set_favicon(png.as_deref());
                peek.tab.favicon = png.map(Arc::from);
            }
            WebViewEvent::LoadStarted => peek.runtime.loading = true,
            WebViewEvent::LoadFinished => peek.runtime.loading = false,
            WebViewEvent::HistoryChanged {
                can_go_back,
                can_go_forward,
            } => {
                peek.runtime.can_go_back = can_go_back;
                peek.runtime.can_go_forward = can_go_forward;
            }
            WebViewEvent::CloseRequested => {
                self.in_main_window(cx, |shell, window, cx| shell.close_peek(window, cx));
                return;
            }
            WebViewEvent::NewWindowRequested {
                url,
                request: Some(request),
                ..
            } => {
                self.in_main_window(cx, move |shell, window, cx| {
                    shell.open_extension_window(url, Some(request), OpenPosition::End, window, cx);
                });
                return;
            }
            WebViewEvent::NewWindowRequested { url, .. }
                if peek.tab.url.starts_with("chrome-extension:") =>
            {
                self.in_main_window(cx, move |shell, window, cx| {
                    shell.open_tab(url, OpenPosition::End, window, cx);
                });
                return;
            }
            WebViewEvent::PeekRequested(url)
            | WebViewEvent::NewWindowRequested {
                url,
                user_initiated: true,
                ..
            } => {
                if let Some(page) = peek.view.read(cx).page.clone() {
                    page.navigate(&url);
                }
            }
            WebViewEvent::PermissionRequested { id, .. } => {
                webview_host::answer_permission(id, false);
                return;
            }
            WebViewEvent::NewWindowRequested { .. }
            | WebViewEvent::ExternalSchemeRequested { .. }
            | WebViewEvent::SecurityChanged(_)
            | WebViewEvent::ZoomChanged(_)
            | WebViewEvent::AudioChanged { .. } => return,
            WebViewEvent::FocusGained | WebViewEvent::Shortcut(_) => return,
            event @ (WebViewEvent::DownloadStarted { .. }
            | WebViewEvent::DownloadProgress { .. }
            | WebViewEvent::DownloadFinished { .. }) => {
                self.on_download_event(event, cx);
                return;
            }
        }
        peek.view
            .update(cx, |view, cx| view.show_tab(&peek.tab, cx));
    }

    pub(super) fn close_peek(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(peek) = self.peek.take() {
            self.dismiss_peek(peek, window, cx);
        }
    }

    fn dismiss_peek(&mut self, peek: OpenPeek, window: &mut Window, cx: &mut Context<Self>) {
        drop(peek.view.update(cx, |view, _| view.page.take()));
        self.dismiss_popup(peek.window, window, cx, |shell, _, _| {
            shell.focus_active_page()
        });
    }

    fn promote_peek(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(peek) = self.peek.take() else {
            return;
        };
        let page = peek.view.update(cx, |view, _| view.page.take());
        let main = window.window_handle().ok().map(|handle| handle.as_raw());
        let mut runtime = peek.runtime;
        runtime.view = match (page, main) {
            (Some(page), Some(main)) => {
                page.reparent(main);
                peek.promoted.set(true);
                Some(page)
            }
            _ => None,
        };
        let position = self
            .browser
            .active_tab_id()
            .map_or(OpenPosition::End, OpenPosition::After);
        let id = self.browser.adopt(peek.tab, position, now());
        self.runtime.insert(id, runtime);
        self.update_runtime(id, |runtime| runtime.focus_when_ready = true);
        self.show_active_tab(window, cx);
        self.dismiss_popup(peek.window, window, cx, |shell, _, _| {
            shell.focus_active_page()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_bare_escape_closes_the_peek() {
        let chord = |shift, key: &str| KeyChord {
            ctrl: false,
            shift,
            alt: false,
            key: key.to_string(),
        };
        assert!(is_plain_escape(&chord(false, "escape")));
        assert!(!is_plain_escape(&chord(true, "escape")));
        assert!(!is_plain_escape(&chord(false, "t")));
    }
}
