//! The History page: every recorded page by the day it was last visited,
//! searchable, with a range to show and clear.

use std::collections::HashMap;
use std::sync::Arc;

use browser_core::history::HistoryEntry;
use browser_core::tabs::OpenPosition;
use gpui::{
    AnyElement, App, AppContext, ClickEvent, Context, Entity, FocusHandle, FontWeight, Image,
    InteractiveElement, IntoElement, KeyBinding, ParentElement, Pixels, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, actions, div, prelude::FluentBuilder,
    px,
};
use gpui_kit::component::Sizable;
use gpui_kit::component::input::{Input, InputEvent, InputState};

use super::Shell;
use super::tab_list::favicon;
use super::tab_ops::now;
use crate::context_menu::MenuEntry;
use crate::icons;
use crate::local_time::{self, LocalTime};
use crate::theme::Theme;
use crate::tooltip::tooltip;

actions!(history_page, [CloseHistory]);

const KEY_CONTEXT: &str = "History";
const TITLE: &str = "History";
const SEARCH: &str = "Search history";
const BACK: &str = "Back to browsing";
const FORGET: &str = "Remove from history";
const COLUMN: f32 = 820.;
const GUTTER: f32 = 32.;
const TOOLBAR: f32 = 44.;
const DAY_ROW: f32 = 38.;
const VISIT_ROW: f32 = 44.;
const HOST_MAX: f32 = 180.;
const TIME_COLUMN: f32 = 62.;
const FORGET_SIZE: f32 = 22.;
const LABEL: f32 = 12.;
const BODY: f32 = 13.;
const PAGE_STEP: usize = 300;
const MS_PER_HOUR: u64 = 3_600_000;
const MS_PER_WEEK: u64 = 7 * 24 * MS_PER_HOUR;

pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", CloseHistory, Some(KEY_CONTEXT))]);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Range {
    Everything,
    Hour,
    Day,
    Week,
}

impl Range {
    const ALL: [Range; 4] = [Range::Everything, Range::Hour, Range::Day, Range::Week];

    fn label(self) -> &'static str {
        match self {
            Range::Everything => "All history",
            Range::Hour => "Last hour",
            Range::Day => "Today",
            Range::Week => "Last 7 days",
        }
    }

    fn confirm(self) -> &'static str {
        match self {
            Range::Everything => "Clear all history?",
            Range::Hour => "Clear the last hour?",
            Range::Day => "Clear today?",
            Range::Week => "Clear 7 days?",
        }
    }

    /// The earliest visit the range covers; `0` for all of it.
    fn since(self, now: u64) -> u64 {
        match self {
            Range::Everything => 0,
            Range::Hour => now.saturating_sub(MS_PER_HOUR),
            Range::Day => local_time::start_of_today(now),
            Range::Week => now.saturating_sub(MS_PER_WEEK),
        }
    }
}

pub(super) struct HistoryPage {
    focus_handle: FocusHandle,
    search: Entity<InputState>,
    query: String,
    range: Range,
    /// Clearing cannot be undone, so the button first says what it removes.
    armed: bool,
    hovered: Option<usize>,
    shown: usize,
    _subscriptions: Vec<Subscription>,
}

/// A day heading or a page under it.
enum Row<'a> {
    Day { label: String, count: usize },
    Visit(&'a HistoryEntry, LocalTime),
}

