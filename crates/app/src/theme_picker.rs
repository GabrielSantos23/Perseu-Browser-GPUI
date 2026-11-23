// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
//
// Layout and behavior follow Zen Browser's space theme picker
// (https://github.com/zen-browser/desktop, ZenGradientGenerator.mjs).

mod zen;

use std::cell::Cell;
use std::f32::consts::{PI, TAU};
use std::rc::Rc;

use browser_core::browser::{DEFAULT_THEME_INTENSITY, SpaceTheme, TextureKind};
use gpui::{
    AnyElement, App, AppContext, Bounds, Context, ElementId, Entity, EventEmitter, FocusHandle,
    Hsla, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    ParentElement, PathBuilder, Pixels, Point, Render, ScrollEvent, StatefulInteractiveElement,
    Styled, Subscription, Window, canvas, div, fill, hsla, linear_color_stop, linear_gradient,
    point, px, rgb, size,
};
use gpui_kit::component::input::{Input, InputEvent, InputState};

use crate::icons;
use crate::theme::Theme;
use crate::tooltip::{Side, tooltip, tooltip_on};
use zen::{Action, Dot, DotKind, PAD, PRESETS, PRESETS_PER_PAGE};

const PANEL_PADDING: f32 = 10.;
const GAP: f32 = 10.;
const HEADER: f32 = 36.;
const LABEL_ROW: f32 = 24.;
const SWATCH: f32 = 26.;
const CONTROLS: f32 = 72.;
const CUSTOM_ROW: f32 = 34.;
const CUSTOM_ITEM: f32 = 28.;
const PRIMARY_DOT: f32 = 38.;
const DOT: f32 = 16.;
const DOT_MARGIN: f32 = 10.;
const GRID_STEP: f32 = 6.;
const PAD_BUTTON: f32 = 30.;
const TEXTURE_STEPS: f32 = 16.;
const WAVE_PERIODS: f32 = 6.;
const NEUTRAL: u32 = 0x8A8A96;
const ICON_PANEL: f32 = 276.;
const ICON_CELL: f32 = 38.;
const ICON_COLOR_DOT: f32 = 20.;
const ICON_COLORS: [u32; 9] = [
    0xFF6B2B, 0xFFB800, 0x3DD16B, 0x22B8C9, 0x3D8BFF, 0x8B5CF6, 0xEC4899, 0xEF4444, 0xA855F7,
];
const PAD_HINT: &str = "Click to add a color";

pub enum ThemePickerEvent {
    Changed {
        theme: SpaceTheme,
        intensity: f32,
        texture: f32,
        texture_kind: TextureKind,
        state: String,
    },
    Renamed(String),
    SetIcon {
        name: String,
        color: Option<u32>,
    },
    CustomColors(Vec<u32>),
    Delete,
    Dismiss,
}

