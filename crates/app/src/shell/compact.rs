use std::time::{Duration, Instant};

use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Pixels, Styled, Window,
    div, point, px,
};

use super::Shell;
use crate::native;
use crate::theme::Theme;

const POLL_INTERVAL: Duration = Duration::from_millis(16);
const SHOW_DELAY: Duration = Duration::from_millis(30);
const HIDE_DELAY: Duration = Duration::from_millis(400);
pub(super) const SLIDE: Duration = Duration::from_millis(120);

#[derive(Debug, Default)]
pub(super) struct HoverTimers {
    entered_hot_zone: Option<Instant>,
    left_area: Option<Instant>,
}

impl HoverTimers {
    /// Whether the pointer has been in the hot zone for `SHOW_DELAY`.
    fn wants_show(&mut self, in_hot_zone: bool, now: Instant) -> bool {
        if !in_hot_zone {
            self.entered_hot_zone = None;
            return false;
        }
        now.duration_since(*self.entered_hot_zone.get_or_insert(now)) >= SHOW_DELAY
    }

    /// Whether the pointer has been away from the area for `HIDE_DELAY`.
    fn wants_hide(&mut self, keep: bool, now: Instant) -> bool {
        if keep {
            self.left_area = None;
            return false;
        }
        now.duration_since(*self.left_area.get_or_insert(now)) >= HIDE_DELAY
    }
}

impl Shell {
    pub(super) fn toggle_compact(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.compact = !self.compact;
        self.reveal.jump(false);
        self.compact_hover = HoverTimers::default();
        self.sync_hover_poll(window, cx);
        cx.notify();
    }

