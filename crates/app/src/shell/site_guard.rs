use browser_core::permissions::Decision;
use browser_core::security::{external_scheme, may_open_window, origin_of};
use browser_core::store;
use browser_core::tabs::{OpenPosition, TabId};
use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div,
};
use webview_host::{PermissionKind, SecurityState};

use super::Shell;
use crate::icons;
use crate::page;
use crate::theme::Theme;
use crate::tooltip::{Side, tooltip_on};

const MAX_BLOCKED_POPUPS_SHOWN: usize = 5;
const UNKNOWN_SITE: &str = "This page";

pub(super) enum SitePrompt {
    Permission {
        tab: TabId,
        request: u64,
        origin: String,
        kind: PermissionKind,
    },
    External {
        tab: TabId,
        uri: String,
        origin: String,
        scheme: String,
    },
}

impl SitePrompt {
    fn tab(&self) -> TabId {
        match self {
            Self::Permission { tab, .. } | Self::External { tab, .. } => *tab,
        }
    }
}

#[derive(Clone, Copy)]
enum Answer {
    Allow,
    Block,
    Dismiss,
}

impl Shell {
    pub(super) fn on_new_window_request(
        &mut self,
        tab: TabId,
        url: String,
        user_initiated: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let opened = self
            .runtime
            .get(&tab)
            .map_or(0, |runtime| runtime.windows_opened);
        if may_open_window(user_initiated, opened) {
            self.update_runtime(tab, |runtime| runtime.windows_opened += 1);
            self.open_tab(url, OpenPosition::After(tab), window, cx);
            return;
        }
        tracing::info!("blocked a popup to {url}");
        self.update_runtime(tab, |runtime| {
            if !runtime.blocked_popups.contains(&url) {
                runtime.blocked_popups.push(url);
                if runtime.blocked_popups.len() > MAX_BLOCKED_POPUPS_SHOWN {
                    runtime.blocked_popups.remove(0);
                }
            }
        });
    }

    pub(super) fn on_permission_request(
        &mut self,
        tab: TabId,
        request: u64,
        url: &str,
        kind: PermissionKind,
    ) {
        let Some(origin) = origin_of(url) else {
            webview_host::answer_permission(request, false);
            return;
        };
        match self.permissions.get(&origin, kind.key()) {
            Some(decision) => webview_host::answer_permission(request, decision == Decision::Allow),
            None => self.prompts.push(SitePrompt::Permission {
                tab,
                request,
                origin,
                kind,
            }),
        }
    }

    pub(super) fn on_external_scheme_request(
        &mut self,
        tab: TabId,
        uri: String,
        origin: String,
        user_initiated: bool,
    ) {
        match external_scheme(&uri) {
            Some(scheme) if user_initiated => self.prompts.push(SitePrompt::External {
                tab,
                uri,
                origin,
                scheme,
            }),
            _ => tracing::info!("refused to hand {uri} to another app"),
        }
    }

    pub(super) fn on_security_change(&mut self, tab: TabId, state: SecurityState) {
        self.update_runtime(tab, |runtime| runtime.security = Some(state));
    }

    /// A page that leaves loses its pending prompts, popups and window budget.
    pub(super) fn forget_page_state(&mut self, tab: TabId) {
        self.update_runtime(tab, |runtime| {
            runtime.windows_opened = 0;
            runtime.blocked_popups.clear();
        });
        self.deny_prompts_of(tab);
    }

    pub(super) fn deny_prompts_of(&mut self, tab: TabId) {
        let (theirs, others): (Vec<_>, Vec<_>) = std::mem::take(&mut self.prompts)
            .into_iter()
            .partition(|prompt| prompt.tab() == tab);
        self.prompts = others;
        for prompt in theirs {
            if let SitePrompt::Permission { request, .. } = prompt {
                webview_host::answer_permission(request, false);
            }
        }
    }

    fn answer_prompt(&mut self, index: usize, answer: Answer, cx: &mut Context<Self>) {
        if index >= self.prompts.len() {
            return;
        }
        match self.prompts.remove(index) {
            SitePrompt::Permission {
                request,
                origin,
                kind,
                ..
            } => {
                let allow = matches!(answer, Answer::Allow);
                webview_host::answer_permission(request, allow);
                let decision = match answer {
                    Answer::Allow => Some(Decision::Allow),
                    Answer::Block => Some(Decision::Block),
                    Answer::Dismiss => None,
                };
                if let Some(decision) = decision {
                    self.permissions.set(&origin, kind.key(), decision);
                    self.save_permissions();
                }
            }
            SitePrompt::External { uri, .. } => {
                if matches!(answer, Answer::Allow) {
                    crate::native::launch_uri(&uri);
                }
            }
        }
        cx.notify();
    }

    pub(super) fn save_permissions(&self) {
        // A private window's decisions last only as long as it does.
        if self.browser.private_shown() {
            return;
        }
        if let Err(error) = store::save_json(&self.paths.permissions(), &self.permissions) {
            tracing::error!("failed to save site permissions: {error:#}");
        }
    }

