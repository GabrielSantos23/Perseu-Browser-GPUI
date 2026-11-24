use gpui::{
    Action, AnyView, AnyWindowHandle, App, AppContext, Context, InteractiveElement, IntoElement,
    ParentElement, Pixels, Render, SharedString, Styled, TextRun, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, font, point, px,
    size,
};
use raw_window_handle::HasWindowHandle;

use crate::native;
use crate::theme::Theme;

const CURSOR_GAP_X: f32 = 12.;
/// A click before the tooltip appeared silences the next one shown this
/// close to it: it belongs to what was clicked.
const CLICK_REACH: f32 = 24.;

/// What a click silenced: the label of the tooltip shown (or about to be
/// shown) for the clicked item, until the pointer reaches another item.
#[derive(Clone)]
enum Silenced {
    Label(SharedString),
    Next(gpui::Point<Pixels>),
}

thread_local! {
    static SHOWN: std::cell::RefCell<Option<SharedString>> = const { std::cell::RefCell::new(None) };
    static SILENCED: std::cell::RefCell<Option<Silenced>> = const { std::cell::RefCell::new(None) };
}

/// Call on every mouse press: the clicked item's tooltip stays hidden
/// while the pointer is still on it.
pub fn clicked(window: &Window) {
    let silenced = match SHOWN.with(|shown| shown.borrow().clone()) {
        Some(label) => Silenced::Label(label),
        None => Silenced::Next(window.mouse_position()),
    };
    SILENCED.with(|cell| *cell.borrow_mut() = Some(silenced));
}

/// Whether `label`'s tooltip should stay hidden after a click.
fn silenced(label: &SharedString, mouse: gpui::Point<Pixels>) -> bool {
    SILENCED.with(|cell| {
        let mut cell = cell.borrow_mut();
        let hide = match cell.clone() {
            Some(Silenced::Label(silenced)) => silenced == *label,
            Some(Silenced::Next(at)) => {
                let near = (mouse.x - at.x).abs() < px(CLICK_REACH)
                    && (mouse.y - at.y).abs() < px(CLICK_REACH);
                if near {
                    *cell = Some(Silenced::Label(label.clone()));
                }
                near
            }
            None => false,
        };
        if !hide {
            *cell = None;
        }
        hide
    })
}
const CURSOR_GAP_Y: f32 = 18.;

#[derive(Clone, Copy, PartialEq)]
pub enum Side {
    Right,
    Left,
}

/// Stand-in gpui keeps while the tooltip is shown. The visible tooltip is a
/// small popup window, so the page's webview cannot cover it; it closes when
/// gpui drops this stand-in.
struct TooltipAnchor {
    popup: Option<AnyWindowHandle>,
}

