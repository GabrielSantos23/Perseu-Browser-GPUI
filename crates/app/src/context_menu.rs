use gpui::{
    Animation, AnimationExt as _, App, Context, EventEmitter, FocusHandle, Hsla,
    InteractiveElement, IntoElement, KeyBinding, ParentElement, Pixels, Render, Rgba, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, actions, div, prelude::FluentBuilder,
    px, rgb,
};

use crate::icons::Icon;
use crate::motion;
use crate::theme::Theme;

actions!(context_menu, [SelectNext, SelectPrevious, Confirm, Dismiss]);

const KEY_CONTEXT: &str = "ContextMenu";
const BORDER: f32 = 1.;
// Zephium's owner-drawn Windows menu, in logical pixels.
const ROW: f32 = 32.;
const PLATE_INSET_X: f32 = 5.;
const PLATE_INSET_Y: f32 = 2.;
const PLATE_RADIUS: f32 = 8.;
const SEPARATOR: f32 = 9.;
const SEPARATOR_INSET: f32 = 12.;
const MARK_COLUMN: f32 = 27.;
const END_PAD: f32 = 15.;
const PAD_Y: f32 = 4.;
const MENU_RADIUS: f32 = 8.;
const TEXT: f32 = 12.;
const SHORTCUT_GAP: f32 = 28.;
const SHORTCUT_EXTRA_WIDTH: f32 = 80.;
const CHECK_MARK: &str = "\u{2713}";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", SelectNext, Some(KEY_CONTEXT)),
        KeyBinding::new("up", SelectPrevious, Some(KEY_CONTEXT)),
        KeyBinding::new("enter", Confirm, Some(KEY_CONTEXT)),
        KeyBinding::new("escape", Dismiss, Some(KEY_CONTEXT)),
    ]);
}

/// Zephium's dark and light menu palettes, shared by every menu-like popup.
pub struct MenuColors {
    pub background: Rgba,
    pub plate: Rgba,
    pub separator: Rgba,
    pub border: Rgba,
    pub text: Rgba,
    pub disabled: Rgba,
    pub shortcut: Rgba,
}

/// Corner radius of menus and menu-like popups.
pub const MENU_CORNER: Pixels = px(MENU_RADIUS);

impl MenuColors {
    pub fn of(cx: &App) -> Self {
        Self::new(Theme::of(cx).colors.light)
    }

    fn new(light: bool) -> Self {
        if light {
            Self {
                background: rgb(0xf9f9fb),
                plate: rgb(0xe6e6eb),
                separator: rgb(0xd7d7dc),
                border: rgb(0xd4d4da),
                text: rgb(0x1c1c1f),
                disabled: rgb(0xa6a6ac),
                shortcut: rgb(0x76767e),
            }
        } else {
            Self {
                background: rgb(0x28282d),
                plate: rgb(0x3c3c42),
                separator: rgb(0x414146),
                border: rgb(0x404046),
                text: rgb(0xf0f0f3),
                disabled: rgb(0x6c6c73),
                shortcut: rgb(0x96969e),
            }
        }
    }
}

pub enum MenuEntry<T> {
    Item {
        label: SharedString,
        value: T,
        danger: bool,
        icon: Option<Icon>,
        checked: bool,
        shortcut: Option<SharedString>,
        disabled: bool,
    },
    Separator,
}

impl<T> MenuEntry<T> {
    pub fn item(label: impl Into<SharedString>, value: T) -> Self {
        Self::Item {
            label: label.into(),
            value,
            danger: false,
            icon: None,
            checked: false,
            shortcut: None,
            disabled: false,
        }
    }

    pub fn danger(label: impl Into<SharedString>, value: T) -> Self {
        let mut entry = Self::item(label, value);
        if let Self::Item { danger, .. } = &mut entry {
            *danger = true;
        }
        entry
    }