pub struct SpaceLook {
    pub name: String,
    pub icon: String,
    pub icon_color: Option<u32>,
    pub show_icons: bool,
    pub theme: SpaceTheme,
    pub intensity: f32,
    pub texture: f32,
    pub texture_kind: TextureKind,
    pub state: Option<String>,
    pub custom_colors: Vec<u32>,
    pub can_delete: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Drag {
    Dot,
    Wave,
    Texture,
}

/// The space editor: name, Zen's color pad with harmonies, preset pages,
/// opacity wave, texture dial and custom colors.
pub struct ThemePicker {
    icon: String,
    icon_color: Option<u32>,
    icons_open: bool,
    dots: Vec<Dot>,
    lightness: f32,
    harmony: String,
    exact_primary: Option<u32>,
    intensity: f32,
    texture: f32,
    texture_kind: TextureKind,
    page: usize,
    custom_colors: Vec<u32>,
    can_delete: bool,
    dragging: Option<Drag>,
    pad: Rc<Cell<Bounds<Pixels>>>,
    wave: Rc<Cell<Bounds<Pixels>>>,
    knob: Rc<Cell<Bounds<Pixels>>>,
    name_input: Entity<InputState>,
    hex_input: Entity<InputState>,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ThemePickerEvent> for ThemePicker {}

/// Height of the editor window for `custom` custom colors.
pub fn picker_height(custom: usize, can_delete: bool, _: &App) -> Pixels {
    let delete = if can_delete { LABEL_ROW + GAP } else { 0. };
    px(PANEL_PADDING * 2.
        + HEADER
        + LABEL_ROW
        + PAD
        + SWATCH
        + CONTROLS
        + LABEL_ROW
        + CUSTOM_ROW
        + CUSTOM_ITEM * custom as f32
        + GAP * 7.
        + delete
        + 2.)
}

fn panel_width() -> Pixels {
    px(PAD + PANEL_PADDING * 2. + 2.)
}

/// Width of the editor window: the panel plus room for the icon grid.
pub fn picker_width() -> Pixels {
    panel_width() + px(GAP + ICON_PANEL)
}

impl ThemePicker {
    pub fn new(look: SpaceLook, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let name_input = cx.new(|cx| {
            let mut input = InputState::new(window, cx);
            input.set_value(look.name.clone(), window, cx);
            input
        });
        let hex_input = cx.new(|cx| InputState::new(window, cx).placeholder("#RRGGBB"));
        name_input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        let subscriptions = vec![
            cx.observe_window_activation(window, |_, window, cx| {
                if !window.is_window_active() {
                    cx.emit(ThemePickerEvent::Dismiss);
                }
            }),
            cx.subscribe_in(&name_input, window, |_, input, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let name = input.read(cx).value().trim().to_string();
                    if !name.is_empty() {
                        cx.emit(ThemePickerEvent::Renamed(name));
                    }
                }
            }),
            cx.subscribe_in(&hex_input, window, |this, _, event, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.add_custom_color(cx);
                }
            }),
        ];
        let (dots, lightness, harmony) = look
            .state
            .as_deref()
            .and_then(zen::decode)
            .unwrap_or_else(|| (dots_from_theme(look.theme), 50., String::new()));
        Self {
            icon: look.icon,
            icon_color: look.icon_color,
            icons_open: look.show_icons,
            dots,
            lightness,
            harmony,
            exact_primary: None,
            intensity: look.intensity,
            texture: look.texture,
            texture_kind: look.texture_kind,
            page: 0,
            custom_colors: look.custom_colors,
            can_delete: look.can_delete,
            dragging: None,
            pad: Rc::default(),
            wave: Rc::default(),
            knob: Rc::default(),
            name_input,
            hex_input,
            focus_handle,
            _subscriptions: subscriptions,
        }
    }

    fn pick_icon(&mut self, name: &str, color: Option<u32>, cx: &mut Context<Self>) {
        self.icon = name.to_string();
        self.icon_color = color;
        cx.emit(ThemePickerEvent::SetIcon {
            name: self.icon.clone(),
            color,
        });
        cx.notify();
    }

    fn render_icon_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx);
        let metrics = &theme.metrics;
        let hover_bg = theme.colors.surface_hover;
        let selected_bg = theme.colors.surface_active;
        let ring = theme.colors.text;
        let current_color = self.icon_color;
        let colors = std::iter::once(None)
            .chain(ICON_COLORS.iter().copied().map(Some))
            .enumerate()
            .map(|(index, color)| {
                let fill: gpui::Background = match color {
                    Some(color) => rgb(color).into(),
                    None => linear_gradient(
                        135.,
                        linear_color_stop(gpui::white(), 0.5),
                        linear_color_stop(gpui::black(), 0.5),
                    ),
                };
                let icon = self.icon.clone();
                div()
                    .id(("icon-color", index))
                    .size(px(ICON_COLOR_DOT))
                    .rounded_full()
                    .bg(fill)
                    .border_2()
                    .border_color(if current_color == color {
                        ring
                    } else {
                        gpui::transparent_black()
                    })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.pick_icon(&icon, color, cx)))
            });
        let tint = current_color.map_or(theme.colors.text, |color| rgb(color).into());
        let cells = std::iter::once(("", None))
            .chain(
                icons::SPACE_ICONS
                    .iter()
                    .map(|(name, icon)| (*name, Some(*icon))),
            )
            .enumerate()
            .map(|(index, (name, icon))| {
                let selected = self.icon == name;
                let cell = div()
                    .id(("space-icon", index))
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(ICON_CELL))
                    .rounded(metrics.radius_md)
                    .text_size(metrics.nav_glyph * 1.2)
                    .text_color(tint)
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover_bg))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let color = this.icon_color;
                        this.pick_icon(name, color, cx)
                    }));
                let cell = if selected { cell.bg(selected_bg) } else { cell };
                match icon {
                    Some(icon) => cell.child(icon),
                    None => cell.text_color(theme.colors.text_muted).child(icons::CLOSE),
                }
            });
        div()
            .id("icon-panel")
            .w(px(ICON_PANEL))
            .flex()
            .flex_col()
            .gap(px(GAP))
            .p(px(PANEL_PADDING))
            .rounded(metrics.radius_popover)
            .bg(theme.colors.surface_raised)
            .border_1()
            .border_color(theme.colors.border)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .pb(px(GAP))
                    .border_b_1()
                    .border_color(theme.colors.border)
                    .children(colors),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap(px(4.))
                    .children(cells),
            )
            .into_any_element()
    }

    fn colors(&self) -> Vec<u32> {
        self.dots
            .iter()
            .enumerate()
            .map(|(index, dot)| match self.exact_primary {
                Some(exact) if index == 0 => exact,
                _ => zen::color_at(*dot, self.lightness),
            })
            .collect()
    }

    fn emit(&mut self, cx: &mut Context<Self>) {
        let colors = self.colors();
        let theme = match colors.as_slice() {
            [first, second, third, ..] => SpaceTheme::Triple(*first, *second, *third),
            [first, second] => SpaceTheme::Gradient(*first, *second),
            [first] => SpaceTheme::Solid(*first),
            [] => SpaceTheme::Solid(NEUTRAL),
        };
        cx.emit(ThemePickerEvent::Changed {
            theme,
            intensity: self.intensity,
            texture: self.texture,
            texture_kind: self.texture_kind,
            state: zen::encode(&self.dots, self.lightness, &self.harmony),
        });
        cx.notify();
    }

    fn relayout(&mut self, action: Action, cx: &mut Context<Self>) {
        let (dots, harmony) = zen::compliments(&self.dots, action, &self.harmony);
        self.dots = dots;
        self.harmony = harmony;
        self.emit(cx);
    }

    fn pad_point(&self, position: Point<Pixels>) -> (f32, f32) {
        let pad = self.pad.get();
        (
            f32::from(position.x - pad.origin.x),
            f32::from(position.y - pad.origin.y),
        )
    }

    fn over_primary(&self, x: f32, y: f32) -> bool {
        self.dots.first().is_some_and(|dot| {
            (dot.x + DOT_MARGIN - x).hypot(dot.y + DOT_MARGIN - y) <= PRIMARY_DOT / 2.
        })
    }

    fn press_pad(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let (x, y) = self.pad_point(event.position);
        if self.over_primary(x, y) {
            self.dragging = Some(Drag::Dot);
            return;
        }
        let (x, y) = zen::clamp_to_circle(x, y);
        match self.dots.first_mut() {
            None => {
                self.dots.push(Dot {
                    x,
                    y,
                    kind: DotKind::Free,
                });
                self.emit(cx);
            }
            Some(primary) => {
                primary.x = x;
                primary.y = y;
                self.exact_primary = None;
                self.relayout(Action::Update, cx);
            }
        }
    }

    fn remove_primary(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let (x, y) = self.pad_point(event.position);
        if self.over_primary(x, y) {
            self.dots.remove(0);
            self.exact_primary = None;
            self.relayout(Action::Remove, cx);
        }
    }

    fn drag_to(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        match self.dragging {
            Some(Drag::Dot) => {
                let (x, y) = self.pad_point(position);
                let (x, y) = zen::clamp_to_circle(x, y);
                if let Some(primary) = self.dots.first_mut() {
                    primary.x = x;
                    primary.y = y;
                    self.exact_primary = None;
                    self.relayout(Action::Update, cx);
                }
            }
            Some(Drag::Wave) => {
                let wave = self.wave.get();
                if wave.size.width > Pixels::ZERO {
                    self.intensity =
                        ((position.x - wave.origin.x) / wave.size.width).clamp(0.0, 1.0);
                    self.emit(cx);
                }
            }
            Some(Drag::Texture) => {
                let center = self.knob.get().center();
                let rotation = f32::from(position.y - center.y)
                    .atan2(f32::from(position.x - center.x))
                    .to_degrees()
                    + 90.;
                let turn = rotation.rem_euclid(360.) / 360.;
                let mut texture = (turn * TEXTURE_STEPS).round() / TEXTURE_STEPS;
                if texture >= 1. {
                    texture = 0.;
                }
                if texture > 0. && self.texture_kind == TextureKind::None {
                    self.texture_kind = TextureKind::Grain;
                }
                if texture != self.texture {
                    self.texture = texture;
                    self.emit(cx);
                }
            }
            None => {}
        }
    }

    fn add_dot(&mut self, cx: &mut Context<Self>) {
        if self.dots.is_empty() || self.dots.len() >= zen::MAX_DOTS {
            return;
        }
        self.relayout(Action::Add, cx);
    }

    fn remove_dot(&mut self, cx: &mut Context<Self>) {
        if self.dots.pop().is_some() {
            if self.dots.is_empty() {
                self.exact_primary = None;
            }
            self.relayout(Action::Remove, cx);
        }
    }

    fn cycle_harmony(&mut self, cx: &mut Context<Self>) {
        let applicable: Vec<&zen::Harmony> = zen::HARMONIES
            .iter()
            .filter(|harmony| harmony.angles.len() + 1 == self.dots.len())
            .collect();
        if applicable.is_empty() {
            return;
        }
        let next = applicable
            .iter()
            .position(|harmony| harmony.name == self.harmony)
            .map_or(0, |index| (index + 1) % applicable.len());
        self.harmony = applicable[next].name.to_string();
        self.relayout(Action::Update, cx);
    }

    fn cycle_texture(&mut self, step: isize, cx: &mut Context<Self>) {
        self.texture_kind = self.texture_kind.cycled(step);
        if self.texture_kind == TextureKind::None {
            self.texture = 0.;
        } else if self.texture == 0. {
            self.texture = 10. / TEXTURE_STEPS;
        }
        self.emit(cx);
    }

    fn apply_preset(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(preset) = PRESETS.get(index) else {
            return;
        };
        let count = if preset.analogous { 3 } else { 1 };
        self.dots = (0..count)
            .map(|_| Dot {
                x: preset.x,
                y: preset.y,
                kind: preset.kind,
            })
            .collect();
        self.harmony = if preset.analogous {
            "analogous".into()
        } else {
            String::new()
        };
        if let Some(lightness) = preset.lightness {
            self.lightness = lightness;
        }
        let (dots, harmony) = zen::compliments(&self.dots, Action::Update, &self.harmony);
        self.dots = dots;
        self.harmony = harmony;
        self.exact_primary = (preset.kind == DotKind::Lightness).then_some(preset.colors[0]);
        self.emit(cx);
    }

    fn reset(&mut self, cx: &mut Context<Self>) {
        self.dots.clear();
        self.harmony.clear();
        self.exact_primary = None;
        self.intensity = DEFAULT_THEME_INTENSITY;
        self.texture = 0.;
        self.emit(cx);
    }

    fn add_custom_color(&mut self, cx: &mut Context<Self>) {
        let text = self.hex_input.read(cx).value().to_string();
        let Some(color) = parse_hex(&text) else {
            return;
        };
        if !self.custom_colors.contains(&color) {
            self.custom_colors.push(color);
            cx.emit(ThemePickerEvent::CustomColors(self.custom_colors.clone()));
        }
        self.apply_color(color, cx);
    }

    fn apply_color(&mut self, color: u32, cx: &mut Context<Self>) {
        let (x, y) = zen::initial_position(color);
        let (_, _, lightness) = zen::rgb_to_hsl(color);
        self.dots = vec![Dot {
            x,
            y,
            kind: DotKind::Lightness,
        }];
        self.lightness = lightness * 100.;
        self.harmony.clear();
        self.exact_primary = Some(color);
        self.emit(cx);
    }

    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx);
        let metrics = &theme.metrics;
        let hover_bg = theme.colors.surface_active;
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(GAP))
            .h(px(HEADER))
            .child(
                div()
                    .id("space-icon-button")
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .justify_center()
                    .size(px(HEADER))
                    .rounded(metrics.radius_md)
                    .bg(theme.colors.surface_hover)
                    .hover(move |style| style.bg(hover_bg))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.icons_open = !this.icons_open;
                        cx.notify();
                    }))
                    .tooltip(tooltip("Choose an icon"))
                    .child(icons::space_icon_view(&self.icon, self.icon_color)),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .px(metrics.radius_md)
                    .rounded(metrics.radius_md)
                    .border_1()
                    .border_color(theme.colors.accent)
                    .child(
                        Input::new(&self.name_input)
                            .appearance(false)
                            .px_0()
                            .aria_label("Space name"),
                    ),
            )
            .into_any_element()
    }

    fn render_pad(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx);
        let metrics = &theme.metrics;
        let pad = self.pad.clone();
        let grid = hsla(0., 0., 1., 0.05);
        let has_dots = !self.dots.is_empty();
        let colors = self.colors();
        let dots: Vec<AnyElement> = self
            .dots
            .iter()
            .zip(colors)
            .enumerate()
            .rev()
            .map(|(index, (dot, color))| {
                let (size, border) = if index == 0 {
                    (PRIMARY_DOT, 6.)
                } else {
                    (DOT, 3.)
                };
                div()
                    .absolute()
                    .left(px(dot.x + DOT_MARGIN - size / 2.))
                    .top(px(dot.y + DOT_MARGIN - size / 2.))
                    .size(px(size))
                    .rounded_full()
                    .bg(rgb(color))
                    .border(px(border))
                    .border_color(gpui::white())
                    .shadow_md()
                    .into_any_element()
            })
            .collect();
        let hover_bg = hsla(0., 0., 1., 0.1);
        let icons_font = theme.fonts.icons.clone();
        let muted = theme.colors.text_muted;
        let glyph_size = metrics.icon_glyph;
        let button = move |id: &'static str, glyph: icons::Icon, enabled: bool| {
            let base = div()
                .id(id)
                .flex()
                .items_center()
                .justify_center()
                .size(px(PAD_BUTTON))
                .rounded_full()
                .font_family(icons_font.clone())
                .text_size(glyph_size)
                .text_color(muted)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(glyph);
            if enabled {
                base.cursor_pointer().hover(move |style| style.bg(hover_bg))
            } else {
                base.opacity(0.5)
            }
        };
        let scheme = div()
            .absolute()
            .top(px(15.))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .gap(px(5.))
            .child(
                button("scheme-auto", icons::SCHEME_AUTO, false)
                    .tooltip(tooltip("Follow the system (dark only for now)")),
            )
            .child(
                button("scheme-light", icons::SCHEME_LIGHT, false)
                    .tooltip(tooltip("Light (not available yet)")),
            )
            .child(
                button("scheme-dark", icons::SCHEME_DARK, true)
                    .bg(hover_bg)
                    .tooltip(tooltip("Dark")),
            );
        let can_add = has_dots && self.dots.len() < zen::MAX_DOTS;
        let can_cycle = self.dots.len() >= 2;
        let add = button("color-add", icons::ADD, can_add).tooltip(tooltip("Add a color"));
        let add = if can_add {
            add.on_click(cx.listener(|this, _, _, cx| this.add_dot(cx)))
        } else {
            add
        };
        let remove =
            button("color-remove", icons::MINUS, has_dots).tooltip(tooltip("Remove a color"));
        let remove = if has_dots {
            remove.on_click(cx.listener(|this, _, _, cx| this.remove_dot(cx)))
        } else {
            remove
        };
        let cycle = button("color-algo", icons::HARMONY, can_cycle)
            .tooltip(tooltip("Change the color harmony"));
        let cycle = if can_cycle {
            cycle.on_click(cx.listener(|this, _, _, cx| this.cycle_harmony(cx)))
        } else {
            cycle
        };
        let actions = div()
            .absolute()
            .bottom(px(16.))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .gap(px(5.))
            .child(add)
            .child(remove)
            .child(cycle);
        div()
            .id("theme-pad")
            .relative()
            .size(px(PAD))
            .flex_shrink_0()
            .rounded(metrics.radius_popover)
            .bg(theme.colors.surface)
            .overflow_hidden()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| this.press_pad(event, cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, _, cx| this.remove_primary(event, cx)),
            )
            .child(
                canvas(
                    move |bounds, _, _| pad.set(bounds),
                    move |bounds, _, window, _| paint_grid(bounds, grid, window),
                )
                .absolute()
                .size_full(),
            )
            .children((!has_dots).then(|| {
                div()
                    .absolute()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(metrics.text_sm)
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(PAD_HINT)
            }))
            .children(dots)
            .child(scheme)
            .child(actions)
            .into_any_element()
    }

    fn render_pages(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx);
        let pages = PRESETS.len() / PRESETS_PER_PAGE;
        let border = theme.colors.border;
        let swatches: Vec<AnyElement> = (0..PRESETS_PER_PAGE)
            .map(|slot| {
                let index = self.page * PRESETS_PER_PAGE + slot;
                let preset = &PRESETS[index];
                let [first, second, third] = preset.colors;
                let fill: gpui::Background = if preset.analogous {
                    linear_gradient(
                        135.,
                        linear_color_stop(rgb(first), 0.),
                        linear_color_stop(blend(second, third), 1.),
                    )
                } else {
                    rgb(first).into()
                };
                div()
                    .id(ElementId::Name(format!("preset-{index}").into()))
                    .size(px(SWATCH))
                    .rounded_full()
                    .bg(fill)
                    .border_1()
                    .border_color(border)
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.apply_preset(index, cx)))
                    .into_any_element()
            })
            .collect();
        let icons_font = theme.fonts.icons.clone();
        let muted = theme.colors.text_muted;
        let glyph_size = theme.metrics.icon_glyph;
        let radius = theme.metrics.radius_md;
        let arrow = move |id: &'static str, glyph: icons::Icon, enabled: bool| {
            let base = div()
                .id(id)
                .flex()
                .items_center()
                .justify_center()
                .size(px(28.))
                .rounded(radius)
                .font_family(icons_font.clone())
                .text_size(glyph_size)
                .text_color(muted)
                .child(glyph);
            if enabled {
                base.cursor_pointer()
            } else {
                base.opacity(0.4)
            }
        };
        let page = self.page;
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .h(px(SWATCH))
            .child(
                arrow("page-left", icons::CHEVRON_LEFT, page > 0).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.page = (this.page + pages - 1) % pages;
                        cx.notify();
                    },
                )),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .children(swatches),
            )
            .child(
                arrow("page-right", icons::CHEVRON_RIGHT, page + 1 < pages).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.page = (this.page + 1) % pages;
                        cx.notify();
                    },
                )),
            )
            .into_any_element()
    }

    fn render_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let wave = self.wave.clone();
        let knob = self.knob.clone();
        let intensity = self.intensity;
        let texture = self.texture;
        let knob_size = px(CONTROLS);
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(GAP * 2.))
            .h(px(CONTROLS))
            .px(px(PANEL_PADDING))
            .child(
                div()
                    .id("theme-opacity")
                    .flex_1()
                    .h_full()
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            this.dragging = Some(Drag::Wave);
                            this.drag_to(event.position, cx);
                        }),
                    )
                    .tooltip(tooltip("Opacity"))
                    .child(
                        canvas(
                            move |bounds, _, _| wave.set(bounds),
                            move |bounds, _, window, _| paint_wave(bounds, intensity, window),
                        )
                        .size_full(),
                    ),
            )
            .child(
                div()
                    .id("theme-texture")
                    .relative()
                    .flex_shrink_0()
                    .size(knob_size)
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            if on_handle(this.knob.get(), this.texture, event.position) {
                                this.dragging = Some(Drag::Texture);
                            } else {
                                this.cycle_texture(1, cx);
                            }
                        }),
                    )
                    .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| {
                        let delta = event.delta.pixel_delta(px(16.));
                        let step = f32::from(delta.x + delta.y);
                        if step.abs() > 8. {
                            this.cycle_texture(if step > 0. { 1 } else { -1 }, cx);
                        }
                    }))
                    .tooltip(tooltip_on(
                        Side::Left,
                        "Click or swipe to change the texture style",
                    ))
                    .child(
                        canvas(
                            move |bounds, _, _| knob.set(bounds),
                            move |bounds, _, window, _| paint_knob(bounds, texture, window),
                        )
                        .absolute()
                        .size_full(),
                    ),
            )
            .into_any_element()
    }

    fn render_custom(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx);
        let metrics = &theme.metrics;
        let hover_bg = theme.colors.surface_active;
        let border = theme.colors.border;
        let icons_font = theme.fonts.icons.clone();
        let small = metrics.text_sm;
        let glyph = metrics.icon_glyph * 0.8;
        let radius = metrics.radius_md;
        let swatch = parse_hex(self.hex_input.read(cx).value().as_ref())
            .map(|color| rgb(color).into())
            .unwrap_or(theme.colors.surface_hover);
        let items: Vec<AnyElement> = self
            .custom_colors
            .iter()
            .enumerate()
            .map(|(index, color)| {
                let color = *color;
                div()
                    .id(("custom-color", index))
                    .group("custom-color")
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(GAP))
                    .h(px(CUSTOM_ITEM))
                    .px(px(5.))
                    .rounded(radius)
                    .hover(move |style| style.bg(hover_bg))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.apply_color(color, cx)))
                    .child(
                        div()
                            .size(px(16.))
                            .rounded(px(5.))
                            .border_1()
                            .border_color(border)
                            .bg(rgb(color)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(small)
                            .font_weight(gpui::FontWeight::BOLD)
                            .child(format!("#{color:06X}")),
                    )
                    .child(
                        div()
                            .id(("custom-color-remove", index))
                            .px(px(4.))
                            .opacity(0.)
                            .group_hover("custom-color", |style| style.opacity(1.))
                            .font_family(icons_font.clone())
                            .text_size(glyph)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.custom_colors.retain(|known| *known != color);
                                cx.emit(ThemePickerEvent::CustomColors(this.custom_colors.clone()));
                                cx.notify();
                            }))
                            .child(icons::CLOSE),
                    )
                    .into_any_element()
            })
            .collect();
        let theme = Theme::of(cx);
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(LABEL_ROW))
                    .text_size(small)
                    .font_weight(gpui::FontWeight::BOLD)
                    .child("Custom colour"),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(GAP))
                    .h(px(CUSTOM_ROW))
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(GAP))
                            .px(radius)
                            .rounded(radius)
                            .border_1()
                            .border_color(border)
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .size(px(16.))
                                    .rounded(px(5.))
                                    .bg(swatch),
                            )
                            .child(
                                Input::new(&self.hex_input)
                                    .appearance(false)
                                    .px_0()
                                    .aria_label("Custom color"),
                            ),
                    )
                    .child(
                        div()
                            .id("custom-color-add")
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(CUSTOM_ROW))
                            .rounded(radius)
                            .border_1()
                            .border_color(border)
                            .hover(move |style| style.bg(hover_bg))
                            .font_family(theme.fonts.icons.clone())
                            .text_size(theme.metrics.icon_glyph)
                            .on_click(cx.listener(|this, _, _, cx| this.add_custom_color(cx)))
                            .tooltip(tooltip_on(Side::Left, "Add this color"))
                            .child(icons::ADD),
                    ),
            )
            .children(items)
            .into_any_element()
    }
}