impl Render for TooltipAnchor {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

struct TooltipPopup {
    label: SharedString,
    keys: Option<SharedString>,
}

pub fn tooltip(label: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView {
    tooltip_on(Side::Right, label)
}

/// Opens toward the left of the pointer instead of the right.
pub fn tooltip_on(
    side: Side,
    label: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> AnyView {
    let label = label.into();
    move |window, cx| show(label.clone(), None, side, window, cx)
}

/// The keys bound to `action`, as Windows writes them ("Ctrl+Shift+T").
pub fn keys_for(action: &dyn Action, window: &Window) -> Option<String> {
    window
        .highest_precedence_binding_for_action(action)
        .map(|binding| {
            binding
                .keystrokes()
                .iter()
                .map(|key| key_label(key.modifiers(), key.key()))
                .collect::<Vec<_>>()
                .join(" ")
        })
}

pub fn tooltip_with_keys(
    side: Side,
    label: &'static str,
    action: impl Action,
) -> impl Fn(&mut Window, &mut App) -> AnyView {
    move |window, cx| {
        let keys = window
            .highest_precedence_binding_for_action(&action)
            .map(|binding| {
                binding
                    .keystrokes()
                    .iter()
                    .map(|key| key_label(key.modifiers(), key.key()))
                    .collect::<Vec<_>>()
                    .join(" ")
                    .into()
            });
        show(label.into(), keys, side, window, cx)
    }
}

fn text_width(text: &SharedString, window: &Window, cx: &App) -> Pixels {
    let theme = Theme::of(cx);
    let run = TextRun {
        len: text.len(),
        font: gpui::Font {
            weight: theme.fonts.ui_weight,
            ..font(theme.fonts.ui.clone())
        },
        color: theme.colors.text,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(text.clone(), theme.metrics.text_sm, &[run], None)
        .width
}

fn show(
    label: SharedString,
    keys: Option<SharedString>,
    side: Side,
    window: &mut Window,
    cx: &mut App,
) -> AnyView {
    let anchor = cx.new(|_| TooltipAnchor { popup: None });
    if silenced(&label, window.mouse_position()) {
        return anchor.into();
    }
    SHOWN.with(|shown| *shown.borrow_mut() = Some(label.clone()));
    let metrics = &Theme::of(cx).metrics;
    let mut width = text_width(&label, window, cx) + metrics.radius_md * 2. + px(4.);
    if let Some(keys) = &keys {
        width += text_width(keys, window, cx) + metrics.radius_md;
    }
    let width = width.min(metrics.menu_width * 1.5);
    let height = metrics.nav_button_size;
    let mouse = window.bounds().origin + window.mouse_position();
    let x = match side {
        Side::Right => mouse.x + px(CURSOR_GAP_X),
        Side::Left => mouse.x - width - px(4.),
    };
    let display = window.display(cx);
    let mut origin = point(x, mouse.y + px(CURSOR_GAP_Y));
    if let Some(screen) = display.as_ref().map(|display| display.bounds()) {
        if origin.y + height > screen.bottom() {
            origin.y = mouse.y - height - px(4.);
        }
        origin.x = origin.x.min(screen.right() - width).max(screen.left());
    }
    let display_id = display.map(|display| display.id());

    anchor.update(cx, |_, cx| {
        cx.observe_window_activation(window, |anchor: &mut TooltipAnchor, window, cx| {
            if !window.is_window_active()
                && let Some(handle) = anchor.popup.take()
            {
                close(handle, cx);
            }
        })
        .detach();
    });
    let weak = anchor.downgrade();
    cx.defer(move |cx| {
        if weak.upgrade().is_none() {
            return;
        }
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(gpui::Bounds::new(
                origin,
                size(width, height),
            ))),
            titlebar: None,
            display_id,
            focus: false,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Transparent,
            ..WindowOptions::default()
        };
        let opened = cx.open_window(options, |popup, cx| {
            if let Ok(handle) = popup.window_handle() {
                native::remove_frame_shadow_and_border(handle.as_raw());
            }
            cx.new(|_| TooltipPopup { label, keys })
        });
        match opened {
            Ok(handle) => {
                let handle: AnyWindowHandle = handle.into();
                let kept = weak.update(cx, |anchor, _| anchor.popup = Some(handle));
                if kept.is_err() {
                    close(handle, cx);
                }
            }
            Err(error) => tracing::warn!("failed to show tooltip: {error:#}"),
        }
    });
    cx.observe_release(&anchor, |anchor, cx| {
        SHOWN.with(|shown| *shown.borrow_mut() = None);
        if let Some(handle) = anchor.popup.take() {
            close(handle, cx);
        }
    })
    .detach();
    anchor.into()
}

fn close(handle: AnyWindowHandle, cx: &mut App) {
    cx.defer(move |cx| {
        handle.update(cx, |_, popup, _| popup.remove_window()).ok();
    });
}

fn key_label(modifiers: &gpui::Modifiers, key: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (held, name) in [
        (modifiers.control, "Ctrl"),
        (modifiers.alt, "Alt"),
        (modifiers.shift, "Shift"),
        (modifiers.platform, "Win"),
    ] {
        if held {
            parts.push(name.to_string());
        }
    }
    let mut chars = key.chars();
    parts.push(match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    });
    parts.join("+")
}

impl Render for TooltipPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let metrics = &theme.metrics;
        div()
            .size_full()
            .on_mouse_move(|_, window, _| window.remove_window())
            .flex()
            .flex_row()
            .items_center()
            .gap(metrics.radius_md)
            .px(metrics.radius_md)
            .rounded(metrics.radius_md)
            .bg(theme.colors.surface_raised)
            .border_1()
            .border_color(theme.colors.border)
            .font_family(theme.fonts.ui.clone())
            .font_weight(theme.fonts.ui_weight)
            .text_size(metrics.text_sm)
            .text_color(theme.colors.text)
            .child(div().min_w_0().truncate().child(self.label.clone()))
            .children(self.keys.clone().map(|keys| {
                div()
                    .flex_shrink_0()
                    .text_color(theme.colors.text_muted)
                    .child(keys)
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_read_like_windows_shortcuts() {
        let ctrl_shift = gpui::Modifiers {
            control: true,
            shift: true,
            ..Default::default()
        };
        assert_eq!(key_label(&ctrl_shift, "c"), "Ctrl+Shift+C");
        assert_eq!(key_label(&gpui::Modifiers::default(), "f5"), "F5");
        let alt = gpui::Modifiers {
            alt: true,
            ..Default::default()
        };
        assert_eq!(key_label(&alt, "left"), "Alt+Left");
    }

    #[test]
    fn a_click_silences_its_own_tooltip_until_another_shows() {
        let label: SharedString = "Close tab".into();
        let other: SharedString = "Mute tab".into();
        SILENCED.with(|cell| *cell.borrow_mut() = Some(Silenced::Next(point(px(10.), px(10.)))));
        assert!(silenced(&label, point(px(14.), px(12.))));
        assert!(silenced(&label, point(px(90.), px(12.))));
        assert!(!silenced(&other, point(px(200.), px(12.))));
        assert!(!silenced(&label, point(px(14.), px(12.))));
    }
}
