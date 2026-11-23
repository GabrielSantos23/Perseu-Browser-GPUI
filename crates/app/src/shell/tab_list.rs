use std::sync::Arc;

use browser_core::tabs::{Section, Split, Tab, TabId};
use gpui::{
    Animation, AnimationExt as _, AnyElement, App, AppContext, ClickEvent, Context, DragMoveEvent,
    Image, InteractiveElement, IntoElement, MouseButton, MouseUpEvent, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, Window, div, img, prelude::FluentBuilder, px,
};

use super::Shell;
use super::row_motion::{TabDrag, slot_at};
use crate::actions;
use crate::icons;
use crate::page;
use crate::theme::Theme;
use crate::tooltip::{Side, tooltip, tooltip_on, tooltip_with_keys};

const ROW_GROUP: &str = "tab-row";
const NEW_TAB_LABEL: &str = "New tab";
const DRIFT_MARKER: &str = "/";
const SELECTED_STRENGTH: f32 = 0.22;
const DROP_TO_PIN: &str = "Drop to pin";
const LABEL_FADE: std::time::Duration = std::time::Duration::from_millis(160);

#[derive(Clone)]
pub(super) struct DraggedTab {
    pub(super) id: TabId,
    pub(super) title: SharedString,
    pub(super) favicon: Option<Arc<Image>>,
}

impl Render for DraggedTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(theme.metrics.radius_md)
            .h(theme.metrics.row_height)
            .w(theme.metrics.sidebar_min_width)
            .px(theme.metrics.radius_md)
            .rounded(theme.metrics.radius_md)
            .bg(theme.colors.surface_raised)
            .border_1()
            .border_color(theme.colors.border)
            .text_size(theme.metrics.text_md)
            .text_color(theme.colors.text)
            .font_family(theme.fonts.ui.clone())
            .font_weight(theme.fonts.ui_weight)
            .child(favicon(self.favicon.clone(), cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(self.title.clone()),
            )
    }
}

