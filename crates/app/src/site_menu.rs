use gpui::{
    AnyElement, App, Context, ElementId, EventEmitter, FocusHandle, Hsla, InteractiveElement,
    IntoElement, ParentElement, Pixels, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Window, div, px,
};

use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};

use crate::icons;
use crate::theme::Theme;

const BORDER: f32 = 1.;
const SEPARATORS: f32 = 2.;

#[derive(Clone, Copy, PartialEq)]
pub enum SiteSecurity {
    Secure,
    NotSecure,
    Unknown,
}

pub struct SitePermission {
    pub key: String,
    pub allowed: bool,
}

pub struct SiteInfo {
    pub host: SharedString,
    pub security: SiteSecurity,
    pub zoom_percent: u32,
    pub permissions: Vec<SitePermission>,
}

pub enum SiteMenuEvent {
    ZoomIn,
    ZoomOut,
    ZoomReset,
    CopyLink,
    ClearData,
    ResetPermission(String),
    Dismiss,
}

pub struct SiteMenu {
    info: SiteInfo,
    confirming_clear: bool,
    focus_handle: FocusHandle,
    _activation: Subscription,
}

impl EventEmitter<SiteMenuEvent> for SiteMenu {}

/// Height of the menu for `info`, so its popup window fits it exactly.
pub fn site_menu_height(info: &SiteInfo, cx: &App) -> Pixels {
    let metrics = &Theme::of(cx).metrics;
    let rows = 5 + info.permissions.len() + usize::from(!info.permissions.is_empty());
    metrics.row_height * rows as f32
        + (metrics.radius_md + px(BORDER)) * SEPARATORS
        + metrics.radius_sm * 2.
        + px(BORDER * 2.)
}

impl SiteMenu {
    pub fn new(info: SiteInfo, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        let activation = cx.observe_window_activation(window, |_, window, cx| {
            if !window.is_window_active() {
                cx.emit(SiteMenuEvent::Dismiss);
            }
        });
        Self {
            info,
            confirming_clear: false,
            focus_handle,
            _activation: activation,
        }
    }

    pub fn set_zoom(&mut self, percent: u32, cx: &mut Context<Self>) {
        self.info.zoom_percent = percent;
        cx.notify();
    }

    fn row(&self, cx: &Context<Self>) -> gpui::Div {
        let metrics = &Theme::of(cx).metrics;
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(metrics.radius_md)
            .h(metrics.row_height)
            .px(metrics.radius_md * 1.5)
    }

    fn separator(&self, cx: &Context<Self>) -> AnyElement {
        let theme = Theme::of(cx);
        div()
            .h(px(BORDER))
            .my(theme.metrics.radius_md / 2.)
            .mx(theme.metrics.radius_md)
            .bg(theme.colors.border)
            .into_any_element()
    }

    fn header(&self, cx: &Context<Self>) -> Vec<AnyElement> {
        let theme = Theme::of(cx);
        let (glyph, color, text) = match self.info.security {
            SiteSecurity::Secure => (icons::LOCK, theme.colors.success, "Connection is secure"),
            SiteSecurity::NotSecure => (
                icons::WARNING,
                theme.colors.warning,
                "Connection is not secure",
            ),
            SiteSecurity::Unknown => (
                icons::LOCK,
                theme.colors.text_muted,
                "Connection not checked yet",
            ),
        };
        vec![
            self.row(cx)
                .font_weight(gpui::FontWeight::BOLD)
                .child(div().min_w_0().truncate().child(self.info.host.clone()))
                .into_any_element(),
            self.row(cx)
                .text_size(theme.metrics.text_sm)
                .text_color(theme.colors.text_muted)
                .child(icon(glyph, color, cx))
                .child(text)
                .into_any_element(),
        ]
    }