impl Render for ThemePicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.render_header(cx);
        let pad = self.render_pad(cx);
        let pages = self.render_pages(cx);
        let controls = self.render_controls(cx);
        let custom = self.render_custom(cx);
        let icon_panel = self.icons_open.then(|| self.render_icon_panel(cx));
        let can_delete = self.can_delete;
        let theme = Theme::of(cx);
        let metrics = &theme.metrics;
        let hover_bg = theme.colors.surface_hover;
        let panel = div()
            .id("theme-picker")
            .track_focus(&self.focus_handle)
            .w(panel_width())
            .h_full()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .p(px(PANEL_PADDING))
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
                    cx.emit(ThemePickerEvent::Dismiss);
                }
            }))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.dragging.is_some() {
                    if event.pressed_button == Some(MouseButton::Left) {
                        this.drag_to(event.position, cx);
                    } else {
                        this.dragging = None;
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.dragging = None),
            )
            .child(header)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .h(px(LABEL_ROW))
                    .child(div().font_weight(gpui::FontWeight::BOLD).child("Theme"))
                    .child(
                        div()
                            .id("theme-reset")
                            .px(metrics.radius_md)
                            .rounded(metrics.radius_sm)
                            .text_size(metrics.text_sm)
                            .text_color(theme.colors.text_muted)
                            .hover(move |style| style.bg(hover_bg))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| this.reset(cx)))
                            .child("Reset"),
                    ),
            )
            .child(pad)
            .child(pages)
            .child(controls)
            .child(custom)
            .children(can_delete.then(|| {
                div()
                    .id("delete-space")
                    .flex()
                    .items_center()
                    .h(px(LABEL_ROW))
                    .px(metrics.radius_md)
                    .rounded(metrics.radius_md)
                    .text_color(theme.colors.danger)
                    .hover(move |style| style.bg(hover_bg))
                    .cursor_pointer()
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ThemePickerEvent::Delete)))
                    .child("Delete Space")
            }));
        div()
            .id("theme-picker-window")
            .size_full()
            .flex()
            .flex_row()
            .items_start()
            .gap(px(GAP))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(ThemePickerEvent::Dismiss)),
            )
            .child(
                div()
                    .h_full()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(panel),
            )
            .children(icon_panel.map(|icon_panel| {
                div()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(icon_panel)
            }))
    }
}