    pub(super) fn render_site_notices(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(active) = self.browser.active_tab_id() else {
            return Vec::new();
        };
        let mut notices: Vec<AnyElement> = self
            .prompts
            .iter()
            .enumerate()
            .filter(|(_, prompt)| prompt.tab() == active)
            .take(1)
            .map(|(index, prompt)| self.render_prompt(index, prompt, cx))
            .collect();
        if let Some(url) = self
            .runtime
            .get(&active)
            .and_then(|runtime| runtime.blocked_popups.last().cloned())
        {
            notices.push(self.render_blocked_popup(active, url, cx));
        }
        notices
    }

    fn render_prompt(
        &self,
        index: usize,
        prompt: &SitePrompt,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (message, allow_label, block_label): (String, &str, Option<&str>) = match prompt {
            SitePrompt::Permission { origin, kind, .. } => (
                format!("{} wants to {}.", host_of(origin), kind.description()),
                "Allow",
                Some("Block"),
            ),
            SitePrompt::External { origin, scheme, .. } => (
                format!(
                    "{} wants to open a {scheme}: link in another app.",
                    host_of(origin)
                ),
                "Open",
                None,
            ),
        };
        let mut actions = vec![
            notice_button(("prompt-allow", index), allow_label, true, cx)
                .on_click(
                    cx.listener(move |this, _, _, cx| this.answer_prompt(index, Answer::Allow, cx)),
                )
                .into_any_element(),
        ];
        if let Some(block_label) = block_label {
            actions.push(
                notice_button(("prompt-block", index), block_label, false, cx)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.answer_prompt(index, Answer::Block, cx)
                    }))
                    .into_any_element(),
            );
        }
        let dismiss =
            cx.listener(move |this, _, _, cx| this.answer_prompt(index, Answer::Dismiss, cx));
        notice(("site-prompt", index), message, actions, dismiss, cx)
    }

    fn render_blocked_popup(&self, tab: TabId, url: String, cx: &mut Context<Self>) -> AnyElement {
        let message = format!("Pop-up blocked: {}", page::host(&url));
        let open_url = url.clone();
        let open = notice_button("popup-open", "Open once", true, cx)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.update_runtime(tab, |runtime| {
                    runtime
                        .blocked_popups
                        .retain(|blocked| *blocked != open_url)
                });
                this.open_tab(open_url.clone(), OpenPosition::After(tab), window, cx);
            }))
            .into_any_element();
        let dismiss = cx.listener(move |this, _, _, cx| {
            this.update_runtime(tab, |runtime| runtime.blocked_popups.clear());
            cx.notify();
        });
        notice("popup-blocked", message, vec![open], dismiss, cx)
    }
}

fn host_of(origin: &str) -> SharedString {
    let host = page::host(origin);
    if host.is_empty() {
        SharedString::new_static(UNKNOWN_SITE)
    } else {
        host
    }
}

fn notice(
    id: impl Into<gpui::ElementId>,
    message: String,
    actions: Vec<AnyElement>,
    dismiss: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let theme = Theme::of(cx);
    let metrics = &theme.metrics;
    let hover_bg = theme.colors.surface_hover;
    div()
        .id(id)
        .flex()
        .flex_col()
        .flex_shrink_0()
        .gap(metrics.radius_md)
        .p(metrics.radius_md * 1.5)
        .rounded(metrics.radius_md)
        .bg(theme.colors.surface_raised)
        .border_1()
        .border_color(theme.colors.border)
        .child(
            div()
                .flex()
                .flex_row()
                .items_start()
                .gap(metrics.radius_md)
                .child(div().flex_1().min_w_0().child(message))
                .child(
                    div()
                        .id("notice-dismiss")
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .justify_center()
                        .size(metrics.nav_button_size - metrics.radius_md)
                        .rounded(metrics.radius_sm)
                        .font_family(theme.fonts.icons.clone())
                        .text_size(metrics.icon_glyph)
                        .text_color(theme.colors.text_muted)
                        .hover(move |style| style.bg(hover_bg))
                        .on_click(dismiss)
                        .tooltip(tooltip_on(Side::Left, "Dismiss"))
                        .child(icons::CLOSE),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .gap(metrics.radius_md)
                .children(actions),
        )
        .into_any_element()
}

pub(super) fn notice_button(
    id: impl Into<gpui::ElementId>,
    label: &str,
    primary: bool,
    cx: &Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::of(cx);
    let accent = theme.colors.accent;
    let hover_bg = theme.colors.surface_hover;
    div()
        .id(id)
        .flex()
        .items_center()
        .h(theme.metrics.nav_button_size)
        .px(theme.metrics.radius_md * 1.5)
        .rounded(theme.metrics.radius_md)
        .border_1()
        .border_color(if primary { accent } else { theme.colors.border })
        .bg(if primary {
            accent.opacity(0.18)
        } else {
            gpui::transparent_black()
        })
        .hover(move |style| style.bg(hover_bg))
        .child(SharedString::from(label.to_string()))
}
