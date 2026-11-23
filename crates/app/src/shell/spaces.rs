use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, ClickEvent, Context, ElementId, Entity,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseUpEvent, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder,
};
use gpui_kit::component::input::{Input, InputEvent, InputState};

use super::{ESCAPE_KEY, Shell};
use crate::icons;
use crate::theme::Theme;
use crate::tooltip::{Side, tooltip, tooltip_on};

const SPACE_SLIDE: Duration = Duration::from_millis(200);
const DOWNLOAD_FLIGHT: Duration = Duration::from_millis(700);
const NEW_SPACE_PREFIX: &str = "Space";

impl Shell {
    pub(super) fn switch_space(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self.browser.active_space_index();
        // A private window has one space and cannot leave it.
        if self.browser.private_shown() || index == current || !self.browser.switch_space(index) {
            return;
        }
        self.space_slide_from_right = index > current;
        self.stop_renaming_space(window, cx);
        if self.browser.space().tabs.is_empty() && self.browser.active_tab_id().is_none() {
            self.open_blank_tab(window, cx);
            self.new_tab(window, cx);
        } else {
            self.show_active_tab(window, cx);
        }
    }

    pub(super) fn create_space(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.browser.private_shown() {
            return;
        }
        let name = format!("{NEW_SPACE_PREFIX} {}", self.browser.spaces().len() + 1);
        let index = self.browser.add_space(name);
        self.switch_space(index, window, cx);
    }

    pub(super) fn on_space_name_event(
        &mut self,
        _input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::PressEnter { .. } | InputEvent::Blur => self.commit_space_name(window, cx),
            InputEvent::Focus | InputEvent::Change => {}
        }
    }

    pub(super) fn start_space_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.browser.space().name.clone();
        self.renaming_space = true;
        self.take_keyboard_from_pages();
        self.space_name_input.update(cx, |input, cx| {
            input.set_value(name, window, cx);
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        cx.notify();
    }

    fn commit_space_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.renaming_space {
            return;
        }
        let name = self.space_name_input.read(cx).value().trim().to_string();
        if !name.is_empty() {
            self.browser.space_mut().name = name;
        }
        self.stop_renaming_space(window, cx);
    }

    fn cancel_space_rename_on_escape(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key == ESCAPE_KEY {
            self.stop_renaming_space(window, cx);
        }
    }

    fn stop_renaming_space(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.renaming_space {
            self.renaming_space = false;
            window.focus(&self.focus_handle, cx);
            cx.notify();
        }
    }

    /// A download icon flying from the page to the downloads button in an
    /// arc, shrinking as it lands, when a download starts.
    pub(super) fn render_download_flight(
        &self,
        viewport: gpui::Size<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (id, started) = self.download_flight?;
        if started.elapsed() > DOWNLOAD_FLIGHT {
            return None;
        }
        let theme = Theme::of(cx);
        let metrics = &theme.metrics;
        let accent = self.palette(cx).accent;
        let content = self.content_bounds.get();
        let from = if content.size.width > gpui::Pixels::ZERO {
            content.center()
        } else {
            gpui::point(viewport.width / 2., viewport.height / 2.)
        };
        let inset = if self.compact {
            metrics.frame_inset
        } else {
            gpui::Pixels::ZERO
        };
        let to = gpui::point(
            inset + metrics.frame_inset + metrics.nav_button_size / 2.,
            viewport.height - inset - metrics.bottom_bar_height / 2.,
        );
        let size = metrics.nav_button_size * 1.6;
        let lift = metrics.space_slide_distance * 6.;
        let ease = crate::motion::curve(crate::motion::Easing::EaseInOut);
        Some(
            div()
                .absolute()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(accent)
                .text_color(gpui::white())
                .shadow_lg()
                .child(icons::DOWNLOAD)
                .with_animation(
                    ElementId::Name(format!("download-flight-{id}").into()),
                    Animation::new(DOWNLOAD_FLIGHT),
                    move |icon, delta| {
                        let t = ease(delta);
                        let x = from.x + (to.x - from.x) * t;
                        let y =
                            from.y + (to.y - from.y) * t - lift * (std::f32::consts::PI * t).sin();
                        let side = size * (1.0 - 0.6 * t);
                        icon.size(side)
                            .left(x - side / 2.)
                            .top(y - side / 2.)
                            .text_size(side * 0.45)
                            .opacity(if delta > 0.85 {
                                (1.0 - delta) / 0.15
                            } else {
                                1.0
                            })
                    },
                )
                .into_any_element(),
        )
    }

    pub(super) fn animate_space_content(
        &self,
        content: impl IntoElement,
        cx: &Context<Self>,
    ) -> AnyElement {
        let distance = Theme::of(cx).metrics.space_slide_distance;
        let direction = if self.space_slide_from_right {
            1.0
        } else {
            -1.0
        };
        let id =
            ElementId::Name(format!("space-content-{}", self.browser.space().id.value()).into());
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(content)
            .with_animation(
                id,
                Animation::new(SPACE_SLIDE)
                    .with_easing(crate::motion::curve(crate::motion::Easing::EaseOut)),
                move |element, delta| {
                    element
                        .left(distance * ((1.0 - delta) * direction))
                        .opacity(delta)
                },
            )
            .into_any_element()
    }

    /// The sidebar footer: downloads on the left, the space button in the
    /// middle (opens the space switcher above it) and the features button on
    /// the right.
    pub(super) fn render_bottom_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let hover_bg = theme.colors.surface_hover;
        let accent = self.palette(cx).accent;
        let space = self.browser.space();
        let active_index = self.browser.active_space_index();
        let downloads_color = if self.downloads.active_count() > 0 {
            accent
        } else {
            theme.colors.text_muted
        };

        let center: AnyElement = if self.renaming_space {
            div()
                .flex_1()
                .min_w_0()
                .on_key_down(cx.listener(Self::cancel_space_rename_on_escape))
                .child(
                    Input::new(&self.space_name_input)
                        .appearance(false)
                        .px(theme.metrics.radius_sm)
                        .h(theme.metrics.nav_button_size)
                        .rounded(theme.metrics.radius_sm)
                        .bg(theme.colors.surface_hover)
                        .aria_label("Space name"),
                )
                .into_any_element()
        } else {
            let private = self.browser.private_shown();
            header_button("space-button", hover_bg, cx)
                .min_w_0()
                .gap(theme.metrics.radius_md)
                .px(theme.metrics.radius_md)
                .when(!private, |button| {
                    button
                        .cursor_pointer()
                        .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                            this.open_space_switcher(event.position(), window, cx)
                        }))
                        .on_mouse_up(
                            MouseButton::Right,
                            cx.listener(move |this, event: &MouseUpEvent, window, cx| {
                                this.open_space_menu(active_index, event.position, window, cx)
                            }),
                        )
                })
                .child(icons::space_icon_view(&space.icon, space.icon_color))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_weight(gpui::FontWeight::BOLD)
                        .child(space.name.clone()),
                )
                .when(!private, |button| {
                    button.child(
                        div()
                            .text_size(theme.metrics.icon_glyph)
                            .text_color(theme.colors.text_muted)
                            .child(icons::CHEVRON_UP),
                    )
                })
                .into_any_element()
        };

        div()
            .flex()
            .flex_row()
            .flex_shrink_0()
            .items_center()
            .gap(theme.metrics.radius_sm)
            .h(theme.metrics.bottom_bar_height)
            .child(
                header_button("downloads-toggle", hover_bg, cx)
                    .size(theme.metrics.nav_button_size)
                    .relative()
                    .when(self.downloads_open, |button| {
                        button.bg(theme.colors.surface_active)
                    })
                    .text_size(theme.metrics.nav_glyph)
                    .text_color(downloads_color)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_panel(super::panel_popup::Panel::Downloads, window, cx)
                    }))
                    .tooltip(tooltip("Downloads"))
                    .child(icons::DOWNLOAD)
                    .children(
                        self.downloads
                            .overall_fraction()
                            .map(|fraction| download_ring(fraction, accent)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_w_0()
                    .justify_center()
                    .child(center),
            )
            .child(
                header_button("features-button", hover_bg, cx)
                    .size(theme.metrics.nav_button_size)
                    .rounded_full()
                    .bg(theme.colors.surface_hover)
                    .text_size(theme.metrics.nav_glyph)
                    .text_color(theme.colors.text_muted)
                    .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                        this.open_features_menu(event.position(), window, cx)
                    }))
                    .tooltip(tooltip_on(Side::Left, "More"))
                    .child(icons::APPS),
            )
    }
}