    /// Shows `glyph` before the label.
    pub fn with_icon(mut self, glyph: Option<Icon>) -> Self {
        if let Self::Item { icon, .. } = &mut self {
            *icon = glyph;
        }
        self
    }

    /// Shows a check mark in the leading column when `on`.
    pub fn with_check(mut self, on: bool) -> Self {
        if let Self::Item { checked, .. } = &mut self {
            *checked = on;
        }
        self
    }

    /// Shows `keys` at the row's end, a step quieter than the label.
    pub fn with_shortcut(mut self, keys: Option<String>) -> Self {
        if let Self::Item { shortcut, .. } = &mut self {
            *shortcut = keys.map(Into::into);
        }
        self
    }

    /// Greys the row out and ignores it when `off`.
    pub fn disabled(mut self, off: bool) -> Self {
        if let Self::Item { disabled, .. } = &mut self {
            *disabled = off;
        }
        self
    }

    fn enabled(&self) -> bool {
        matches!(
            self,
            Self::Item {
                disabled: false,
                ..
            }
        )
    }
}

pub enum MenuEvent<T> {
    Chosen(T),
    Dismiss,
}

pub fn menu_height<T>(entries: &[MenuEntry<T>], _: &App) -> Pixels {
    let rows: Pixels = entries
        .iter()
        .map(|entry| match entry {
            MenuEntry::Item { .. } => px(ROW),
            MenuEntry::Separator => px(SEPARATOR),
        })
        .sum();
    rows + px(PAD_Y * 2. + BORDER * 2.)
}

/// Wider when a row shows a shortcut, so label and keys never meet.
pub fn menu_width<T>(entries: &[MenuEntry<T>], cx: &App) -> Pixels {
    let width = Theme::of(cx).metrics.menu_width;
    let shortcuts = entries.iter().any(|entry| {
        matches!(
            entry,
            MenuEntry::Item {
                shortcut: Some(_),
                ..
            }
        )
    });
    if shortcuts {
        width + px(SHORTCUT_EXTRA_WIDTH)
    } else {
        width
    }
}

/// How long a menu takes to fade in or out.
pub const MENU_FADE: std::time::Duration = std::time::Duration::from_millis(140);

pub struct ContextMenu<T: Clone + 'static> {
    entries: Vec<MenuEntry<T>>,
    selected: Option<usize>,
    closing: bool,
    focus_handle: FocusHandle,
    _activation: Subscription,
}

impl<T: Clone + 'static> EventEmitter<MenuEvent<T>> for ContextMenu<T> {}

impl<T: Clone + 'static> ContextMenu<T> {
    pub fn new(entries: Vec<MenuEntry<T>>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        let activation = cx.observe_window_activation(window, |_, window, cx| {
            if !window.is_window_active() {
                cx.emit(MenuEvent::Dismiss);
            }
        });
        Self {
            entries,
            selected: None,
            closing: false,
            focus_handle,
            _activation: activation,
        }
    }

    /// Starts fading the menu out; its window is removed after `MENU_FADE`.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.closing = true;
        cx.notify();
    }

    fn item_indexes(&self) -> Vec<usize> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.enabled())
            .map(|(index, _)| index)
            .collect()
    }

    fn move_selection(&mut self, forward: bool, cx: &mut Context<Self>) {
        let items = self.item_indexes();
        let Some(&first) = items.first() else {
            return;
        };
        let current = self
            .selected
            .and_then(|selected| items.iter().position(|index| *index == selected));
        let next = match (current, forward) {
            (None, true) => first,
            (None, false) => items[items.len() - 1],
            (Some(position), true) => items[(position + 1) % items.len()],
            (Some(position), false) => items[(position + items.len() - 1) % items.len()],
        };
        self.selected = Some(next);
        cx.notify();
    }

    fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(entry @ MenuEntry::Item { value, .. }) = self.entries.get(index)
            && entry.enabled()
        {
            cx.emit(MenuEvent::Chosen(value.clone()));
        }
    }
}

