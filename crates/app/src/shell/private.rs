//! Private browsing: a separate process of the browser, like Chrome's
//! incognito window. Its one space is private (`Browser::show_private`), its
//! pages run InPrivate, nothing it does is recorded or saved, and closing
//! its window erases it all.

use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div,
};

use super::Shell;
use crate::icons::{self, Icon};
use crate::theme::Theme;
use crate::tooltip::tooltip;

/// The page a private tab shows before it goes anywhere.
pub(super) const PRIVATE_PAGE: &str = include_str!("private_page.html");
/// Starts the process as a private window.
pub(crate) const PRIVATE_FLAG: &str = "--private";

const TITLE: &str = "Private";
const EXPLAIN: &str = "Nothing here is saved. Closing erases it all.";
const CLOSE: &str = "Close private window";
const CLOSE_HINT: &str = "Closes the private window and erases its tabs and everything it kept.";

impl Shell {
    /// Opens a private window: another process of this browser.
    pub(super) fn new_private_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let started = std::env::current_exe()
            .and_then(|exe| std::process::Command::new(exe).arg(PRIVATE_FLAG).spawn());
        if let Err(error) = started {
            self.show_toast(
                format!("Could not open a private window: {error}"),
                window,
                cx,
            );
        }
    }

    /// The foot of the sidebar in a private window: what it is, and the way
    /// out that erases it.
    pub(super) fn render_private_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.browser.private_shown() {
            return None;
        }
        let theme = Theme::of(cx);
        let metrics = &theme.metrics;
        let danger = theme.colors.danger;
        Some(
            div()
                .flex()
                .flex_col()
                .flex_shrink_0()
                .gap(metrics.radius_sm * 1.5)
                .p(metrics.radius_md)
                .rounded(metrics.radius_frame)
                .bg(theme.colors.surface_hover)
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap(metrics.radius_md)
                        .px(metrics.radius_sm / 2.)
                        .pt(metrics.radius_sm / 2.)
                        .pb(metrics.radius_sm)
                        .child(glyph(icons::INCOGNITO, cx))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_size(metrics.text_md)
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .child(TITLE),
                                )
                                .child(
                                    div()
                                        .text_size(metrics.text_sm)
                                        .text_color(theme.colors.text_muted)
                                        .child(EXPLAIN),
                                ),
                        ),
                )
                .child(
                    action_button("private-close", icons::CLOSE, CLOSE.into(), true, cx)
                        .text_color(danger)
                        .tooltip(tooltip(CLOSE_HINT))
                        .on_click(|_, window, _| window.remove_window()),
                )
                .into_any_element(),
        )
    }
}

fn glyph(icon: Icon, cx: &Context<Shell>) -> impl IntoElement {
    let theme = Theme::of(cx);
    div()
        .flex_shrink_0()
        .text_size(theme.metrics.nav_glyph * 1.25)
        .text_color(theme.colors.text_muted)
        .child(icon)
}

fn action_button(
    id: &'static str,
    icon: Icon,
    label: SharedString,
    danger: bool,
    cx: &Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::of(cx);
    let metrics = &theme.metrics;
    let hover_bg = theme.colors.surface_active;
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .w_full()
        .h(metrics.nav_button_size)
        .gap(metrics.radius_md)
        .px(metrics.radius_md * 1.5)
        .rounded(metrics.radius_md)
        .border_1()
        .border_color(if danger {
            theme.colors.danger.opacity(0.5)
        } else {
            theme.colors.border
        })
        .hover(move |style| style.bg(hover_bg))
        .child(div().text_size(metrics.nav_glyph).child(icon))
        .child(div().min_w_0().truncate().child(label))
}