fn dots_from_theme(theme: SpaceTheme) -> Vec<Dot> {
    let colors: Vec<u32> = match theme {
        SpaceTheme::Solid(color) => vec![color],
        SpaceTheme::Gradient(start, end) => vec![start, end],
        SpaceTheme::Triple(start, end, third) => vec![start, end, third],
    };
    colors
        .into_iter()
        .map(|color| {
            let (x, y) = zen::initial_position(color);
            Dot {
                x,
                y,
                kind: DotKind::Free,
            }
        })
        .collect()
}

fn blend(first: u32, second: u32) -> Hsla {
    let channel = |shift: u32| (((first >> shift) & 0xFF) + ((second >> shift) & 0xFF)) / 2;
    rgb((channel(16) << 16) | (channel(8) << 8) | channel(0)).into()
}

fn parse_hex(text: &str) -> Option<u32> {
    let digits = text.trim().trim_start_matches('#');
    (digits.len() == 6)
        .then(|| u32::from_str_radix(digits, 16).ok())
        .flatten()
}

fn paint_grid(bounds: Bounds<Pixels>, color: Hsla, window: &mut Window) {
    let mut y = f32::from(bounds.origin.y) + GRID_STEP / 2.;
    let bottom = f32::from(bounds.origin.y + bounds.size.height);
    let right = f32::from(bounds.origin.x + bounds.size.width);
    while y < bottom {
        let mut x = f32::from(bounds.origin.x) + GRID_STEP / 2.;
        while x < right {
            window.paint_quad(fill(
                Bounds::new(point(px(x), px(y)), size(px(1.), px(1.))),
                color,
            ));
            x += GRID_STEP;
        }
        y += GRID_STEP;
    }
}

