use std::sync::Arc;

use browser_core::tabs::Tab;
use gpui::{
    AnyElement, App, AppContext, Context, Image, InteractiveElement, IntoElement, MouseButton,
    MouseUpEvent, ParentElement, StatefulInteractiveElement, Styled, div, img,
    prelude::FluentBuilder,
};

use super::Shell;
use super::tab_list::DraggedTab;
use crate::icons;
use crate::page;
use crate::theme::Theme;
use crate::tooltip::{Side, tooltip_on};

const DROP_HINT: &str = "Drop here to add to essentials";
const UNLOADED_OPACITY: f32 = 0.55;

impl Shell {
    pub(super) fn render_essentials(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let hint = self.essentials_hint.fraction();
        if self.browser.essentials().is_empty() && hint <= 0.0 {
            return None;
        }
        let theme = Theme::of(cx);
        let accent = self.palette(cx).accent;

        if self.browser.essentials().is_empty() {
            return Some(
                div()
                    .id("essentials-drop-hint")
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(theme.metrics.essential_tile_height * hint)
                    .mt(-theme.metrics.radius_sm * (1.0 - hint))
                    .opacity(hint)
                    .overflow_hidden()
                    .rounded(theme.metrics.radius_md)
                    .border_1()
                    .border_dashed()
                    .border_color(theme.colors.border)
                    .text_size(theme.metrics.text_sm)
                    .text_color(theme.colors.text_muted)
                    .drag_over::<DraggedTab>(move |style, _, _, _| style.border_color(accent))
                    .on_drop(cx.listener(|this, dragged: &DraggedTab, _, cx| {
                        this.drop_on_essentials(dragged.id, None, cx)
                    }))
                    .child(DROP_HINT)
                    .into_any_element(),
            );
        }

        let per_row = usize::from(Theme::of(cx).metrics.essentials_per_row);
        let tiles: Vec<AnyElement> = self
            .browser
            .essentials()
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let side = if index % per_row == 0 {
                    Side::Right
                } else {
                    Side::Left
                };
                self.render_essential_tile(tab, side, cx).into_any_element()
            })
            .collect();
        let theme = Theme::of(cx);

        Some(
            div()
                .id("essentials")
                .grid()
                .grid_cols(theme.metrics.essentials_per_row)
                .flex_shrink_0()
                .gap(theme.metrics.radius_sm)
                .rounded(theme.metrics.radius_md)
                .drag_over::<DraggedTab>(move |style, _, _, _| style.bg(accent.opacity(0.12)))
                .on_drop(cx.listener(|this, dragged: &DraggedTab, _, cx| {
                    this.drop_on_essentials(dragged.id, None, cx)
                }))
                .children(tiles)
                .into_any_element(),
        )
    }

    fn render_essential_tile(
        &self,
        tab: &Tab,
        side: Side,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::of(cx);
        let accent = self.palette(cx).accent;
        let id = tab.id;
        let is_active = self.browser.active_tab_id() == Some(id);
        let runtime = self.runtime.get(&id);
        let loaded = runtime.is_some_and(|runtime| runtime.view.is_some());
        let favicon = runtime.and_then(|runtime| runtime.favicon.clone());
        let hover_bg = theme.colors.surface_active;
        let dragged = DraggedTab {
            id,
            title: page::display_title(tab),
            favicon: favicon.clone(),
        };

        div()
            .id(("essential", id.value()))
            .flex()
            .items_center()
            .justify_center()
            .h(theme.metrics.essential_tile_height)
            .rounded(theme.metrics.radius_md * 1.5)
            .border_1()
            .map(|tile| {
                if is_active {
                    tile.bg(theme.colors.surface_active).border_color(accent)
                } else {
                    tile.bg(theme.colors.surface_hover)
                        .border_color(gpui::transparent_black())
                        .hover(move |style| style.bg(hover_bg))
                }
            })
            .on_click(cx.listener(move |this, _, window, cx| this.activate(id, window, cx)))
            .tooltip(tooltip_on(side, dragged.title.clone()))
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseUpEvent, window, cx| {
                    this.open_tab_menu(id, event.position, window, cx)
                }),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(move |this, _, window, cx| this.close_tab(id, window, cx)),
            )
            .on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
            .drag_over::<DraggedTab>(move |style, _, _, _| style.border_color(accent))
            .on_drop(cx.listener(move |this, dragged: &DraggedTab, _, cx| {
                cx.stop_propagation();
                this.drop_on_essentials(dragged.id, Some(id), cx)
            }))
            .child(
                div()
                    .when(!loaded, |icon| icon.opacity(UNLOADED_OPACITY))
                    .child(essential_icon(favicon, cx)),
            )
    }
}

fn essential_icon(image: Option<Arc<Image>>, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    let size = theme.metrics.essential_icon_size;
    match image {
        Some(image) => img(image).size(size).into_any_element(),
        None => div()
            .font_family(theme.fonts.icons.clone())
            .text_size(theme.metrics.essential_icon_size * 0.8)
            .text_color(theme.colors.text_muted)
            .child(icons::GLOBE)
            .into_any_element(),
    }
}
