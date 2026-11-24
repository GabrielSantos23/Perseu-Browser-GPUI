use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use browser_core::bangs;
use browser_core::command_bar::{Candidate, Group, rank};
use browser_core::omnibox::{self, SearchEngine};
use browser_core::tabs::TabId;
use gpui::{
    AnyElement, App, AppContext, Context, Entity, EventEmitter, Image, ImageFormat,
    InteractiveElement, IntoElement, KeyBinding, KeyDownEvent, MouseButton, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, Subscription, Window, actions, div, img,
    prelude::FluentBuilder, px,
};
use gpui_ce_motion::MotionValue;
use gpui_kit::component::input::{Input, InputEvent, InputState};

use crate::icons;
use crate::motion;
use crate::theme::Theme;

actions!(
    command_bar,
    [
        SelectNext,
        SelectPrevious,
        NextGroup,
        PreviousGroup,
        Dismiss
    ]
);

const KEY_CONTEXT: &str = "CommandBar";
const INPUT_CONTEXT: &str = "CommandBar > Input";
const SELECTED_ROW_TINT: f32 = 0.22;
const LABEL_SHARE: f32 = 0.75;
const RESIZE: std::time::Duration = std::time::Duration::from_millis(200);
const PLACEHOLDER: &str = "Search tabs, history, commands, or the web";
const BACKSPACE_KEY: &str = "backspace";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const ICON_TIMEOUT_SECS: &str = "5";
const ICON_MAX_BYTES: &str = "1000000";

/// Bang site icons fetched this session, by domain; `None` when the site
/// has no usable `/favicon.ico`.
static BANG_ICONS: LazyLock<Mutex<HashMap<String, Option<Arc<Image>>>>> =
    LazyLock::new(Mutex::default);