/// Zen's opacity slider: the line turns into a sine wave as opacity grows,
/// solid up to the thumb and faded after it.
fn paint_wave(bounds: Bounds<Pixels>, progress: f32, window: &mut Window) {
    let left = f32::from(bounds.origin.x) + 6.;
    let right = f32::from(bounds.origin.x + bounds.size.width) - 6.;
    let middle = f32::from(bounds.origin.y) + f32::from(bounds.size.height) / 2.;
    let track_height = 18.;
    window.paint_quad(
        fill(
            Bounds::new(
                point(px(left), px(middle - track_height / 2.)),
                size(px(right - left), px(track_height)),
            ),
            hsla(0., 0., 1., 0.1),
        )
        .corner_radii(px(track_height / 2.)),
    );
    let amplitude = progress * f32::from(bounds.size.height) * 0.28;
    let period = (right - left) / WAVE_PERIODS;
    let thumb_x = left + (right - left) * progress;
    let wave_point = |x: f32| {
        let phase = (x - left) / period * TAU;
        point(px(x), px(middle - phase.sin() * amplitude))
    };
    for (from, to, color) in [
        (left, thumb_x, hsla(0., 0., 0.63, 1.)),
        (thumb_x, right, hsla(0., 0., 0.63, 0.5)),
    ] {
        if to - from < 1. {
            continue;
        }
        let mut path = PathBuilder::stroke(px(4.));
        path.move_to(wave_point(from));
        let mut x = from;
        while x < to {
            x = (x + 2.).min(to);
            path.line_to(wave_point(x));
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, color);
        }
    }
    let thumb_width = 10. + progress * 15.;
    let thumb_height = (40. + progress * 15.).min(f32::from(bounds.size.height));
    window.paint_quad(
        fill(
            Bounds::new(
                point(
                    px(thumb_x - thumb_width / 2.),
                    px(middle - thumb_height / 2.),
                ),
                size(px(thumb_width), px(thumb_height)),
            ),
            gpui::white(),
        )
        .corner_radii(px(thumb_width / 2.)),
    );
}