impl<T: Clone + 'static> Render for ContextMenu<T> {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let closing = self.closing;
        let theme = Theme::of(cx);
        let metrics = &theme.metrics;
        let colors = MenuColors::new(theme.colors.light);
        let rows: Vec<_> = self
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| match entry {
                MenuEntry::Separator => div()
                    .h(px(SEPARATOR))
                    .flex()
                    .items_center()
                    .px(px(SEPARATOR_INSET))
                    .child(div().w_full().h(px(BORDER)).bg(colors.separator))
                    .into_any_element(),
                MenuEntry::Item {
                    label,
                    danger,
                    icon,
                    checked,
                    shortcut,
                    disabled,
                    ..
                } => {
                    let disabled = *disabled;
                    let text: Hsla = match (disabled, *danger) {
                        (true, _) => colors.disabled.into(),
                        (false, true) => theme.colors.danger,
                        (false, false) => colors.text.into(),
                    };
                    let lit = !disabled && self.selected == Some(index);
                    div()
                        .id(("menu-item", index))
                        .h(px(ROW))
                        .px(px(PLATE_INSET_X))
                        .py(px(PLATE_INSET_Y))
                        .when(!disabled, |row| {
                            row.on_mouse_move(cx.listener(move |this, _, _, cx| {
                                if this.selected != Some(index) {
                                    this.selected = Some(index);
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| this.choose(index, cx)))
                        })
                        .child(
                            div()
                                .size_full()
                                .flex()
                                .items_center()
                                .rounded(px(PLATE_RADIUS))
                                .pr(px(END_PAD))
                                .text_color(text)
                                .when(lit, |row| row.bg(colors.plate))
                                .child(
                                    div()
                                        .flex()
                                        .flex_shrink_0()
                                        .justify_center()
                                        .w(px(MARK_COLUMN))
                                        .when(*checked, |mark| mark.child(CHECK_MARK)),
                                )
                                .children(icon.map(|icon| {
                                    div()
                                        .flex()
                                        .flex_shrink_0()
                                        .justify_center()
                                        .w(metrics.favicon_size)
                                        .mr(metrics.radius_md)
                                        .text_size(metrics.nav_glyph)
                                        .child(icon)
                                }))
                                .child(div().flex_1().min_w_0().truncate().child(label.clone()))
                                .children(shortcut.clone().map(|keys| {
                                    div()
                                        .flex_shrink_0()
                                        .pl(px(SHORTCUT_GAP))
                                        .when(!disabled, |keys| keys.text_color(colors.shortcut))
                                        .child(keys)
                                })),
                        )
                        .into_any_element()
                }
            })
            .collect();

        div()
            .id("context-menu")
            .track_focus(&self.focus_handle)
            .key_context(KEY_CONTEXT)
            .size_full()
            .flex()
            .flex_col()
            .py(px(PAD_Y))
            .rounded(px(MENU_RADIUS))
            .bg(colors.background)
            .border_1()
            .border_color(colors.border)
            .font_family(theme.fonts.ui.clone())
            .font_weight(theme.fonts.ui_weight)
            .text_size(px(TEXT))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.move_selection(true, cx)))
            .on_action(
                cx.listener(|this, _: &SelectPrevious, _, cx| this.move_selection(false, cx)),
            )
            .on_action(cx.listener(|this, _: &Confirm, _, cx| {
                if let Some(index) = this.selected {
                    this.choose(index, cx);
                }
            }))
            .on_action(cx.listener(|_, _: &Dismiss, _, cx| cx.emit(MenuEvent::Dismiss)))
            .children(rows)
            .with_animation(
                ("menu-fade", usize::from(closing)),
                Animation::new(MENU_FADE).with_easing(motion::curve(motion::Easing::EaseOut)),
                move |menu, delta| menu.opacity(if closing { 1.0 - delta } else { delta }),
            )
    }
}
