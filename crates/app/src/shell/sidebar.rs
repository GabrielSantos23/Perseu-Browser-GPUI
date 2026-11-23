use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, Context, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Styled, div, prelude::FluentBuilder, relative,
};

use super::Shell;
use crate::theme::Theme;

const LOADING_SWEEP: Duration = Duration::from_millis(1100);
const LOADING_SEGMENT: f32 = 0.3;

impl Shell {
    pub(super) fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);

        div()
            .relative()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w(self.sidebar_width)
            .h_full()
            .px(theme.metrics.frame_inset)
            .gap(theme.metrics.radius_sm)
            .when(self.settings.site_title_in_sidebar, |sidebar| {
                sidebar.child(self.render_sidebar_nav(cx))
            })
            .children(self.render_sidebar_site_title(cx))
            .children(self.render_site_notices(cx))
            .children((!self.loading_in_title_bar()).then(|| self.render_loading_bar(cx)))
            .children(
                (!self.browser.private_shown())
                    .then(|| self.render_essentials(cx))
                    .flatten(),
            )
            .child(self.render_space_body(cx))
            .children(self.render_media_player(cx))
            .children(self.render_private_bar(cx))
            .child(self.render_bottom_bar(cx))
            .children(self.render_store_offer(cx))
            .child(self.render_resize_handle(cx))
    }

    fn render_space_body(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.extensions_open {
            return self.render_extensions_panel(cx).into_any_element();
        }
        let content = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(self.render_tab_list(cx));
        self.animate_space_content(content, cx)
    }

    /// Whether the page loading bar sits under the title bar instead of at
    /// the top of the sidebar: whenever the title bar is on screen.
    fn loading_in_title_bar(&self) -> bool {
        !self.settings.hide_title_bar || self.title_reveal.shown()
    }

    pub(super) fn render_title_loading_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.loading_in_title_bar() {
            return None;
        }
        let metrics = &Theme::of(cx).metrics;
        let bottom = if self.settings.hide_title_bar {
            gpui::px(self.title_overlay_bottom(cx))
        } else {
            metrics.title_bar_height
        };
        let top = bottom - metrics.loading_bar_height;
        Some(
            div()
                .absolute()
                .top(top)
                .left(self.title_overlay_left())
                .right_0()
                .child(self.render_loading_bar(cx))
                .into_any_element(),
        )
    }

    fn render_loading_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let accent = self.palette(cx).accent;
        let sweep = crate::motion::curve(crate::motion::Easing::EaseInOut);
        let reduced = crate::motion::reduced();

        div()
            .relative()
            .h(theme.metrics.loading_bar_height)
            .mx(theme.metrics.radius_md)
            .overflow_hidden()
            .rounded(theme.metrics.loading_bar_height)
            .when(
                self.active_runtime().is_some_and(|runtime| runtime.loading),
                |track| {
                    if reduced {
                        return track.child(div().size_full().bg(accent.opacity(0.6)));
                    }
                    track.child(
                        div()
                            .absolute()
                            .top_0()
                            .h_full()
                            .w(relative(LOADING_SEGMENT))
                            .bg(accent)
                            .with_animation(
                                "loading-sweep",
                                Animation::new(LOADING_SWEEP).repeat(),
                                move |segment, delta| {
                                    segment.left(relative(sweep_offset(sweep(delta))))
                                },
                            ),
                    )
                },
            )
    }

    fn render_resize_handle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let handle_width = Theme::of(cx).metrics.resize_handle_width;

        div()
            .id("sidebar-resize-handle")
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(handle_width)
            .cursor_col_resize()
            .on_mouse_down(MouseButton::Left, cx.listener(Self::start_sidebar_resize))
    }
}

fn sweep_offset(delta: f32) -> f32 {
    delta * (1.0 + LOADING_SEGMENT) - LOADING_SEGMENT
}