/// A ring around the downloads button that fills as downloads complete.
fn download_ring(fraction: f32, color: gpui::Hsla) -> impl IntoElement {
    let stroke = gpui::px(2.);
    div()
        .absolute()
        .inset(stroke)
        .rounded_full()
        .border_2()
        .border_color(color.opacity(0.2))
        .child(
            gpui::canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let radius = bounds.size.width.min(bounds.size.height) / 2. - stroke / 2.;
                    let center = bounds.center();
                    let sweep = fraction.clamp(0.001, 0.999) * std::f32::consts::TAU;
                    let start = gpui::point(center.x, center.y - radius);
                    let end = gpui::point(
                        center.x + radius * sweep.sin(),
                        center.y - radius * sweep.cos(),
                    );
                    let mut path = gpui::PathBuilder::stroke(stroke);
                    path.move_to(start);
                    path.arc_to(
                        gpui::point(radius, radius),
                        gpui::px(0.),
                        sweep > std::f32::consts::PI,
                        true,
                        end,
                    );
                    if let Ok(path) = path.build() {
                        window.paint_path(path, color);
                    }
                },
            )
            .absolute()
            .top(-stroke)
            .left(-stroke)
            .right(-stroke)
            .bottom(-stroke),
        )
}

fn header_button(
    id: impl Into<ElementId>,
    hover_bg: gpui::Hsla,
    cx: &Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::of(cx);
    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .min_w(theme.metrics.favicon_size + theme.metrics.radius_sm)
        .h(theme.metrics.nav_button_size)
        .rounded(theme.metrics.radius_md)
        .hover(move |style| style.bg(hover_bg))
}
