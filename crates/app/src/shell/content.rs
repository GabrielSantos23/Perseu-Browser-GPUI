use std::rc::Rc;

use gpui::{
    AnyElement, Bounds, Context, Div, Hsla, IntoElement, ParentElement, Pixels, Styled, Window,
    canvas, div, prelude::FluentBuilder,
};
use webview_host::{BrowserView, ContentBounds};

use super::Shell;
use crate::theme::Theme;

impl Shell {
    pub(super) fn render_content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let panes: AnyElement = match self.active_split() {
            _ if self.settings_page.is_some() => self.render_settings(cx),
            _ if self.history_page.is_some() => self.render_history(cx),
            Some(split) => self.render_split(split, cx),
            None => {
                let border = self.palette(cx).frame_border;
                self.render_pane(self.active_view(), self.overlay_cutout(), border, cx)
                    .size_full()
                    .into_any_element()
            }
        };
        let theme = Theme::of(cx);
        let content_bounds = self.content_bounds.clone();

        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .pr(theme.metrics.frame_inset)
            .when(self.compact, |content| {
                content.pl(theme.metrics.frame_inset)
            })
            .pb(theme.metrics.frame_inset)
            .child(
                div()
                    .relative()
                    .size_full()
                    .child(
                        canvas(
                            move |bounds, _, _| content_bounds.set(bounds),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(panes),
            )
    }

    pub(super) fn render_pane(
        &self,
        view: Option<Rc<BrowserView>>,
        cutout: f32,
        border: Hsla,
        cx: &Context<Self>,
    ) -> Div {
        let theme = Theme::of(cx);
        let corner_radius = f32::from(theme.metrics.radius_frame) - 1.0;
        let overlay_bottom = self.title_overlay_bottom(cx);
        div()
            .min_w_0()
            .min_h_0()
            .rounded(theme.metrics.radius_frame)
            .bg(theme.colors.surface)
            .border_1()
            .border_color(border)
            .child(
                canvas(
                    move |bounds, window, _| {
                        let cutout_top = (overlay_bottom - f32::from(bounds.origin.y)).max(0.0);
                        sync_webview_bounds(view, bounds, corner_radius, cutout, cutout_top, window)
                    },
                    |_, _, _, _| {},
                )
                .size_full(),
            )
    }
}

pub(super) fn sync_webview_bounds(
    view: Option<Rc<BrowserView>>,
    bounds: Bounds<Pixels>,
    corner_radius: f32,
    cutout_width: f32,
    cutout_top: f32,
    window: &mut Window,
) {
    let Some(view) = view else {
        return;
    };
    view.set_bounds(ContentBounds {
        x: bounds.origin.x.into(),
        y: bounds.origin.y.into(),
        width: bounds.size.width.into(),
        height: bounds.size.height.into(),
        corner_radius,
        scale_factor: window.scale_factor(),
        cutout_width,
        cutout_top,
    });
}