/// The site's own `/favicon.ico`, asked of the site itself (no icon
/// service), with Windows' curl.
fn fetch_icon(domain: &str) -> Option<Arc<Image>> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("curl.exe")
        .args([
            "--fail",
            "--silent",
            "--location",
            "--max-time",
            ICON_TIMEOUT_SECS,
            "--max-filesize",
            ICON_MAX_BYTES,
            &format!("https://{domain}/favicon.ico"),
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    let bytes = output.stdout;
    let format = match bytes.get(..4)? {
        [0x89, b'P', b'N', b'G'] => ImageFormat::Png,
        [0, 0, 1, 0] => ImageFormat::Ico,
        [b'G', b'I', b'F', b'8'] => ImageFormat::Gif,
        [0xFF, 0xD8, _, _] => ImageFormat::Jpeg,
        _ => return None,
    };
    (output.status.success()).then(|| Arc::new(Image::from_bytes(format, bytes)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    NewSpace,
    CloseTab,
    ReopenClosedTab,
    ViewArchive,
    Reload,
    ToggleCompactMode,
    SplitView,
    Unsplit,
    RotateSplit,
    Settings,
}

impl Command {
    pub const ALL: [Command; 10] = [
        Command::SplitView,
        Command::Unsplit,
        Command::RotateSplit,
        Command::ToggleCompactMode,
        Command::NewSpace,
        Command::CloseTab,
        Command::ReopenClosedTab,
        Command::ViewArchive,
        Command::Reload,
        Command::Settings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::NewSpace => "New space",
            Self::CloseTab => "Close tab",
            Self::ReopenClosedTab => "Reopen closed tab",
            Self::ViewArchive => "View archive",
            Self::Reload => "Reload page",
            Self::ToggleCompactMode => "Toggle compact mode",
            Self::SplitView => "Split view",
            Self::Unsplit => "Unsplit view",
            Self::RotateSplit => "Rotate split",
            Self::Settings => "Settings",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Tab(TabId),
    Open(String),
    Run(Command),
}

pub enum CommandBarEvent {
    Confirm(Target),
    Dismiss,
}

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", SelectNext, Some(INPUT_CONTEXT)),
        KeyBinding::new("up", SelectPrevious, Some(INPUT_CONTEXT)),
        KeyBinding::new("tab", NextGroup, Some(INPUT_CONTEXT)),
        KeyBinding::new("shift-tab", PreviousGroup, Some(INPUT_CONTEXT)),
        KeyBinding::new("escape", Dismiss, Some(INPUT_CONTEXT)),
    ]);
}

pub struct CommandBar {
    input: Entity<InputState>,
    candidates: Arc<Vec<Candidate<Target>>>,
    engine: SearchEngine,
    results: Vec<Candidate<Target>>,
    /// How many of `results` come from the typed text rather than ranking.
    typed: usize,
    /// Ranking runs off the UI thread: history can hold thousands of visits.
    ranking: Option<gpui::Task<()>>,
    /// The bang typed as `!trigger ` and shown as its site's icon.
    bang: Option<bangs::Site>,
    bang_icon: Option<Arc<Image>>,
    selected: usize,
    height: MotionValue,
    target_height: Option<f32>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<CommandBarEvent> for CommandBar {}

impl CommandBar {
    pub fn new(
        candidates: Vec<Candidate<Target>>,
        engine: SearchEngine,
        initial_text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder(PLACEHOLDER);
            state.set_value(initial_text, window, cx);
            state
        });
        input.update(cx, |state, cx| {
            state.focus(window, cx);
            state.select_all(window, cx);
        });
        let subscriptions = vec![
            cx.subscribe_in(&input, window, Self::on_input_event),
            cx.observe_window_activation(window, |_, window, cx| {
                if !window.is_window_active() {
                    cx.emit(CommandBarEvent::Dismiss);
                }
            }),
        ];
        let mut bar = Self {
            input,
            candidates: Arc::new(candidates),
            engine,
            results: Vec::new(),
            typed: 0,
            ranking: None,
            bang: None,
            bang_icon: None,
            selected: 0,
            height: MotionValue::new(0.0),
            target_height: None,
            _subscriptions: subscriptions,
        };
        if bar.absorb_bang(window, cx) {
            bar.input
                .update(cx, |state, cx| state.select_all(window, cx));
        }
        bar.refresh(cx);
        bar
    }

    /// Turns a leading `!trigger ` into the bang chip, keeping the rest of
    /// the text. Returns whether it did.
    fn absorb_bang(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.bang.is_some() {
            return false;
        }
        let value = self.input.read(cx).value().to_string();
        let Some((trigger, rest)) = value
            .strip_prefix('!')
            .and_then(|value| value.split_once(' '))
        else {
            return false;
        };
        let Some(site) = bangs::site(trigger) else {
            return false;
        };
        let rest = rest.trim_start().to_string();
        self.bang = Some(site);
        self.load_bang_icon(site.domain, cx);
        self.input
            .update(cx, |state, cx| state.set_value(rest, window, cx));
        true
    }

    /// Backspace in an empty field gives the bang back as text.
    fn release_bang(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.input.read(cx).value().is_empty() {
            return false;
        }
        let Some(site) = self.bang.take() else {
            return false;
        };
        self.bang_icon = None;
        self.input.update(cx, |state, cx| {
            state.set_value(format!("!{}", site.trigger), window, cx)
        });
        self.refresh(cx);
        true
    }

    fn load_bang_icon(&mut self, domain: &'static str, cx: &mut Context<Self>) {
        let cached = BANG_ICONS
            .lock()
            .ok()
            .and_then(|icons| icons.get(domain).cloned());
        if let Some(icon) = cached {
            self.bang_icon = icon;
            return;
        }
        let fetch = cx.background_spawn(async move { fetch_icon(domain) });
        cx.spawn(async move |this, cx| {
            let icon = fetch.await;
            if let Ok(mut icons) = BANG_ICONS.lock() {
                icons.insert(domain.to_string(), icon.clone());
            }
            this.update(cx, |this, cx| {
                if this.bang.is_some_and(|site| site.domain == domain) {
                    this.bang_icon = icon;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn on_input_event(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                self.absorb_bang(window, cx);
                self.refresh(cx)
            }
            InputEvent::PressEnter { .. } => self.confirm(self.selected, cx),
            InputEvent::Focus | InputEvent::Blur => {}
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let query = self.input.read(cx).value().to_string();
        let typed = match self.bang {
            Some(site) => vec![bang_candidate(site, &query)],
            None => typed_candidates(&query, &self.engine),
        };
        // The earlier ranking stays until the new one is ready; Enter only
        // needs the typed rows, which are always current.
        let ranked = self.results.split_off(self.typed);
        self.typed = typed.len();
        self.results = typed;
        self.results.extend(ranked);
        self.selected = 0;
        cx.notify();
        let candidates = self.candidates.clone();
        let ranked = cx.background_spawn(async move { rank(&query, &candidates) });
        self.ranking = Some(cx.spawn(async move |this, cx| {
            let ranked = ranked.await;
            this.update(cx, |this, cx| {
                this.results.truncate(this.typed);
                this.results.extend(ranked);
                if this.selected >= this.results.len() {
                    this.selected = 0;
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// The text typed in the bar, with its bang written out.
    pub fn query(&self, cx: &App) -> String {
        let text = self.input.read(cx).value().to_string();
        match self.bang {
            Some(site) => format!("!{} {text}", site.trigger),
            None => text,
        }
    }

    fn confirm(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(result) = self.results.get(index) {
            cx.emit(CommandBarEvent::Confirm(result.target.clone()));
        }
    }

    fn render_bang_chip(&self, site: bangs::Site, cx: &App) -> AnyElement {
        let theme = Theme::of(cx);
        let metrics = &theme.metrics;
        let icon: AnyElement = match self.bang_icon.clone() {
            Some(icon) => img(icon).size(metrics.favicon_size).into_any_element(),
            None => div()
                .font_family(theme.fonts.icons.clone())
                .text_size(metrics.nav_glyph)
                .text_color(theme.colors.text_muted)
                .child(icons::SEARCH)
                .into_any_element(),
        };
        div()
            .id("bang-chip")
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .size(metrics.favicon_size + metrics.radius_md * 2.)
            .mr(metrics.radius_md)
            .rounded(metrics.radius_md)
            .bg(theme.colors.surface_hover)
            .tooltip(crate::tooltip::tooltip(site.name))
            .child(icon)
            .into_any_element()
    }

    fn move_selection(&mut self, offset: isize, cx: &mut Context<Self>) {
        let len = self.results.len() as isize;
        if len > 0 {
            self.selected = (self.selected as isize + offset).rem_euclid(len) as usize;
            cx.notify();
        }
    }

    fn jump_group(&mut self, forward: bool, cx: &mut Context<Self>) {
        let starts = group_starts(&self.results);
        if starts.is_empty() {
            return;
        }
        let current = starts
            .iter()
            .rposition(|start| *start <= self.selected)
            .unwrap_or(0);
        let next = if forward {
            (current + 1) % starts.len()
        } else {
            (current + starts.len() - 1) % starts.len()
        };
        self.selected = starts[next];
        cx.notify();
    }

    fn render_row(
        &self,
        index: usize,
        result: &Candidate<Target>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx);
        let selected = index == self.selected;
        let hover_bg = theme.colors.surface_hover;

        div()
            .id(("command-result", index))
            .flex()
            .flex_shrink_0()
            .flex_row()
            .items_center()
            .gap(theme.metrics.radius_md * 1.5)
            .h(theme.metrics.row_height + theme.metrics.radius_sm)
            .px(theme.metrics.radius_md * 1.5)
            .rounded(theme.metrics.radius_md)
            .when(selected, |row| {
                row.bg(theme.colors.accent.opacity(SELECTED_ROW_TINT))
            })
            .when(!selected, |row| row.hover(move |style| style.bg(hover_bg)))
            .on_click(cx.listener(move |this, _, _, cx| this.confirm(index, cx)))
            .child(
                div()
                    .flex_shrink_0()
                    .w(theme.metrics.favicon_size)
                    .font_family(theme.fonts.icons.clone())
                    .text_size(theme.metrics.nav_glyph)
                    .text_color(theme.colors.text_muted)
                    .child(group_glyph(result.group)),
            )
            .child(div().min_w_0().truncate().child(result.title.clone()))
            .when(!result.subtitle.is_empty(), |row| {
                row.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(theme.metrics.text_sm)
                        .text_color(theme.colors.text_muted)
                        .child(result.subtitle.clone()),
                )
            })
            .into_any_element()
    }
}

impl CommandBar {
    /// The panel's height for this many result rows and group labels,
    /// springing from the last height when the results change.
    fn animated_height(
        &mut self,
        rows: usize,
        labels: usize,
        window: &mut Window,
        cx: &App,
    ) -> f32 {
        let metrics = &Theme::of(cx).metrics;
        let gap = f32::from(metrics.radius_sm);
        let header = f32::from(metrics.url_field_height + metrics.radius_md);
        let mut target = f32::from(metrics.radius_md) * 2. + 2. + header;
        if rows > 0 {
            target += gap + 1.;
            target += rows as f32 * (gap + f32::from(metrics.row_height + metrics.radius_sm));
            target += labels as f32 * (gap + f32::from(metrics.row_height * LABEL_SHARE));
        }
        let target = target.min(f32::from(metrics.command_bar_height - metrics.radius_md));
        let now = motion::now();
        match self.target_height {
            None => self.height.jump(f64::from(target), now),
            Some(previous) if (previous - target).abs() > 0.5 => {
                self.height
                    .animate(f64::from(target), motion::spring(RESIZE), now)
            }
            Some(_) => {}
        }
        self.target_height = Some(target);
        let sample = self.height.sample(now, motion::reduced());
        if sample.is_animating() {
            window.request_animation_frame();
        }
        sample.value() as f32
    }
}

impl Render for CommandBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut labels = 0;
        let mut previous_group = None;
        for (index, result) in self.results.clone().iter().enumerate() {
            if previous_group != Some(result.group) {
                previous_group = Some(result.group);
                if let Some(label) = group_label(result.group) {
                    rows.push(section_label(label, cx));
                    labels += 1;
                }
            }
            rows.push(self.render_row(index, result, cx));
        }
        let height = self.animated_height(rows.len() - labels, labels, window, cx);
        let theme = Theme::of(cx);

        div()
            .id("command-bar-backdrop")
            .key_context(KEY_CONTEXT)
            .size_full()
            .font_family(theme.fonts.ui.clone())
            .font_weight(theme.fonts.ui_weight)
            .text_size(theme.metrics.text_md)
            .text_color(theme.colors.text)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(CommandBarEvent::Dismiss)),
            )
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.move_selection(1, cx)))
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| this.move_selection(-1, cx)))
            .on_action(cx.listener(|this, _: &NextGroup, _, cx| this.jump_group(true, cx)))
            .on_action(cx.listener(|this, _: &PreviousGroup, _, cx| this.jump_group(false, cx)))
            .on_action(cx.listener(|_, _: &Dismiss, _, cx| cx.emit(CommandBarEvent::Dismiss)))
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == BACKSPACE_KEY && this.release_bang(window, cx) {
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .id("command-bar")
                    .flex()
                    .flex_col()
                    .w_full()
                    .h(px(height))
                    .overflow_hidden()
                    .p(theme.metrics.radius_md)
                    .gap(theme.metrics.radius_sm)
                    .rounded(theme.metrics.radius_popover)
                    .bg(theme.colors.surface_raised)
                    .border_1()
                    .border_color(theme.colors.border)
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .items_center()
                            .h(theme.metrics.url_field_height + theme.metrics.radius_md)
                            .px(theme.metrics.radius_md * 1.5)
                            .text_size(theme.metrics.command_bar_text)
                            .children(self.bang.map(|site| self.render_bang_chip(site, cx)))
                            .child(
                                Input::new(&self.input)
                                    .appearance(false)
                                    .px_0()
                                    .text_size(theme.metrics.command_bar_text)
                                    .aria_label("Command bar"),
                            ),
                    )
                    .when(!rows.is_empty(), |panel| {
                        panel
                            .child(div().flex_shrink_0().h_px().bg(theme.colors.border))
                            .children(rows)
                    }),
            )
    }
}

fn bang_candidate(site: bangs::Site, query: &str) -> Candidate<Target> {
    let query = query.trim();
    let title = if query.is_empty() {
        format!("Open {}", site.name)
    } else {
        format!("{query} — Search {}", site.name)
    };
    Candidate {
        group: Group::Search,
        title,
        subtitle: String::new(),
        target: Target::Open(site.url(query)),
    }
}

fn typed_candidates(query: &str, engine: &SearchEngine) -> Vec<Candidate<Target>> {
    let typed = query.trim();
    if let Some(bang) = browser_core::bangs::expand(typed) {
        let title = if bang.query.is_empty() {
            format!("Open {}", bang.name)
        } else {
            format!("{} — Search {}", bang.query, bang.name)
        };
        return vec![Candidate {
            group: Group::Search,
            title,
            subtitle: String::new(),
            target: Target::Open(bang.url),
        }];
    }
    let Some(resolved) = omnibox::resolve(typed, engine) else {
        return Vec::new();
    };
    let search_url = engine.search_url(typed);
    let mut candidates = Vec::new();
    if resolved != search_url {
        candidates.push(Candidate {
            group: Group::Address,
            title: typed.to_string(),
            subtitle: String::new(),
            target: Target::Open(resolved),
        });
    }
    candidates.push(Candidate {
        group: Group::Search,
        title: format!("{typed} — Search {}", engine.name),
        subtitle: String::new(),
        target: Target::Open(search_url),
    });
    candidates
}

fn group_starts(results: &[Candidate<Target>]) -> Vec<usize> {
    results
        .iter()
        .enumerate()
        .filter(|(index, result)| *index == 0 || results[index - 1].group != result.group)
        .map(|(index, _)| index)
        .collect()
}

fn group_label(group: Group) -> Option<&'static str> {
    match group {
        Group::Tabs => Some("Tabs"),
        Group::Pinned => Some("Pinned"),
        Group::Commands => Some("Commands"),
        Group::History => Some("History"),
        Group::Address | Group::Search => None,
    }
}