impl Shell {
    pub(super) fn render_tab_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        self.row_motion.borrow_mut().begin();
        let pinned = if self.settings.favorites_bar {
            Vec::new()
        } else {
            self.render_pinned(cx)
        };
        let unpinned = self.render_gap_list(Section::Unpinned, cx);
        self.row_motion.borrow_mut().end();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .children(pinned)
            .child(self.render_divider(cx))
            .child(self.render_new_tab_row(cx))
            .child(
                div()
                    .id("unpinned-tabs")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.unpinned_scroll)
                    .on_drag_move(
                        cx.listener(|this, event: &DragMoveEvent<DraggedTab>, _, cx| {
                            this.on_tab_drag_move(Section::Unpinned, event, cx)
                        }),
                    )
                    .on_drop(cx.listener(|this, dragged: &DraggedTab, _, cx| {
                        this.drop_into_gap(dragged.id, Section::Unpinned, cx)
                    }))
                    .children(unpinned)
                    .child(self.render_trailing_drop_zone(cx)),
            )
    }

    /// A key for one animated list of rows in the current space.
    pub(super) fn list_scope(&self, list: &str) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (self.browser.space().id.value(), list).hash(&mut hasher);
        hasher.finish()
    }

    /// Wraps a row so it slides to a new slot and fades in when it is new.
    pub(super) fn animated_row(
        &self,
        id: TabId,
        scope: u64,
        slot: f32,
        row: AnyElement,
    ) -> AnyElement {
        let frame = self.row_motion.borrow_mut().place(id, scope, slot);
        div()
            .relative()
            .flex_shrink_0()
            .top(px(frame.offset))
            .opacity(frame.opacity)
            .child(row)
            .into_any_element()
    }

    fn hides_dragged(&self, id: TabId) -> bool {
        self.tab_drag.is_some_and(|drag| drag.id == id)
            && self.browser.space().tabs.split_of(id).is_none()
    }

    /// The rows of a list that opens a gap under a dragged tab: unpinned
    /// tabs, or pinned tabs outside folders. One id per row (a split's first).
    pub(super) fn gap_entries(&self, section: Section) -> Vec<TabId> {
        let tabs = &self.browser.space().tabs;
        tabs.section(section)
            .filter(|tab| section == Section::Unpinned || tab.folder().is_none())
            .filter(|tab| !matches!(tabs.split_of(tab.id), Some(split) if split.second == tab.id))
            .map(|tab| tab.id)
            .filter(|id| !self.hides_dragged(*id))
            .collect()
    }

    pub(super) fn render_gap_list(
        &self,
        section: Section,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let row = f32::from(Theme::of(cx).metrics.row_height);
        let scope = self.list_scope(if section == Section::Pinned {
            "pinned"
        } else {
            "unpinned"
        });
        let entries = self.gap_entries(section);
        let gap = self
            .tab_drag
            .and_then(|drag| drag.over)
            .filter(|(over, _)| *over == section)
            .map(|(_, slot)| slot);
        let spacer = || div().flex_shrink_0().h(px(row)).into_any_element();
        let mut rows = Vec::new();
        let mut y = 0.0;
        for (index, id) in entries.iter().enumerate() {
            if gap == Some(index) {
                rows.push(spacer());
                y += row;
            }
            let Some(tab) = self.browser.space().tabs.get(*id) else {
                continue;
            };
            if let Some(element) = self.render_strip_entry(tab, false, cx) {
                rows.push(self.animated_row(*id, scope, y, element));
                y += row;
            }
        }
        if gap == Some(entries.len()) {
            rows.push(spacer());
        }
        rows
    }

    pub(super) fn on_tab_drag_move(
        &mut self,
        section: Section,
        event: &DragMoveEvent<DraggedTab>,
        cx: &mut Context<Self>,
    ) {
        let id = event.drag(cx).id;
        let drag = *self.tab_drag.get_or_insert(TabDrag {
            id,
            over: None,
            pointer_y: 0.0,
        });
        let position = event.event.position;
        let mut pointer_y = drag.pointer_y;
        let over = if event.bounds.contains(&position) {
            let mut top = event.bounds.top();
            if section == Section::Unpinned {
                top += self.unpinned_scroll.offset().y;
            }
            pointer_y = f32::from(position.y - top);
            let row = f32::from(Theme::of(cx).metrics.row_height);
            Some((
                section,
                slot_at(pointer_y, row, self.gap_entries(section).len()),
            ))
        } else if drag.over.is_some_and(|(over, _)| over == section) {
            None
        } else {
            return;
        };
        let changed = over != drag.over;
        self.tab_drag = Some(TabDrag {
            id,
            over,
            pointer_y,
        });
        if changed {
            cx.notify();
        }
    }

    /// Drops a dragged tab into the gap it opened, gliding it in from the
    /// pointer.
    pub(super) fn drop_into_gap(&mut self, id: TabId, section: Section, cx: &mut Context<Self>) {
        let drag = self.tab_drag.take();
        let slot = drag
            .and_then(|drag| drag.over)
            .filter(|(over, _)| *over == section)
            .map(|(_, slot)| slot);
        let entries: Vec<TabId> = self
            .gap_entries(section)
            .into_iter()
            .filter(|entry| *entry != id)
            .collect();
        match slot.and_then(|slot| entries.get(slot)) {
            Some(target) => self.drop_tab_before(id, *target, cx),
            None => self.drop_tab_at_end(id, section, cx),
        }
        if section == Section::Pinned {
            self.browser.space_mut().tabs.set_folder(id, None);
        }
        if let Some(drag) = drag {
            let row = f32::from(Theme::of(cx).metrics.row_height);
            self.row_motion
                .borrow_mut()
                .glide_from(id, drag.pointer_y - row / 2.);
        }
        cx.notify();
    }

    /// A tab's row, or its split's row; `None` for a split's second tab.
    /// `droppable` rows take a dropped tab before themselves; rows in a list
    /// that opens a gap leave dropping to the list.
    pub(super) fn render_strip_entry(
        &self,
        tab: &Tab,
        droppable: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        match self.browser.space().tabs.split_of(tab.id) {
            Some(split) if split.second == tab.id => None,
            Some(split) => Some(self.render_split_row(*split, droppable, cx)),
            None => Some(self.render_tab_row(tab, droppable, cx).into_any_element()),
        }
    }

    fn render_split_row(
        &self,
        split: Split,
        droppable: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tabs = &self.browser.space().tabs;
        let (Some(first), Some(second)) = (tabs.get(split.first), tabs.get(split.second)) else {
            return div().into_any_element();
        };
        let first = self.render_tab_row(first, droppable, cx).into_any_element();
        let second = self
            .render_tab_row(second, droppable, cx)
            .into_any_element();
        let theme = Theme::of(cx);
        let hover_bg = theme.colors.surface_hover;
        let half = |row: AnyElement| div().flex_1().min_w_0().child(row);

        div()
            .id(("split-row", split.first.value()))
            .flex()
            .flex_row()
            .flex_shrink_0()
            .items_center()
            .rounded(theme.metrics.radius_md)
            .border_1()
            .border_color(self.palette(cx).frame_border)
            .child(half(first))
            .child(
                div()
                    .w_px()
                    .h(theme.metrics.row_height / 2.)
                    .bg(theme.colors.border),
            )
            .child(half(second))
            .child(
                div()
                    .id(("unsplit", split.first.value()))
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .justify_center()
                    .size(theme.metrics.nav_button_size - theme.metrics.radius_md)
                    .mr(theme.metrics.radius_sm)
                    .rounded(theme.metrics.radius_sm)
                    .font_family(theme.fonts.icons.clone())
                    .text_size(theme.metrics.icon_glyph)
                    .text_color(theme.colors.text_muted)
                    .hover(move |style| style.bg(hover_bg))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.browser.space_mut().tabs.unsplit(split.first);
                        this.show_active_tab(window, cx);
                    }))
                    .tooltip(tooltip_on(Side::Left, "Separate tabs"))
                    .child(icons::UNSPLIT),
            )
            .into_any_element()
    }

    fn render_tab_row(
        &self,
        tab: &Tab,
        droppable: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::of(cx);
        let id = tab.id;
        let is_active = self.browser.active_tab_id() == Some(id);
        let runtime = self.runtime.get(&id);
        let runtime_favicon = runtime.and_then(|runtime| runtime.favicon.clone());
        let audio = runtime
            .filter(|runtime| runtime.playing_audio || runtime.muted)
            .map(|runtime| runtime.muted);
        let unloaded = tab.is_pinned() && runtime.is_none_or(|runtime| runtime.view.is_none());
        let drifted = tab.has_drifted();
        let dragged = DraggedTab {
            id,
            title: page::display_title(tab),
            favicon: runtime_favicon.clone(),
        };
        let hover_bg = theme.colors.surface_hover;
        let drop_marker = self.palette(cx).accent;
        let selected = self.split_selection.contains(&id);
        let muted = theme.colors.text_muted;
        let title_color = if unloaded {
            theme.colors.text_muted
        } else {
            theme.colors.text
        };

        div()
            .id(("tab", id.value()))
            .group(ROW_GROUP)
            .flex()
            .flex_row()
            .flex_shrink_0()
            .items_center()
            .gap(theme.metrics.radius_md)
            .h(theme.metrics.row_height)
            .pl(theme.metrics.radius_md)
            .pr(theme.metrics.radius_sm)
            .rounded(theme.metrics.radius_md)
            .border_t_2()
            .border_color(gpui::transparent_black())
            .map(|row| {
                if is_active {
                    row.bg(theme.colors.surface_active)
                } else if selected {
                    row.bg(drop_marker.opacity(SELECTED_STRENGTH))
                } else {
                    row.hover(move |style| style.bg(hover_bg))
                }
            })
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                if event.modifiers().control {
                    this.toggle_split_selection(id, cx);
                } else {
                    this.split_selection.clear();
                    this.activate(id, window, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(move |this, _, window, cx| this.close_tab(id, window, cx)),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseUpEvent, window, cx| {
                    this.open_tab_menu(id, event.position, window, cx)
                }),
            )
            .on_drag(dragged.clone(), |dragged, _, _, cx| {
                cx.new(|_| dragged.clone())
            })
            .when(droppable, |row| {
                row.drag_over::<DraggedTab>(move |style, _, _, _| style.border_color(drop_marker))
                    .on_drop(cx.listener(move |this, dragged: &DraggedTab, _, cx| {
                        this.drop_tab_before(dragged.id, id, cx)
                    }))
            })
            .child(self.render_row_favicon(id, runtime_favicon, drifted, cx))
            .when(drifted, |row| {
                row.child(div().text_color(muted).child(DRIFT_MARKER))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(title_color)
                    .child(dragged.title),
            )
            .children(audio.map(|muted| self.render_audio_button(id, muted, cx)))
            .child(self.render_close_button(id, cx))
    }

    fn render_row_favicon(
        &self,
        id: TabId,
        image: Option<Arc<Image>>,
        drifted: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let hover_bg = Theme::of(cx).colors.surface_active;
        div()
            .id(("tab-favicon", id.value()))
            .flex_shrink_0()
            .rounded(Theme::of(cx).metrics.radius_sm)
            .when(drifted, |icon| {
                icon.hover(move |style| style.bg(hover_bg))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.reset_pinned_tab(id, cx);
                    }))
                    .tooltip(tooltip("Back to pinned page"))
            })
            .child(favicon(image, cx))
    }

    fn render_audio_button(&self, id: TabId, muted: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx);
        let hover_bg = theme.colors.surface_hover;
        let (glyph, label) = if muted {
            (icons::MUTE, "Unmute tab")
        } else {
            (icons::VOLUME, "Mute tab")
        };
        div()
            .id(("tab-audio", id.value()))
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .size(theme.metrics.nav_button_size - theme.metrics.radius_md)
            .rounded(theme.metrics.radius_sm)
            .font_family(theme.fonts.icons.clone())
            .text_size(theme.metrics.icon_glyph)
            .text_color(theme.colors.text_muted)
            .hover(move |style| style.bg(hover_bg))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                if let Some(view) = this
                    .runtime
                    .get(&id)
                    .and_then(|runtime| runtime.view.clone())
                {
                    view.set_muted(!muted);
                }
            }))
            .tooltip(tooltip_on(Side::Left, label))
            .child(glyph)
            .into_any_element()
    }

    fn render_close_button(&self, id: TabId, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let hover_bg = theme.colors.surface_hover;
        let size = theme.metrics.nav_button_size - theme.metrics.radius_md;

        div()
            .id(("close-tab", id.value()))
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .size(size)
            .rounded(theme.metrics.radius_sm)
            .font_family(theme.fonts.icons.clone())
            .text_size(theme.metrics.icon_glyph)
            .text_color(theme.colors.text_muted)
            .opacity(0.)
            .group_hover(ROW_GROUP, |style| style.opacity(1.))
            .hover(move |style| style.bg(hover_bg))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.close_tab(id, window, cx);
            }))
            .tooltip(tooltip_with_keys(
                Side::Left,
                "Close tab",
                actions::CloseTab,
            ))
            .child(icons::CLOSE)
    }

    fn render_divider(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let drop_marker = self.palette(cx).accent;
        let tabs = &self.browser.space().tabs;
        let has_pinned = !self.settings.favorites_bar
            && (!tabs.folders().is_empty() || tabs.section(Section::Pinned).next().is_some());
        let label = self.tab_drag.is_some().then(|| {
            div()
                .flex_shrink_0()
                .text_size(theme.metrics.text_sm)
                .text_color(theme.colors.text_muted)
                .child(DROP_TO_PIN)
                .with_animation(
                    "drop-to-pin",
                    Animation::new(LABEL_FADE)
                        .with_easing(crate::motion::curve(crate::motion::Easing::EaseOut)),
                    |label, delta| label.opacity(delta),
                )
        });

        div()
            .id("pinned-divider")
            .flex_shrink_0()
            .py(theme.metrics.radius_sm)
            .px(theme.metrics.radius_md)
            .drag_over::<DraggedTab>(move |style, _, _, _| style.bg(drop_marker.opacity(0.15)))
            .on_drop(cx.listener(|this, dragged: &DraggedTab, _, cx| {
                this.drop_tab_at_end(dragged.id, Section::Pinned, cx)
            }))
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|this, event: &MouseUpEvent, window, cx| {
                    this.open_empty_area_menu(event.position, window, cx)
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .h(theme.metrics.text_sm * 1.7)
                    .gap(theme.metrics.radius_md)
                    .child(
                        div()
                            .flex_1()
                            .h_px()
                            .when(has_pinned, |line| line.bg(theme.colors.border)),
                    )
                    .children(label),
            )
    }

    fn render_new_tab_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let hover_bg = theme.colors.surface_hover;

        div()
            .id("new-tab-row")
            .flex()
            .flex_row()
            .flex_shrink_0()
            .items_center()
            .gap(theme.metrics.radius_md)
            .h(theme.metrics.row_height)
            .px(theme.metrics.radius_md)
            .rounded(theme.metrics.radius_md)
            .text_color(theme.colors.text_muted)
            .hover(move |style| style.bg(hover_bg))
            .on_click(cx.listener(|this, _, window, cx| this.new_tab(window, cx)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(theme.metrics.favicon_size)
                    .font_family(theme.fonts.icons.clone())
                    .text_size(theme.metrics.nav_glyph)
                    .child(icons::ADD),
            )
            .child(NEW_TAB_LABEL)
    }

    fn render_trailing_drop_zone(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let min_height = Theme::of(cx).metrics.row_height;

        div()
            .id("unpinned-drop-zone")
            .flex_1()
            .min_h(min_height)
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|this, event: &MouseUpEvent, window, cx| {
                    this.open_empty_area_menu(event.position, window, cx)
                }),
            )
    }
}

pub(super) fn favicon(image: Option<Arc<Image>>, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    let size = theme.metrics.favicon_size;
    match image {
        Some(image) => img(image).size(size).flex_shrink_0().into_any_element(),
        None => div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .size(size)
            .font_family(theme.fonts.icons.clone())
            .text_size(theme.metrics.nav_glyph)
            .text_color(theme.colors.text_muted)
            .child(icons::GLOBE)
            .into_any_element(),
    }
}
