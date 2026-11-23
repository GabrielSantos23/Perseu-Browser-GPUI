use browser_core::tabs::{Split, SplitOrientation, TabId};
use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Styled, Window, div,
    prelude::FluentBuilder, relative,
};

use super::Shell;
use super::tab_ops::now;
use crate::theme::Theme;

const MAX_SELECTION: usize = 2;

impl Shell {
    pub(super) fn active_split(&self) -> Option<Split> {
        let active = self.browser.active_tab_id()?;
        self.browser.space().tabs.split_of(active).copied()
    }

    pub(super) fn active_partner(&self) -> Option<TabId> {
        let active = self.browser.active_tab_id()?;
        self.active_split()?.partner_of(active)
    }

    pub(super) fn toggle_split_selection(&mut self, id: TabId, cx: &mut Context<Self>) {
        if let Some(index) = self
            .split_selection
            .iter()
            .position(|selected| *selected == id)
        {
            self.split_selection.remove(index);
        } else {
            if self.split_selection.len() == MAX_SELECTION {
                self.split_selection.remove(0);
            }
            self.split_selection.push(id);
        }
        cx.notify();
    }

    pub(super) fn can_split(&self) -> bool {
        self.active_split().is_none() && self.split_pair().is_some()
    }

    fn split_pair(&self) -> Option<(TabId, TabId)> {
        let tabs = &self.browser.space().tabs;
        let active = self
            .browser
            .active_tab_id()
            .filter(|id| tabs.get(*id).is_some());
        let unsplit = |id: &TabId| tabs.get(*id).is_some() && tabs.split_of(*id).is_none();
        let selected: Vec<TabId> = self
            .split_selection
            .iter()
            .copied()
            .filter(unsplit)
            .collect();
        match (selected.as_slice(), active) {
            ([first, second], _) => Some((*first, *second)),
            ([other], Some(active)) if *other != active && unsplit(&active) => {
                Some((active, *other))
            }
            (_, Some(active)) if unsplit(&active) => tabs
                .tabs()
                .iter()
                .filter(|tab| tab.id != active && unsplit(&tab.id))
                .max_by_key(|tab| tab.last_active)
                .map(|tab| (active, tab.id)),
            _ => None,
        }
    }

    pub(super) fn split_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((first, second)) = self.split_pair() else {
            return;
        };
        if self.browser.space_mut().tabs.split(first, second) {
            self.split_selection.clear();
            self.browser.select(first, now());
            self.show_active_tab(window, cx);
        }
    }

    pub(super) fn unsplit_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(active) = self.browser.active_tab_id()
            && self.browser.space_mut().tabs.unsplit(active).is_some()
        {
            self.show_active_tab(window, cx);
        }
    }

    pub(super) fn rotate_active_split(&mut self, cx: &mut Context<Self>) {
        if let Some(active) = self.browser.active_tab_id()
            && self
                .browser
                .space_mut()
                .tabs
                .toggle_split_orientation(active)
        {
            cx.notify();
        }
    }

    pub(super) fn start_split_resize(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.resizing_split = true;
        cx.stop_propagation();
    }

    pub(super) fn drag_split_divider(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (true, Some(split)) = (self.resizing_split, self.active_split()) else {
            return;
        };
        let content = self.content_bounds.get();
        let (offset, length) = match split.orientation {
            SplitOrientation::Horizontal => {
                (event.position.x - content.origin.x, content.size.width)
            }
            SplitOrientation::Vertical => {
                (event.position.y - content.origin.y, content.size.height)
            }
        };
        if length <= Pixels::ZERO {
            return;
        }
        if self
            .browser
            .space_mut()
            .tabs
            .set_split_ratio(split.first, offset / length)
        {
            cx.notify();
        }
    }

    pub(super) fn finish_split_resize(
        &mut self,
        _: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.resizing_split {
            self.resizing_split = false;
            cx.notify();
        }
    }

    pub(super) fn render_split(&self, split: Split, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx);
        let palette = self.palette(cx);
        let horizontal = split.orientation == SplitOrientation::Horizontal;
        let active = self.browser.active_tab_id();
        let cutout = self.overlay_cutout();
        let pane = |id: TabId, cutout: f32| {
            let view = self
                .runtime
                .get(&id)
                .and_then(|runtime| runtime.view.clone());
            let border = if Some(id) == active {
                palette.accent
            } else {
                palette.frame_border
            };
            self.render_pane(view, cutout, border, cx)
        };
        let first = pane(split.first, cutout)
            .flex_basis(relative(split.ratio))
            .flex_shrink_0();
        let second = pane(split.second, if horizontal { 0.0 } else { cutout }).flex_1();
        let gap = theme.metrics.frame_inset;
        let divider = div()
            .id("split-divider")
            .flex_shrink_0()
            .map(|divider| {
                if horizontal {
                    divider.w(gap).h_full().cursor_col_resize()
                } else {
                    divider.h(gap).w_full().cursor_row_resize()
                }
            })
            .on_mouse_down(MouseButton::Left, cx.listener(Self::start_split_resize));

        div()
            .size_full()
            .flex()
            .when(horizontal, |panes| panes.flex_row())
            .when(!horizontal, |panes| panes.flex_col())
            .child(first)
            .child(divider)
            .child(second)
            .into_any_element()
    }
}
