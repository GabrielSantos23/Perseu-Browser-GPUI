use std::time::{Duration, Instant};

use gpui::{
    AnyElement, AnyWindowHandle, AppContext, Bounds, Context, Entity, EventEmitter, FocusHandle,
    InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render, Styled, Subscription,
    WeakEntity, Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div,
    point, size,
};
use raw_window_handle::HasWindowHandle;

use super::{ESCAPE_KEY, Shell};
use crate::native;
use crate::theme::Theme;

/// A click on the footer button right after the panel closed itself (by
/// losing focus to that click) must not reopen it.
const REOPEN_GUARD: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Panel {
    Downloads,
    Archive,
}

pub(super) struct OpenPanel {
    pub(super) kind: Panel,
    window: AnyWindowHandle,
    /// Where the popup sits, in main-window coordinates.
    pub(super) bounds: Bounds<gpui::Pixels>,
    _events: Subscription,
}

pub(super) enum PanelEvent {
    Dismiss,
}

/// The popup window showing the downloads or archive panel beside the
/// sidebar. It draws the shell's own panel and redraws when the shell changes.
pub(super) struct PanelHost {
    shell: WeakEntity<Shell>,
    kind: Panel,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PanelEvent> for PanelHost {}

impl PanelHost {
    fn new(shell: Entity<Shell>, kind: Panel, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        let subscriptions = vec![
            cx.observe(&shell, |_, _, cx| cx.notify()),
            cx.observe_window_activation(window, |_, window, cx| {
                if !window.is_window_active() {
                    cx.emit(PanelEvent::Dismiss);
                }
            }),
        ];
        Self {
            shell: shell.downgrade(),
            kind,
            focus_handle,
            _subscriptions: subscriptions,
        }
    }
}

impl Render for PanelHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let kind = self.kind;
        let body: AnyElement = self
            .shell
            .update(cx, |shell, cx| match kind {
                Panel::Downloads => shell.render_downloads_panel(cx),
                Panel::Archive => shell.render_archive_panel(cx).into_any_element(),
            })
            .unwrap_or_else(|_| div().into_any_element());
        let theme = Theme::of(cx);
        div()
            .id("panel-popup")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .p(theme.metrics.radius_md)
            .rounded(theme.metrics.radius_popover)
            .bg(theme.colors.surface_raised)
            .border_1()
            .border_color(theme.colors.border)
            .font_family(theme.fonts.ui.clone())
            .font_weight(theme.fonts.ui_weight)
            .text_size(theme.metrics.text_md)
            .text_color(theme.colors.text)
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == ESCAPE_KEY {
                    cx.emit(PanelEvent::Dismiss);
                }
            }))
            .child(body)
    }
}

impl Shell {
    fn panel_shown(&self, kind: Panel) -> bool {
        match kind {
            Panel::Downloads => self.downloads_open,
            Panel::Archive => self.archive_open,
        }
    }

    pub(super) fn set_panel_shown(&mut self, kind: Panel, shown: bool) {
        match kind {
            Panel::Downloads => self.downloads_open = shown,
            Panel::Archive => self.archive_open = shown,
        }
    }

    /// Opens the panel beside the sidebar, or closes it when it is open.
    pub(super) fn toggle_panel(
        &mut self,
        kind: Panel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let just_closed = self
            .panel_closed
            .take()
            .is_some_and(|(closed, at)| closed == kind && at.elapsed() < REOPEN_GUARD);
        let was_open = self.panel.as_ref().map(|open| open.kind);
        self.archive_open = false;
        self.downloads_open = false;
        self.sync_panel(window, cx);
        if was_open == Some(kind) || just_closed {
            cx.notify();
            return;
        }
        self.open_panel(kind, window, cx);
    }

    fn open_panel(&mut self, kind: Panel, window: &mut Window, cx: &mut Context<Self>) {
        let metrics = &Theme::of(cx).metrics;
        let window_bounds = window.bounds();
        let inset = metrics.frame_inset;
        let sidebar_left = if self.compact {
            inset
        } else {
            gpui::Pixels::ZERO
        };
        let width = self.sidebar_width;
        let height = (window_bounds.size.height / 2.)
            .min(window_bounds.size.height - inset * 2.)
            .max(metrics.row_height * 6.);
        let max_x = (window_bounds.size.width - width - inset).max(inset);
        let x = (sidebar_left + self.sidebar_width + metrics.radius_md).clamp(inset, max_x);
        let y = (window_bounds.size.height - inset - height).max(inset);
        let bounds = Bounds::new(point(x, y), size(width, height));
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                window_bounds.origin + bounds.origin,
                bounds.size,
            ))),
            titlebar: None,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Transparent,
            ..WindowOptions::default()
        };
        self.set_panel_shown(kind, true);
        cx.notify();
        let shell = cx.entity();
        // The popup draws the shell's panel as soon as it opens, so it must
        // open after this update of the shell has ended.
        cx.spawn(async move |this, cx| {
            let mut host: Option<Entity<PanelHost>> = None;
            let opened = cx.update(|cx| {
                cx.open_window(options, |popup, cx| {
                    if let Ok(handle) = popup.window_handle() {
                        native::remove_frame_shadow_and_border(handle.as_raw());
                    }
                    let view = cx.new(|cx| PanelHost::new(shell, kind, popup, cx));
                    host = Some(view.clone());
                    view
                })
            });
            let (handle, host) = match (opened, host) {
                (Ok(handle), Some(host)) => (handle, host),
                (Err(error), _) => {
                    tracing::error!("failed to open panel: {error:#}");
                    return;
                }
                (Ok(_), None) => return,
            };
            let handle: AnyWindowHandle = handle.into();
            let stored = this.update(cx, |shell, cx| {
                let events = cx.subscribe(&host, move |shell, _, _: &PanelEvent, cx| {
                    shell.set_panel_shown(kind, false);
                    cx.notify();
                });
                shell.panel = Some(OpenPanel {
                    kind,
                    window: handle,
                    bounds,
                    _events: events,
                });
                cx.notify();
            });
            if stored.is_err() {
                handle.update(cx, |_, popup, _| popup.remove_window()).ok();
            }
        })
        .detach();
    }

    /// Closes the popup once its panel is no longer shown, whoever cleared
    /// the flag (the panel's back button, a restore, Escape, losing focus).
    pub(super) fn sync_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(kind) = self.panel.as_ref().map(|open| open.kind) else {
            return;
        };
        if self.panel_shown(kind) {
            return;
        }
        if let Some(open) = self.panel.take() {
            self.panel_closed = Some((kind, Instant::now()));
            self.dismiss_popup(open.window, window, cx, |_, _, _| {});
        }
    }

    /// Runs `then` with the main window, for clicks in the panel popup that
    /// must act on the main window (showing a restored tab).
    pub(super) fn in_main_window(
        &self,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let main = self.main_window;
        cx.spawn(async move |shell, cx| {
            main.update(cx, |_, window, cx| {
                shell.update(cx, |shell, cx| then(shell, window, cx)).ok();
            })
            .ok();
        })
        .detach();
    }
}