/// Zen's texture dial: 16 dots around a face, lit up to the current value,
/// and a handle at the value's angle.
fn paint_knob(bounds: Bounds<Pixels>, texture: f32, window: &mut Window) {
    let center = bounds.center();
    let radius = f32::from(bounds.size.width.min(bounds.size.height)) / 2. - 3.;
    for step in 0..16 {
        let angle = step as f32 / TEXTURE_STEPS * TAU - PI / 2.;
        let lit = texture > 0. && step as f32 / TEXTURE_STEPS <= texture;
        let at = point(
            center.x + px(angle.cos() * radius),
            center.y + px(angle.sin() * radius),
        );
        window.paint_quad(
            fill(
                Bounds::new(at - point(px(2.), px(2.)), size(px(4.), px(4.))),
                hsla(0., 0., 1., if lit { 0.3 } else { 0.12 }),
            )
            .corner_radii(px(2.)),
        );
    }
    let face = radius * 0.62;
    window.paint_quad(
        fill(
            Bounds::new(
                center - point(px(face), px(face)),
                size(px(face * 2.), px(face * 2.)),
            ),
            hsla(0., 0., 1., 0.06),
        )
        .corner_radii(px(face))
        .border_widths(px(1.))
        .border_color(hsla(0., 0., 1., 0.12)),
    );
    let angle = texture * TAU - PI / 2.;
    let mut path = PathBuilder::stroke(px(6.));
    let outer = radius - 2.;
    let inner = radius - 14.;
    path.move_to(point(
        center.x + px(angle.cos() * outer),
        center.y + px(angle.sin() * outer),
    ));
    path.line_to(point(
        center.x + px(angle.cos() * inner),
        center.y + px(angle.sin() * inner),
    ));
    if let Ok(path) = path.build() {
        window.paint_path(path, gpui::white());
    }
}

fn on_handle(knob: Bounds<Pixels>, texture: f32, position: Point<Pixels>) -> bool {
    let center = knob.center();
    let radius = f32::from(knob.size.width) / 2. - 11.;
    let angle = texture * TAU - PI / 2.;
    let handle = point(
        center.x + px(angle.cos() * radius),
        center.y + px(angle.sin() * radius),
    );
    f32::from(position.x - handle.x).hypot(f32::from(position.y - handle.y)) <= 14.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_colors_parse_with_or_without_hash() {
        assert_eq!(parse_hex("#7C8CFF"), Some(0x7C8CFF));
        assert_eq!(parse_hex(" a1b2c3 "), Some(0xA1B2C3));
        assert_eq!(parse_hex("#12345"), None);
    }

    #[test]
    fn saved_themes_reopen_with_one_dot_per_color() {
        assert_eq!(dots_from_theme(SpaceTheme::Solid(0xFF0000)).len(), 1);
        assert_eq!(dots_from_theme(SpaceTheme::Triple(1, 2, 3)).len(), 3);
    }
}
