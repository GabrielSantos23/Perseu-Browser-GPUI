use browser_core::downloads::{Download, DownloadState, format_bytes};
use browser_core::security::is_dangerous_file;
use browser_core::tabs::TabId;
use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, relative,
};
use webview_host::{DownloadOutcome, WebViewEvent};

use super::Shell;
use super::tab_ops::now;
use crate::icons;
use crate::native;
use crate::page;
use crate::theme::Theme;
use crate::tooltip::{Side, tooltip_on};

const PANEL_TITLE: &str = "Downloads";
const CLEAR_LABEL: &str = "Clear";
const EMPTY_DOWNLOADS: &str = "Files you download show up here";
const PROGRESS_HEIGHT_SHARE: f32 = 0.5;
const DANGER_NOTE: &str = "This type of file can run programs on your computer.";
const OPEN_ANYWAY: &str = "Open anyway";

fn confirm_button(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    danger: bool,
    cx: &Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::of(cx);
    let hover_bg = theme.colors.surface_hover;
    div()
        .id(id)
        .flex()
        .items_center()
        .h(theme.metrics.nav_button_size - theme.metrics.radius_sm)
        .px(theme.metrics.radius_md)
        .rounded(theme.metrics.radius_md)
        .border_1()
        .border_color(if danger {
            theme.colors.danger
        } else {
            theme.colors.border
        })
        .text_size(theme.metrics.text_sm)
        .text_color(if danger {
            theme.colors.danger
        } else {
            theme.colors.text
        })
        .hover(move |style| style.bg(hover_bg))
        .child(label)
}

fn status_line(download: &Download) -> String {
    let source = page::host(&download.url);
    let state = state_text(download);
    if source.is_empty() {
        state
    } else {
        format!("{state} · {source}")
    }
}

fn state_text(download: &Download) -> String {
    match download.state {
        DownloadState::InProgress => match download.total {
            Some(total) => format!(
                "{} of {}",
                format_bytes(download.received),
                format_bytes(total)
            ),
            None => format_bytes(download.received),
        },
        DownloadState::Completed => format_bytes(download.received),
        DownloadState::Cancelled => "Cancelled".to_string(),
        DownloadState::Failed => match download.reason {
            Some(reason) => format!("Failed · {reason}"),
            None => "Failed".to_string(),
        },
    }
}

impl Shell {
    pub(super) fn on_download_event(&mut self, event: WebViewEvent, cx: &mut Context<Self>) {
        match event {
            WebViewEvent::DownloadStarted {
                id,
                url,
                path,
                total,
            } => {
                self.downloads.start(id, url, path, total, now());
            }
            WebViewEvent::DownloadProgress {
                id,
                received,
                total,
            } => self.downloads.progress(id, received, total),
            WebViewEvent::DownloadFinished { id, outcome } => {
                let (state, reason) = match outcome {
                    DownloadOutcome::Completed => (DownloadState::Completed, None),
                    DownloadOutcome::Cancelled => (DownloadState::Cancelled, None),
                    DownloadOutcome::Failed(reason) => (DownloadState::Failed, Some(reason)),
                };
                self.downloads.finish(id, state, reason)
            }
            _ => return,
        }
        cx.notify();
    }

    // A tab that only led to a download leaves the list, but its webview must
    // outlive the download: closing it stalls the transfer.
    pub(super) fn hide_download_tab(
        &mut self,
        tab: TabId,
        download: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.browser.space().tabs.get(tab).is_none() {
            return;
        }
        let Some(view) = self.runtime.remove(&tab).and_then(|runtime| runtime.view) else {
            return;
        };
        view.set_visible(false);
        self.download_hosts.insert(download, view);
        self.browser.space_mut().tabs.remove(tab, now());
        if self.browser.active_tab_id().is_some() {
            self.show_active_tab(window, cx);
        } else {
            self.open_blank_tab(window, cx);
        }
    }