    fn zoom_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx);
        let percent = format!("{}%", self.info.zoom_percent);
        self.row(cx)
            .child(div().flex_1().child("Zoom"))
            .child(
                small_button("zoom-out")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SiteMenuEvent::ZoomOut)))
                    .child(icons::MINUS),
            )
            .child(
                small_button("zoom-reset")
                    .min_w(theme.metrics.nav_button_size * 1.6)
                    .label(percent)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SiteMenuEvent::ZoomReset))),
            )
            .child(
                small_button("zoom-in")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SiteMenuEvent::ZoomIn)))
                    .child(icons::ADD),
            )
            .into_any_element()
    }

    fn item(
        &self,
        id: &'static str,
        label: &'static str,
        color: Hsla,
        event: fn() -> SiteMenuEvent,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let hover_bg = Theme::of(cx).colors.surface_active;
        self.row(cx)
            .id(id)
            .rounded(Theme::of(cx).metrics.radius_md)
            .text_color(color)
            .hover(move |style| style.bg(hover_bg))
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(event())))
            .child(label)
            .into_any_element()
    }

    fn permission_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if self.info.permissions.is_empty() {
            return Vec::new();
        }
        let theme = Theme::of(cx);
        let mut rows = vec![
            self.row(cx)
                .text_size(theme.metrics.text_sm)
                .text_color(theme.colors.text_muted)
                .child("Permissions")
                .into_any_element(),
        ];
        for permission in &self.info.permissions {
            let key = permission.key.clone();
            let state = if permission.allowed {
                "Allowed"
            } else {
                "Blocked"
            };
            rows.push(
                self.row(cx)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(permission_label(&key)),
                    )
                    .child(div().text_color(theme.colors.text_muted).child(state))
                    .child(
                        small_button(ElementId::Name(format!("reset-{key}").into()))
                            .label("Reset")
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(SiteMenuEvent::ResetPermission(key.clone()))
                            })),
                    )
                    .into_any_element(),
            );
        }
        rows
    }

    fn clear_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx);
        if !self.confirming_clear {
            let hover_bg = theme.colors.surface_active;
            return self
                .row(cx)
                .id("clear-site-data")
                .rounded(theme.metrics.radius_md)
                .text_color(theme.colors.danger)
                .hover(move |style| style.bg(hover_bg))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.confirming_clear = true;
                    cx.notify();
                }))
                .child("Clear cookies and site data")
                .into_any_element();
        }
        self.row(cx)
            .text_size(theme.metrics.text_sm)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(theme.colors.warning)
                    .child("Delete site data?"),
            )
            .child(
                small_button("clear-confirm")
                    .danger()
                    .label("Clear")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SiteMenuEvent::ClearData))),
            )
            .child(
                small_button("clear-cancel")
                    .label("Cancel")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.confirming_clear = false;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
}

impl Render for SiteMenu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text = Theme::of(cx).colors.text;
        let copy_link = self.item(
            "copy-link",
            "Copy link",
            text,
            || SiteMenuEvent::CopyLink,
            cx,
        );
        let header = self.header(cx);
        let zoom = self.zoom_row(cx);
        let permissions = self.permission_rows(cx);
        let clear = self.clear_row(cx);
        let separator = self.separator(cx);
        let second_separator = self.separator(cx);
        let theme = Theme::of(cx);
        let metrics = &theme.metrics;
        div()
            .id("site-menu")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .p(metrics.radius_sm)
            .rounded(metrics.radius_popover)
            .bg(theme.colors.surface_raised)
            .border_1()
            .border_color(theme.colors.border)
            .font_family(theme.fonts.ui.clone())
            .font_weight(theme.fonts.ui_weight)
            .text_size(metrics.text_md)
            .text_color(theme.colors.text)
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(SiteMenuEvent::Dismiss);
                }
            }))
            .children(header)
            .child(separator)
            .child(zoom)
            .child(copy_link)
            .children(permissions)
            .child(second_separator)
            .child(clear)
    }
}

fn icon(glyph: icons::Icon, color: Hsla, cx: &App) -> impl IntoElement {
    let theme = Theme::of(cx);
    div()
        .flex_shrink_0()
        .font_family(theme.fonts.icons.clone())
        .text_size(theme.metrics.icon_glyph)
        .text_color(color)
        .child(glyph)
}

fn small_button(id: impl Into<ElementId>) -> Button {
    Button::new(id).small().secondary()
}

fn permission_label(key: &str) -> String {
    let spaced = key.replace(['-', '_'], " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => spaced,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_keys_read_as_words() {
        assert_eq!(permission_label("geolocation"), "Geolocation");
        assert_eq!(permission_label("clipboard-read"), "Clipboard read");
    }
}