    /// Starts or stops watching the pointer, which compact mode and the
    /// hidden title bar need to know when to slide in.
    pub(super) fn sync_hover_poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let needed = self.compact || self.settings.hide_title_bar;
        if !needed {
            self.compact_poll = None;
            self.title_reveal.jump(false);
            return;
        }
        if self.compact_poll.is_some() {
            return;
        }
        self.compact_poll = Some(cx.spawn_in(window, async move |shell, cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;
                let polled = shell.update_in(cx, |shell, window, cx| shell.poll_hover(window, cx));
                // A failed update can be a nested message pump borrowing the
                // window; only a dropped shell ends the poll.
                if polled.is_err() && shell.upgrade().is_none() {
                    break;
                }
            }
        }));
    }

    fn poll_hover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(cursor) = native::cursor_position() else {
            return;
        };
        let scale = window.scale_factor();
        let bounds = window.bounds();
        let local = point(
            px(cursor.0 / scale) - bounds.origin.x,
            px(cursor.1 / scale) - bounds.origin.y,
        );
        let metrics = &Theme::of(cx).metrics;
        let title_bar_height = metrics.title_bar_height;
        let frame_inset = metrics.frame_inset;
        let edge = metrics.frame_inset + metrics.radius_sm;
        let in_window_x = local.x >= self.title_overlay_left() && local.x <= bounds.size.width;
        let now = Instant::now();
        // The downloads or archive popup takes focus, but it is still ours.
        let active = window.is_window_active() || self.panel.is_some();

        if self.settings.hide_title_bar {
            if self.title_reveal.shown() {
                let over_bar = in_window_x
                    && local.y >= -frame_inset
                    && local.y <= self.title_overlay_height(cx) + frame_inset;
                let keep = over_bar || self.site_menu.is_some() || self.menu.is_some();
                if self.title_hover.wants_hide(keep, now) {
                    self.set_title_revealed(false, cx);
                }
            } else {
                let in_hot_zone = active
                    && in_window_x
                    && local.y >= -frame_inset
                    && local.y <= edge;
                if self.title_hover.wants_show(in_hot_zone, now) {
                    self.set_title_revealed(true, cx);
                }
            }
        }

        if !self.compact {
            return;
        }
        let body_top = if self.settings.hide_title_bar {
            Pixels::ZERO
        } else {
            title_bar_height
        };
        let in_body = local.y >= body_top && local.y <= bounds.size.height;
        if self.reveal.shown() {
            let over_sidebar =
                in_body && local.x >= Pixels::ZERO && local.x <= frame_inset + self.sidebar_width;
            let over_panel = self
                .panel
                .as_ref()
                .is_some_and(|open| open.bounds.contains(&local));
            let keep = over_sidebar
                || over_panel
                || self.download_landing()
                || cx.has_active_drag()
                || self.renaming_space
                || self.menu.is_some()
                || self.theme_picker.is_some()
                || self.extension_popup_open()
                || self.store_offer.is_some();
            if self.compact_hover.wants_hide(keep, now) {
                // The panel sits beside the sidebar and leaves with it.
                if let Some(kind) = self.panel.as_ref().map(|open| open.kind) {
                    self.set_panel_shown(kind, false);
                }
                self.set_revealed(false, cx);
            }
            return;
        }
        let in_hot_zone =
            in_body && active && local.x >= -frame_inset && local.x <= edge;
        if self.compact_hover.wants_show(in_hot_zone, now) {
            self.set_revealed(true, cx);
        }
    }

    /// Brings the auto-hiding sidebar out, for something that needs an
    /// answer there.
    pub(super) fn reveal_sidebar(&mut self, cx: &mut Context<Self>) {
        if self.compact && !self.reveal.shown() {
            self.set_revealed(true, cx);
        }
    }

    fn set_revealed(&mut self, showing: bool, cx: &mut Context<Self>) {
        self.compact_hover = HoverTimers::default();
        self.reveal.set(showing);
        cx.notify();
    }

    fn set_title_revealed(&mut self, showing: bool, cx: &mut Context<Self>) {
        self.title_hover = HoverTimers::default();
        self.title_reveal.set(showing);
        cx.notify();
    }

    pub(super) fn reveal_fraction(&self) -> f32 {
        self.reveal.fraction()
    }

    pub(super) fn title_reveal_fraction(&self) -> f32 {
        self.title_reveal.fraction()
    }

    /// Bottom edge, in window pixels, of the title bar while it slides over
    /// the page. 0 when the title bar is part of the layout or hidden.
    pub(super) fn title_overlay_bottom(&self, cx: &Context<Self>) -> f32 {
        if !self.settings.hide_title_bar {
            return 0.0;
        }
        f32::from(self.title_overlay_height(cx)) * self.title_reveal_fraction()
    }

    /// Left edge of the sliding title bar: beside the sidebar when it is
    /// docked, across the whole window in compact mode.
    pub(super) fn title_overlay_left(&self) -> Pixels {
        if self.compact {
            Pixels::ZERO
        } else {
            self.sidebar_width
        }
    }

    /// Full height of the sliding overlay: the title bar, plus the favorites
    /// bar when it is on.
    pub(super) fn title_overlay_height(&self, cx: &Context<Self>) -> Pixels {
        let metrics = &Theme::of(cx).metrics;
        let favorites = if self.settings.favorites_bar {
            metrics.favorites_bar_height
        } else {
            Pixels::ZERO
        };
        metrics.title_bar_height + favorites
    }

    pub(super) fn advance_reveal(&mut self, window: &mut Window) -> f32 {
        if self.reveal.advance() | self.title_reveal.advance() {
            window.request_animation_frame();
        }
        self.reveal_fraction()
    }

    pub(super) fn overlay_cutout(&self) -> f32 {
        let frame_border = 1.0;
        (f32::from(self.sidebar_width) * self.reveal_fraction() - frame_border).max(0.0)
    }

    pub(super) fn render_compact_sidebar(
        &self,
        fraction: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if fraction <= 0.0 {
            return None;
        }
        let sidebar = self.render_sidebar(cx).into_any_element();
        let theme = Theme::of(cx);
        let palette = self.palette(cx);
        let width = self.sidebar_width;
        Some(
            div()
                .id("compact-sidebar")
                .absolute()
                .top_0()
                .bottom(theme.metrics.frame_inset)
                .left(theme.metrics.frame_inset - width * (1.0 - fraction))
                .w(width)
                .rounded(theme.metrics.radius_frame)
                .bg(palette.sidebar)
                .border_1()
                .border_color(theme.colors.border)
                .shadow_lg()
                .overflow_hidden()
                .children(self.space_backdrop(cx))
                .child(sidebar)
                .into_any_element(),
        )
    }
}