    pub(super) fn render_downloads_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let rows: Vec<AnyElement> = self
            .downloads
            .items()
            .iter()
            .map(|download| self.render_download_row(download, cx))
            .collect();
        let empty = rows.is_empty();
        let has_finished = self.downloads.items().len() > self.downloads.active_count();
        let theme = Theme::of(cx);
        let hover_bg = theme.colors.surface_hover;

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_shrink_0()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .id("downloads-back")
                            .flex()
                            .flex_row()
                            .flex_1()
                            .items_center()
                            .gap(theme.metrics.radius_md)
                            .h(theme.metrics.row_height)
                            .px(theme.metrics.radius_md)
                            .rounded(theme.metrics.radius_md)
                            .text_size(theme.metrics.text_sm)
                            .text_color(theme.colors.text_muted)
                            .hover(move |style| style.bg(hover_bg))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.downloads_open = false;
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
                    .when(has_finished, |header| {
                        header.child(
                            div()
                                .id("downloads-clear")
                                .flex()
                                .items_center()
                                .h(theme.metrics.row_height)
                                .px(theme.metrics.radius_md)
                                .rounded(theme.metrics.radius_md)
                                .text_size(theme.metrics.text_sm)
                                .text_color(theme.colors.text_muted)
                                .hover(move |style| style.bg(hover_bg))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.downloads.clear_finished();
                                    cx.notify();
                                }))
                                .child(CLEAR_LABEL),
                        )
                    }),
            )
            .child(
                div()
                    .id("download-list")
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
                                .child(EMPTY_DOWNLOADS),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_download_row(&self, download: &Download, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx);
        let hover_bg = theme.colors.surface_hover;
        let accent = self.palette(cx).accent;
        let id = download.id;
        let state = download.state;
        let path = download.path.clone();
        let dangerous = is_dangerous_file(&download.path);
        let confirming = self.download_confirm == Some(id);
        let status_color = match state {
            DownloadState::Failed => theme.colors.danger,
            DownloadState::Completed if dangerous => theme.colors.warning,
            _ => theme.colors.text_muted,
        };
        let icon_button = |key: &'static str, glyph: icons::Icon| {
            div()
                .id((key, id as usize))
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
                .child(glyph)
        };
        let mut actions: Vec<AnyElement> = Vec::new();
        if state == DownloadState::Completed {
            let folder_path = path.clone();
            actions.push(
                icon_button("download-folder", icons::FOLDER)
                    .tooltip(tooltip_on(Side::Left, "Show in folder"))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        native::show_in_folder(&folder_path);
                    })
                    .into_any_element(),
            );
        }
        actions.push(
            icon_button("download-dismiss", icons::CLOSE)
                .tooltip(tooltip_on(Side::Left, "Remove from list"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    if state == DownloadState::InProgress {
                        webview_host::cancel_download(id);
                    } else {
                        this.downloads.remove(id);
                        cx.notify();
                    }
                }))
                .into_any_element(),
        );
        let progress = (state == DownloadState::InProgress).then(|| {
            let fill = download.fraction();
            div()
                .h(theme.metrics.loading_bar_height)
                .mt(theme.metrics.radius_sm * PROGRESS_HEIGHT_SHARE)
                .rounded(theme.metrics.loading_bar_height)
                .bg(theme.colors.surface_hover)
                .overflow_hidden()
                .child(
                    div()
                        .h_full()
                        .w(relative(fill.unwrap_or(1.0)))
                        .bg(if fill.is_some() {
                            accent
                        } else {
                            accent.opacity(PROGRESS_HEIGHT_SHARE)
                        }),
                )
        });

        div()
            .id(("download", id as usize))
            .flex()
            .flex_col()
            .flex_shrink_0()
            .py(theme.metrics.radius_sm)
            .px(theme.metrics.radius_md)
            .rounded(theme.metrics.radius_md)
            .hover(move |style| style.bg(hover_bg))
            .when(state == DownloadState::Completed, |row| {
                let open_path = path.clone();
                row.on_click(cx.listener(move |this, _, _, cx| {
                    if dangerous {
                        this.download_confirm = Some(id);
                        cx.notify();
                    } else {
                        native::open_path(&open_path);
                    }
                }))
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(theme.metrics.radius_sm)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(download.file_name()),
                    )
                    .children(actions),
            )
            .child(
                div()
                    .text_size(theme.metrics.text_sm)
                    .text_color(status_color)
                    .child(status_line(download)),
            )
            .when(dangerous && state == DownloadState::Completed, |row| {
                row.child(
                    div()
                        .text_size(theme.metrics.text_sm)
                        .text_color(theme.colors.warning)
                        .child(DANGER_NOTE),
                )
            })
            .when(confirming, |row| {
                let confirmed_path = path.clone();
                row.child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(theme.metrics.radius_md)
                        .mt(theme.metrics.radius_sm)
                        .child(
                            confirm_button(
                                ("download-open-anyway", id as usize),
                                OPEN_ANYWAY,
                                true,
                                cx,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.download_confirm = None;
                                    native::open_path(&confirmed_path);
                                    cx.notify();
                                },
                            )),
                        )
                        .child(
                            confirm_button(
                                ("download-keep-closed", id as usize),
                                "Cancel",
                                false,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.stop_propagation();
                                this.download_confirm = None;
                                cx.notify();
                            })),
                        ),
                )
            })
            .children(progress)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn status_lines_describe_each_state() {
        let mut download = Download {
            id: 1,
            url: String::new(),
            path: PathBuf::from("f.zip"),
            received: 1024,
            total: Some(4096),
            state: DownloadState::InProgress,
            reason: None,
            started_at: 0,
        };
        assert_eq!(status_line(&download), "1.0 KB of 4.0 KB");
        download.total = None;
        assert_eq!(status_line(&download), "1.0 KB");
        download.state = DownloadState::Failed;
        assert_eq!(status_line(&download), "Failed");
        download.reason = Some("cross-origin redirect");
        download.url = "https://files.example.com/f.zip".into();
        assert_eq!(
            status_line(&download),
            "Failed · cross-origin redirect · files.example.com"
        );
    }
}