fn group_glyph(group: Group) -> icons::Icon {
    match group {
        Group::Address | Group::Tabs | Group::History => icons::GLOBE,
        Group::Pinned => icons::PIN,
        Group::Commands => icons::COMMAND,
        Group::Search => icons::SEARCH,
    }
}

fn section_label(label: &'static str, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    div()
        .flex()
        .items_end()
        .flex_shrink_0()
        .h(theme.metrics.row_height * LABEL_SHARE)
        .px(theme.metrics.radius_md * 1.5)
        .text_size(theme.metrics.text_sm)
        .text_color(theme.colors.text_muted)
        .child(SharedString::new_static(label))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> SearchEngine {
        SearchEngine::GOOGLE
    }

    #[test]
    fn plain_words_offer_only_a_search() {
        let typed = typed_candidates("rust gpui", &engine());
        assert_eq!(typed.len(), 1);
        assert_eq!(typed[0].group, Group::Search);
    }

    #[test]
    fn addresses_offer_open_before_search() {
        let typed = typed_candidates("github.com", &engine());
        assert_eq!(typed[0].group, Group::Address);
        assert_eq!(typed[0].target, Target::Open("https://github.com".into()));
        assert_eq!(typed[1].group, Group::Search);
    }

    #[test]
    fn blank_query_offers_nothing() {
        assert!(typed_candidates("  ", &engine()).is_empty());
    }

    #[test]
    fn group_starts_mark_each_new_group() {
        let make = |group| Candidate {
            group,
            title: String::new(),
            subtitle: String::new(),
            target: Target::Run(Command::Reload),
        };
        let results = [
            make(Group::Tabs),
            make(Group::Tabs),
            make(Group::Commands),
            make(Group::Search),
        ];
        assert_eq!(group_starts(&results), [0, 2, 3]);
    }
}
