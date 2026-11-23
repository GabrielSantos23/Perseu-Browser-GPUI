use gpui::{
    AnyElement, App, Hsla, InteractiveElement, IntoElement, ParentElement, RenderOnce, Styled,
    Window, WindowControlArea, div,
};

use crate::icons;
use crate::theme::Theme;

#[derive(IntoElement)]
pub struct TitleBar {
    leading: AnyElement,
    center: Option<AnyElement>,
}

impl TitleBar {
    pub fn new(leading: impl IntoElement) -> Self {
        Self {
            leading: leading.into_any_element(),
            center: None,
        }
    }

    pub fn center(mut self, center: impl IntoElement) -> Self {
        self.center = Some(center.into_any_element());
        self
    }
}

impl RenderOnce for TitleBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let maximize = if window.is_maximized() {
            ControlButton::restore()
        } else {
            ControlButton::maximize()
        };

        div()
            .relative()
            .flex()
            .flex_row()
            .flex_shrink_0()
            .items_center()
            .h(theme.metrics.title_bar_height)
            .w_full()
            .child(self.leading)
            .child(drag_region())
            .child(ControlButton::minimize())
            .child(maximize)
            .child(ControlButton::close())
            .children(self.center.map(|center| {
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(theme.metrics.title_center_inset)
                    .right(theme.metrics.title_center_inset)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(center)
            }))
    }
}

fn drag_region() -> impl IntoElement {
    div()
        .id("title-bar-drag")
        .flex_1()
        .h_full()
        .window_control_area(WindowControlArea::Drag)
}

#[derive(IntoElement)]
struct ControlButton {
    id: &'static str,
    glyph: crate::icons::Icon,
    area: WindowControlArea,
    destructive: bool,
}

impl ControlButton {
    fn minimize() -> Self {
        Self::plain("minimize", icons::MINIMIZE, WindowControlArea::Min)
    }

    fn maximize() -> Self {
        Self::plain("maximize", icons::MAXIMIZE, WindowControlArea::Max)
    }

    fn restore() -> Self {
        Self::plain("restore", icons::RESTORE, WindowControlArea::Max)
    }

    fn close() -> Self {
        Self {
            id: "close",
            glyph: icons::WINDOW_CLOSE,
            area: WindowControlArea::Close,
            destructive: true,
        }
    }

    fn plain(id: &'static str, glyph: crate::icons::Icon, area: WindowControlArea) -> Self {
        Self {
            id,
            glyph,
            area,
            destructive: false,
        }
    }

    fn hover_colors(&self, theme: &Theme) -> (Hsla, Hsla) {
        if self.destructive {
            (theme.colors.danger, theme.colors.text)
        } else {
            (theme.colors.surface_hover, theme.colors.text)
        }
    }
}

impl RenderOnce for ControlButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let (hover_bg, hover_fg) = self.hover_colors(theme);

        div()
            .id(self.id)
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .w(theme.metrics.window_control_width)
            .h_full()
            .text_size(theme.metrics.icon_glyph)
            .text_color(theme.colors.text_muted)
            .hover(move |style| style.bg(hover_bg).text_color(hover_fg))
            .window_control_area(self.area)
            .child(self.glyph)
    }
}