impl Shell {
    pub(super) fn toggle_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history_page.is_some() {
            self.close_history(window, cx);
        } else {
            self.open_history(window, cx);
        }
    }

    pub(super) fn open_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(page) = &self.history_page {
            window.focus(&page.focus_handle, cx);
            return;
        }
        self.close_settings(window, cx);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(SEARCH));
        let subscription = cx.subscribe_in(
            &search,
            window,
            |shell, input, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change)
                    && let Some(page) = &mut shell.history_page
                {
                    page.query = input.read(cx).value().to_string();
                    page.shown = PAGE_STEP;
                    page.armed = false;
                    cx.notify();
                }
            },
        );
        let page = HistoryPage {
            focus_handle: cx.focus_handle(),
            search,
            query: String::new(),
            range: Range::Everything,
            armed: false,
            hovered: None,
            shown: PAGE_STEP,
            _subscriptions: vec![subscription],
        };
        self.take_keyboard_from_pages();
        page.search.update(cx, |input, cx| input.focus(window, cx));
        self.history_page = Some(page);
        self.show_only_active_view();
        cx.notify();
    }

    pub(super) fn close_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history_page.take().is_none() {
            return;
        }
        self.show_only_active_view();
        window.focus(&self.focus_handle, cx);
        if let Some(view) = self.active_view() {
            view.focus();
        }
        cx.notify();
    }

    fn open_from_history(
        &mut self,
        url: String,
        new_tab: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_history(window, cx);
        if new_tab || self.browser.active_tab_id().is_none() {
            self.open_tab(url, OpenPosition::End, window, cx);
        } else {
            self.navigate_active(url, window, cx);
        }
    }

    fn clear_history_range(&mut self, cx: &mut Context<Self>) {
        let Some(page) = &mut self.history_page else {
            return;
        };
        if !page.armed {
            page.armed = true;
            cx.notify();
            return;
        }
        page.armed = false;
        let since = page.range.since(now());
        super::persist::log_failure(self.history.clear_since(since));
        cx.notify();
    }

    fn open_range_menu(&mut self, event: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(page) = &self.history_page else {
            return;
        };
        let current = page.range;
        let entries = Range::ALL
            .into_iter()
            .map(|range| MenuEntry::item(range.label(), range).with_check(range == current))
            .collect();
        self.open_menu(
            entries,
            event.position(),
            window,
            cx,
            |shell, range, _, cx| {
                if let Some(page) = &mut shell.history_page {
                    page.range = range;
                    page.armed = false;
                    page.shown = PAGE_STEP;
                }
                cx.notify();
            },
        );
    }

    /// Favicons of open tabs, by host, for the rows that share one.
    fn favicons_by_host(&self) -> HashMap<String, Arc<Image>> {
        self.runtime
            .iter()
            .filter_map(|(id, runtime)| {
                let image = runtime.favicon.clone()?;
                let tab = self.browser.tab(*id)?;
                Some((host_of(&tab.url).to_string(), image))
            })
            .collect()
    }

    pub(super) fn render_history(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(page) = &self.history_page else {
            return div().into_any_element();
        };
        let theme = Theme::of(cx);
        let metrics = &theme.metrics;
        let now = now();
        let since = page.range.since(now);
        let query = page.query.trim().to_lowercase();
        let matches: Vec<&HistoryEntry> = self
            .history
            .entries()
            .iter()
            .filter(|entry| entry.last_visit >= since)
            .filter(|entry| {
                query.is_empty()
                    || entry.title.to_lowercase().contains(&query)
                    || entry.url.to_lowercase().contains(&query)
            })
            .collect();
        let empty = matches.is_empty();
        let more = matches.len() > page.shown;
        let rows = day_rows(&matches[..matches.len().min(page.shown)], now);
        let icons_by_host = self.favicons_by_host();
        let hover_bg = theme.colors.surface_hover;
        let danger = theme.colors.danger;

        let clear_label = if page.armed {
            page.range.confirm()
        } else {
            "Clear"
        };
        let bar = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .pt(px(4.))
            .pb(px(14.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&page.search).small().aria_label(SEARCH)),
            )
            .child(
                dropdown("history-range", page.range.label(), cx).on_click(cx.listener(
                    |this, event: &ClickEvent, window, cx| this.open_range_menu(event, window, cx),
                )),
            )
            .child(
                div()
                    .id("history-clear")
                    .flex()
                    .items_center()
                    .h(metrics.nav_button_size)
                    .px(metrics.radius_md * 1.5)
                    .rounded(metrics.radius_md)
                    .border_1()
                    .when(empty, |button| button.opacity(0.45))
                    .map(|button| {
                        if page.armed {
                            button
                                .border_color(danger)
                                .bg(danger.opacity(0.16))
                                .text_color(danger)
                        } else {
                            button
                                .border_color(theme.colors.border)
                                .hover(move |style| style.bg(hover_bg))
                        }
                    })
                    .when(!empty, |button| {
                        button.on_click(cx.listener(|this, _, _, cx| this.clear_history_range(cx)))
                    })
                    .child(clear_label),
            );

        let body: AnyElement = if empty {
            let (title, help) = if query.is_empty() {
                (
                    "Your browsing history belongs here",
                    "Pages you visit are listed here so you can find your way back.",
                )
            } else {
                (
                    "No matching pages",
                    "Try a different word, or part of an address.",
                )
            };
            empty_state(icons::HISTORY, title, help, cx)
        } else {
            let list: Vec<AnyElement> = rows
                .iter()
                .enumerate()
                .map(|(index, row)| match row {
                    Row::Day { label, count } => div()
                        .flex()
                        .items_end()
                        .gap(px(8.))
                        .h(px(DAY_ROW))
                        .px(px(10.))
                        .pb(px(6.))
                        .text_size(px(LABEL))
                        .text_color(theme.colors.text_muted)
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(label.clone()))
                        .child(count.to_string())
                        .into_any_element(),
                    Row::Visit(entry, at) => {
                        let lit = page.hovered == Some(index);
                        let url = entry.url.clone();
                        let forget_url = entry.url.clone();
                        let host = host_of(&entry.url).to_string();
                        let title: SharedString = if entry.title.is_empty() {
                            entry.url.clone().into()
                        } else {
                            entry.title.clone().into()
                        };
                        div()
                            .id(("history-visit", index))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .h(px(VISIT_ROW))
                            .px(px(10.))
                            .rounded(metrics.radius_md)
                            .when(lit, |row| row.bg(theme.colors.surface_active))
                            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                if let Some(page) = &mut this.history_page {
                                    let next = if *hovered { Some(index) } else { None };
                                    if *hovered || page.hovered == Some(index) {
                                        page.hovered = next;
                                        cx.notify();
                                    }
                                }
                            }))
                            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                                let new_tab = event.modifiers().control;
                                this.open_from_history(url.clone(), new_tab, window, cx)
                            }))
                            .child(favicon(icons_by_host.get(&host).cloned(), cx))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(BODY))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .max_w(px(HOST_MAX))
                                    .truncate()
                                    .text_size(px(LABEL))
                                    .text_color(theme.colors.text_muted)
                                    .child(host),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .w(px(TIME_COLUMN))
                                    .flex()
                                    .justify_end()
                                    .text_size(px(LABEL))
                                    .text_color(theme.colors.text_muted)
                                    .child(at.time_label()),
                            )
                            .child(
                                div()
                                    .id(("history-forget", index))
                                    .flex()
                                    .flex_shrink_0()
                                    .items_center()
                                    .justify_center()
                                    .size(px(FORGET_SIZE))
                                    .rounded(metrics.radius_sm)
                                    .text_size(metrics.nav_glyph)
                                    .text_color(theme.colors.text_muted)
                                    .when(!lit, |button| button.invisible())
                                    .hover(move |style| style.bg(hover_bg))
                                    .tooltip(tooltip(FORGET))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        super::persist::log_failure(
                                            this.history.forget(&forget_url),
                                        );
                                        cx.notify();
                                    }))
                                    .child(icons::TRASH),
                            )
                            .into_any_element()
                    }
                })
                .collect();
            div()
                .flex()
                .flex_col()
                .pb(px(24.))
                .children(list)
                .when(more, |list| {
                    list.child(
                        div()
                            .id("history-more")
                            .mt(px(12.))
                            .p(px(10.))
                            .rounded(metrics.radius_md)
                            .flex()
                            .justify_center()
                            .text_size(px(LABEL))
                            .text_color(theme.colors.text_muted)
                            .hover(move |style| style.bg(hover_bg))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(page) = &mut this.history_page {
                                    page.shown += PAGE_STEP;
                                }
                                cx.notify();
                            }))
                            .child("Show more"),
                    )
                })
                .into_any_element()
        };

        div()
            .id("history-page")
            .track_focus(&page.focus_handle)
            .key_context(KEY_CONTEXT)
            .on_action(
                cx.listener(|this, _: &CloseHistory, window, cx| this.close_history(window, cx)),
            )
            .size_full()
            .flex()
            .flex_col()
            .rounded(metrics.radius_frame)
            .bg(theme.colors.surface)
            .border_1()
            .border_color(self.palette(cx).frame_border)
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(8.))
                    .h(px(TOOLBAR))
                    .px(px(10.))
                    .text_size(px(LABEL))
                    .child(
                        icon_button("history-back", icons::ARROW_BACK, cx)
                            .tooltip(tooltip(BACK))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.close_history(window, cx)),
                            ),
                    )
                    .child(TITLE),
            )
            .child(
                div()
                    .id("history-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(GUTTER))
                    .child(
                        div()
                            .w_full()
                            .max_w(px(COLUMN))
                            .mx_auto()
                            .child(bar)
                            .child(body),
                    ),
            )
            .into_any_element()
    }
}

