use std::time::Duration;

use browser_core::browser::ArchivedTab;
use browser_core::settings::Settings;
use browser_core::tabs::Timestamp;
use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder,
};

use super::Shell;
use super::tab_ops::now;
use crate::icons;
use crate::page;
use crate::theme::Theme;

const ARCHIVE_AFTER_OVERRIDE_VAR: &str = "PERSEU_ARCHIVE_AFTER_SECS";
const MIN_CHECK_INTERVAL: Duration = Duration::from_secs(5);
const MAX_CHECK_INTERVAL: Duration = Duration::from_secs(60);
const PANEL_TITLE: &str = "Archived";
const EMPTY_ARCHIVE: &str = "Idle tabs land here after a while";
const MS_PER_MINUTE: u64 = 60_000;
const MINUTES_PER_HOUR: u64 = 60;
const HOURS_PER_DAY: u64 = 24;

pub(super) fn archive_after_ms(settings: &Settings) -> u64 {
    std::env::var(ARCHIVE_AFTER_OVERRIDE_VAR)
        .ok()
        .and_then(|secs| secs.trim().parse::<u64>().ok())
        .map_or_else(
            || settings.archive_after_ms(),
            |secs| secs.saturating_mul(1000),
        )
}

fn check_interval(archive_after_ms: u64) -> Duration {
    Duration::from_millis(archive_after_ms / 4).clamp(MIN_CHECK_INTERVAL, MAX_CHECK_INTERVAL)
}

fn age_label(archived_at: Timestamp, now: Timestamp) -> String {
    let minutes = now.saturating_sub(archived_at) / MS_PER_MINUTE;
    let hours = minutes / MINUTES_PER_HOUR;
    let days = hours / HOURS_PER_DAY;
    match (days, hours, minutes) {
        (0, 0, 0) => "now".to_string(),
        (0, 0, m) => format!("{m}m"),
        (0, h, _) => format!("{h}h"),
        (d, _, _) => format!("{d}d"),
    }
}

impl Shell {
    pub(super) fn start_archive_timer(&self, window: &mut Window, cx: &mut Context<Self>) {
        let interval = check_interval(self.browser.archive_after_ms);
        cx.spawn_in(window, async move |shell, cx| {
            loop {
                cx.background_executor().timer(interval).await;
                if shell
                    .update(cx, |shell, cx| shell.archive_idle_tabs(cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    fn archive_idle_tabs(&mut self, cx: &mut Context<Self>) {
        let archived = self.browser.archive_idle(now());
        if archived.is_empty() {
            return;
        }
        for id in archived {
            self.runtime.remove(&id);
        }
        cx.notify();
    }

    fn restore_archived(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.browser.restore_archived(index, now()).is_some() {
            self.archive_open = false;
            self.show_active_tab(window, cx);
        }
    }

    pub(super) fn render_archive_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = now();
        let rows: Vec<AnyElement> = self
            .browser
            .archive()
            .iter()
            .enumerate()
            .map(|(index, entry)| archived_row(index, entry, current, cx).into_any_element())
            .collect();
        let empty = rows.is_empty();
        let theme = Theme::of(cx);
        let hover_bg = theme.colors.surface_hover;

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .id("archive-back")
                    .flex()
                    .flex_row()
                    .flex_shrink_0()
                    .items_center()
                    .gap(theme.metrics.radius_md)
                    .h(theme.metrics.row_height)
                    .px(theme.metrics.radius_md)
                    .rounded(theme.metrics.radius_md)
                    .text_size(theme.metrics.text_sm)
                    .text_color(theme.colors.text_muted)
                    .hover(move |style| style.bg(hover_bg))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.archive_open = false;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .font_family(theme.fonts.icons.clone())
                            .text_size(theme.metrics.nav_glyph)
                            .child(icons::BACK),
                    )
                    .child(PANEL_TITLE),
            )
            .child(
                div()
                    .id("archived-tabs")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows)
                    .when(empty, |list| {
                        list.child(
                            div()
                                .p(theme.metrics.radius_md)
                                .text_size(theme.metrics.text_sm)
                                .text_color(theme.colors.text_muted)
                                .child(EMPTY_ARCHIVE),
                        )
                    }),
            )
    }
}

fn archived_row(
    index: usize,
    entry: &ArchivedTab,
    now: Timestamp,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    let hover_bg = theme.colors.surface_hover;
    let title = if entry.title.trim().is_empty() {
        page::host(&entry.url)
    } else {
        entry.title.clone().into()
    };

    div()
        .id(("archived", index))
        .flex()
        .flex_col()
        .flex_shrink_0()
        .justify_center()
        .py(theme.metrics.radius_sm)
        .px(theme.metrics.radius_md)
        .rounded(theme.metrics.radius_md)
        .hover(move |style| style.bg(hover_bg))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.in_main_window(cx, move |this, window, cx| {
                this.restore_archived(index, window, cx)
            })
        }))
        .child(div().truncate().child(title))
        .child(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .gap(theme.metrics.radius_md)
                .text_size(theme.metrics.text_sm)
                .text_color(theme.colors.text_muted)
                .child(div().min_w_0().truncate().child(page::host(&entry.url)))
                .child(
                    div()
                        .flex_shrink_0()
                        .child(age_label(entry.archived_at, now)),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn age_labels_use_the_largest_unit() {
        assert_eq!(age_label(0, 30_000), "now");
        assert_eq!(age_label(0, 5 * MS_PER_MINUTE), "5m");
        assert_eq!(age_label(0, 3 * 60 * MS_PER_MINUTE), "3h");
        assert_eq!(age_label(0, 50 * 60 * MS_PER_MINUTE), "2d");
        assert_eq!(age_label(10, 0), "now");
    }

    #[test]
    fn check_interval_is_clamped() {
        assert_eq!(check_interval(1_000), MIN_CHECK_INTERVAL);
        assert_eq!(
            check_interval(browser_core::browser::DEFAULT_ARCHIVE_AFTER_MS),
            MAX_CHECK_INTERVAL
        );
    }
}
