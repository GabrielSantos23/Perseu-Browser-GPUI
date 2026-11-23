use gpui::{Context, IntoElement, ParentElement, Render, SharedString, Styled, Window, div};

use crate::icons;
use crate::theme::Theme;

pub struct Toast {
    message: SharedString,
}

impl Toast {
    pub fn new(message: impl Into<SharedString>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Render for Toast {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let metrics = &theme.metrics;
        div()
            .size_full()
            .flex()
            .flex_row()
            .items_center()
            .gap(metrics.radius_md)
            .px(metrics.radius_md * 1.5)
            .rounded(metrics.radius_popover)
            .bg(theme.colors.surface_raised)
            .border_1()
            .border_color(theme.colors.border)
            .font_family(theme.fonts.ui.clone())
            .font_weight(theme.fonts.ui_weight)
            .text_size(metrics.text_md)
            .text_color(theme.colors.text)
            .child(
                div()
                    .flex_shrink_0()
                    .font_family(theme.fonts.icons.clone())
                    .text_size(metrics.nav_glyph)
                    .text_color(theme.colors.success)
                    .child(icons::CHECK),
            )
            .child(div().min_w_0().truncate().child(self.message.clone()))
    }
}
