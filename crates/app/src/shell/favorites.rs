use browser_core::tabs::{Section, Tab, TabId};
use gpui::{
    AnyElement, ClickEvent, Context, ElementId, InteractiveElement, IntoElement, MouseButton,
    MouseUpEvent, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, div,
    point, prelude::FluentBuilder,
};

use super::Shell;
use super::tab_list::{DraggedTab, favicon};
use crate::context_menu::MenuEntry;
use crate::icons;
use crate::page;
use crate::theme::Theme;
use crate::tooltip::tooltip;

const EMPTY_HINT: &str = "Pin tabs to show them here";

impl Shell {
    /// Chrome-style bar under the title bar holding the space's pinned tabs
    /// and folders, when Settings → Favorites bar is on.
    pub(super) fn render_favorites_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.settings.favorites_bar {
            return None;
        }
        let drop_marker = self.palette(cx).accent;
        let tabs = &self.browser.space().tabs;
        let mut items: Vec<AnyElement> = Vec::new();
        for folder in tabs.folders() {
            items.push(self.render_favorite_folder(&folder, cx));
        }
        for tab in tabs
            .section(Section::Pinned)
            .filter(|tab| tab.folder().is_none())
        {
            items.push(self.render_favorite(tab, cx));
        }
        let theme = Theme::of(cx);
        if items.is_empty() {
            items.push(
                div()
                    .text_size(theme.metrics.text_sm)
                    .text_color(theme.colors.text_faint)
                    .child(EMPTY_HINT)
                    .into_any_element(),
            );
        }
        Some(
            div()
                .id("favorites-bar")
                .flex()
                .flex_row()
                .flex_shrink_0()
                .items_center()
                .gap(theme.metrics.radius_sm)
                .h(theme.metrics.favorites_bar_height - theme.metrics.radius_sm)
                .px(theme.metrics.frame_inset)
                .mb(theme.metrics.radius_sm)
                .overflow_x_scroll()
                .drag_over::<DraggedTab>(move |style, _, _, _| style.bg(drop_marker.opacity(0.12)))
                .on_drop(cx.listener(|this, dragged: &DraggedTab, _, cx| {
                    this.drop_tab_at_end(dragged.id, Section::Pinned, cx)
                }))
                .on_mouse_up(
                    MouseButton::Right,
                    cx.listener(|this, event: &MouseUpEvent, window, cx| {
                        this.open_empty_area_menu(event.position, window, cx)
                    }),
                )
                .children(items)
                .into_any_element(),
        )
    }

    fn render_favorite(&self, tab: &Tab, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx);
        let id = tab.id;
        let active = self.browser.active_tab_id() == Some(id);
        let title = page::display_title(tab);
        let image = self
            .runtime
            .get(&id)
            .and_then(|runtime| runtime.favicon.clone());
        chip(ElementId::Integer(id.value()), active, cx)
            .on_click(cx.listener(move |this, _, window, cx| this.activate(id, window, cx)))
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseUpEvent, window, cx| {
                    cx.stop_propagation();
                    this.open_tab_menu(id, event.position, window, cx)
                }),
            )
            .tooltip(tooltip(title.clone()))
            .child(favicon(image, cx))
            .child(
                div()
                    .min_w_0()
                    .max_w(theme.metrics.favorite_max_width)
                    .truncate()
                    .child(title),
            )
            .into_any_element()
    }

    fn render_favorite_folder(&self, name: &str, cx: &mut Context<Self>) -> AnyElement {
        let label = if self.renaming_folder.as_deref() == Some(name) {
            self.folder_rename_field(cx)
        } else {
            SharedString::from(name.to_string()).into_any_element()
        };
        let theme = Theme::of(cx);
        let open_name = name.to_string();
        let menu_name = name.to_string();
        chip(
            ElementId::Name(format!("favorite-folder-{name}").into()),
            false,
            cx,
        )
        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
            this.open_folder_dropdown(&open_name, event, window, cx)
        }))
        .on_mouse_up(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseUpEvent, window, cx| {
                cx.stop_propagation();
                this.open_folder_menu(menu_name.clone(), event.position, window, cx)
            }),
        )
        .child(
            div()
                .font_family(theme.fonts.icons.clone())
                .text_size(theme.metrics.icon_glyph)
                .child(icons::FOLDER),
        )
        .child(label)
        .child(
            div()
                .font_family(theme.fonts.icons.clone())
                .text_size(theme.metrics.icon_glyph)
                .text_color(theme.colors.text_muted)
                .child(icons::CHEVRON_DOWN),
        )
        .into_any_element()
    }

    fn open_folder_dropdown(
        &mut self,
        name: &str,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries: Vec<MenuEntry<TabId>> = self
            .browser
            .space()
            .tabs
            .section(Section::Pinned)
            .filter(|tab| tab.folder() == Some(name))
            .map(|tab| MenuEntry::item(page::display_title(tab), tab.id))
            .collect();
        let row = Theme::of(cx).metrics.row_height;
        let below = point(event.position().x, event.position().y + row / 2.);
        self.open_menu(entries, below, window, cx, |this, id, window, cx| {
            this.activate(id, window, cx)
        });
    }
}

fn chip(id: ElementId, active: bool, cx: &Context<Shell>) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::of(cx);
    let hover_bg = theme.colors.surface_hover;
    div()
        .id(id)
        .flex()
        .flex_row()
        .flex_shrink_0()
        .items_center()
        .gap(theme.metrics.radius_sm * 1.5)
        .h(theme.metrics.favorites_bar_height - theme.metrics.radius_sm * 1.5)
        .px(theme.metrics.radius_md)
        .rounded(theme.metrics.radius_md)
        .text_size(theme.metrics.text_sm)
        .text_color(theme.colors.text)
        .map(|chip| {
            if active {
                chip.bg(theme.colors.surface_active)
            } else {
                chip.hover(move |style| style.bg(hover_bg))
            }
        })
}