/// Groups visits, newest first, under a heading for each local day.
fn day_rows<'a>(visits: &[&'a HistoryEntry], now: u64) -> Vec<Row<'a>> {
    let mut rows = Vec::new();
    let mut open_day = None;
    let mut heading = 0;
    for entry in visits {
        let Some(at) = LocalTime::at(entry.last_visit) else {
            continue;
        };
        if open_day != Some(at.day_key()) {
            open_day = Some(at.day_key());
            heading = rows.len();
            rows.push(Row::Day {
                label: local_time::day_label(at, now),
                count: 0,
            });
        }
        if let Some(Row::Day { count, .. }) = rows.get_mut(heading) {
            *count += 1;
        }
        rows.push(Row::Visit(entry, at));
    }
    rows
}

fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    host.strip_prefix("www.").unwrap_or(host)
}

/// A button that shows its current choice and opens a menu of the others.
pub(super) fn dropdown(
    id: &'static str,
    label: impl Into<SharedString>,
    cx: &Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let label = label.into();
    let theme = Theme::of(cx);
    let metrics = &theme.metrics;
    let hover_bg = theme.colors.surface_hover;
    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(metrics.radius_md)
        .h(metrics.nav_button_size)
        .pl(metrics.radius_md * 1.5)
        .pr(metrics.radius_md)
        .rounded(metrics.radius_md)
        .border_1()
        .border_color(theme.colors.border)
        .hover(move |style| style.bg(hover_bg))
        .child(label)
        .child(
            div()
                .text_size(metrics.icon_glyph)
                .text_color(theme.colors.text_muted)
                .child(icons::CHEVRON_DOWN),
        )
}

fn icon_button(
    id: &'static str,
    icon: icons::Icon,
    cx: &Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::of(cx);
    let hover_bg = theme.colors.surface_hover;
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(theme.metrics.nav_button_size)
        .rounded(theme.metrics.radius_md)
        .text_size(theme.metrics.nav_glyph)
        .text_color(theme.colors.text_muted)
        .hover(move |style| style.bg(hover_bg))
        .child(icon)
}

/// A quiet notice where a list would be.
pub(super) fn empty_state(
    icon: icons::Icon,
    title: &'static str,
    help: &'static str,
    cx: &Context<Shell>,
) -> AnyElement {
    let theme = Theme::of(cx);
    let glyph: Pixels = px(28.);
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(6.))
        .pt(px(72.))
        .text_center()
        .child(
            div()
                .mb(px(8.))
                .text_size(glyph)
                .text_color(theme.colors.text_faint)
                .child(icon),
        )
        .child(
            div()
                .text_size(px(BODY + 1.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(
            div()
                .text_size(px(LABEL))
                .text_color(theme.colors.text_muted)
                .child(help),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_drop_the_scheme_path_and_www() {
        assert_eq!(host_of("https://www.example.com/a?b"), "example.com");
        assert_eq!(host_of("http://docs.rs#x"), "docs.rs");
        assert_eq!(host_of("file:///C:/x.html"), "");
    }

    #[test]
    fn visits_are_headed_by_their_day() {
        let now = 1_795_000_000_000;
        let visit = |at| HistoryEntry {
            url: "https://a.com/".into(),
            title: String::new(),
            visit_count: 1,
            last_visit: at,
        };
        let entries = [visit(now), visit(now - 1), visit(now - 30 * 86_400_000)];
        let refs: Vec<&HistoryEntry> = entries.iter().collect();
        let rows = day_rows(&refs, now);
        let counts: Vec<usize> = rows
            .iter()
            .filter_map(|row| match row {
                Row::Day { count, .. } => Some(*count),
                Row::Visit(..) => None,
            })
            .collect();
        assert_eq!(counts, [2, 1]);
        assert_eq!(rows.len(), 5);
    }
}
