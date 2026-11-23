use std::path::PathBuf;

use browser_core::import::{self, ImportError, Source, SourceBrowser};
use browser_core::omnibox::SearchEngine;
use browser_core::settings::{ARCHIVE_PRESETS_SECS, PeekModifier, Settings};
use browser_core::store;
use gpui::{
    AnyElement, App, AppContext, ClickEvent, Context, Div, ElementId, Entity, FocusHandle,
    FontWeight, InteractiveElement, IntoElement, KeyBinding, ParentElement, PathPromptOptions,
    SharedString, Stateful, StatefulInteractiveElement, Styled, Subscription, Window, actions, div,
    img, prelude::FluentBuilder, px, rgb,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable as _, Sizable};

use super::history_page::{dropdown, empty_state};
use super::tab_ops::now;
use super::{Shell, archive};
use crate::actions;
use crate::context_menu::MenuEntry;
use crate::icons::{self, Icon};
use crate::paths;
use crate::theme::{ACCENT_PRESETS, Theme};
use crate::tooltip::tooltip;

actions!(settings_page, [CloseSettings]);

const KEY_CONTEXT: &str = "Settings";
const ESCAPE_KEY: &str = "escape";
const PAGE_TITLE: &str = "Settings";
const SEARCH: &str = "Search settings";
const NAV_WIDTH: f32 = 224.;
const PAGE_MAX_WIDTH: f32 = 850.;
const PAGE_PAD_TOP: f32 = 52.;
const PAGE_PAD_X: f32 = 40.;
const PAGE_PAD_BOTTOM: f32 = 64.;
const HEADING: f32 = 27.;
const GROUP_TITLE: f32 = 15.;
const ROW_TITLE: f32 = 13.5;
const LABEL: f32 = 12.;
const NAV_TEXT: f32 = 13.;
const NAV_GROUP: f32 = 10.;
const ROW_MIN_HEIGHT: f32 = 56.;
const HAIRLINE_INSET: f32 = 16.;
const CHECKBOX: f32 = 16.;
const SHORTCUT_FIELD_WIDTH: f32 = 180.;
const SECS_PER_HOUR: u64 = 3_600;
const SECS_PER_DAY: u64 = 86_400;
const SECS_PER_WEEK: u64 = 604_800;
const ICON_CREDIT: &str = "Icons: Uicons by Flaticon (flaticon.com/uicons).";

pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        ESCAPE_KEY,
        CloseSettings,
        Some(KEY_CONTEXT),
    )]);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    General,
    Appearance,
    Tabs,
    Privacy,
    Extensions,
    Keyboard,
    About,
}

impl Section {
    const ALL: [Section; 7] = [
        Section::General,
        Section::Appearance,
        Section::Tabs,
        Section::Privacy,
        Section::Extensions,
        Section::Keyboard,
        Section::About,
    ];

    fn title(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Appearance => "Appearance",
            Section::Tabs => "Tabs",
            Section::Privacy => "Privacy",
            Section::Extensions => "Extensions",
            Section::Keyboard => "Keyboard",
            Section::About => "About",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Section::General => {
                "Your default browser, search, downloads and data from other browsers."
            }
            Section::Appearance => "Colors and the parts of the window you see.",
            Section::Tabs => "How idle tabs are put away and how links are previewed.",
            Section::Privacy => "What this browser keeps, and what sites may do.",
            Section::Extensions => "Chromium extensions, hosted by the WebView2 runtime.",
            Section::Keyboard => "Shortcuts for the browser's own commands.",
            Section::About => "This build of the browser.",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Section::General => icons::SETTINGS,
            Section::Appearance => icons::PALETTE,
            Section::Tabs => icons::BROWSERS,
            Section::Privacy => icons::SHIELD,
            Section::Extensions => icons::PUZZLE,
            Section::Keyboard => icons::KEYBOARD,
            Section::About => icons::INFO,
        }
    }
}

/// A titled card of rows on a settings page.
struct Group {
    title: &'static str,
    note: Option<&'static str>,
    rows: Vec<Row>,
}

/// A setting: its name and explanation on the left, its control on the
/// right, and anything that needs the full width below.
struct Row {
    title: SharedString,
    description: Option<SharedString>,
    control: Option<AnyElement>,
    below: Option<AnyElement>,
}

impl Row {
    fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            description: None,
            control: None,
            below: None,
        }
    }

    fn describe(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    fn control(mut self, control: impl IntoElement) -> Self {
        self.control = Some(control.into_any_element());
        self
    }

    fn below(mut self, below: impl IntoElement) -> Self {
        self.below = Some(below.into_any_element());
        self
    }

    fn matches(&self, query: &str) -> bool {
        self.title.to_lowercase().contains(query)
            || self
                .description
                .as_ref()
                .is_some_and(|text| text.to_lowercase().contains(query))
    }
}

pub(super) struct SettingsPage {
    focus_handle: FocusHandle,
    section: Section,
    search: Entity<InputState>,
    query: String,
    shortcuts: Vec<ShortcutField>,
    error: Option<String>,
    default_browser_error: Option<String>,
    confirm_clear: bool,
    cleared: bool,
    import: ImportState,
    _subscriptions: Vec<Subscription>,
}

struct ShortcutField {
    default: &'static str,
    label: SharedString,
    input: Entity<InputState>,
}

/// Bringing bookmarks and history over from another browser.
struct ImportState {
    /// `None` while the other browsers are being looked for.
    sources: Option<Vec<Source>>,
    chosen: usize,
    profile: usize,
    bookmarks: bool,
    history: bool,
    job: Option<ImportJob>,
}

struct ImportJob {
    browser: SourceBrowser,
    finished: bool,
    bookmarks: Option<KindResult>,
    history: Option<KindResult>,
}

enum KindResult {
    Reading,
    /// How many were added, of how many read, and whether a limit stopped it.
    Done {
        added: usize,
        read: usize,
        partial: bool,
    },
    Failed(String),
}

impl KindResult {
    fn text(&self) -> String {
        match self {
            KindResult::Reading => "Reading\u{2026}".into(),
            KindResult::Done { partial: true, .. } => {
                "Import stopped at a limit. Items already brought over stay.".into()
            }
            KindResult::Done {
                added: 0, read: 0, ..
            } => "Nothing to import".into(),
            KindResult::Done { added: 0, .. } => "Already here".into(),
            KindResult::Done { added, .. } => format!("{added} added"),
            KindResult::Failed(reason) => reason.clone(),
        }
    }
}

fn archive_label(secs: u64) -> String {
    match secs {
        0 => "Never".to_string(),
        secs if secs % SECS_PER_WEEK == 0 => plural(secs / SECS_PER_WEEK, "week"),
        secs if secs % SECS_PER_DAY == 0 => plural(secs / SECS_PER_DAY, "day"),
        secs => plural(secs.div_ceil(SECS_PER_HOUR), "hour"),
    }
}

fn plural(count: u64, unit: &str) -> String {
    if count == 1 {
        format!("1 {unit}")
    } else {
        format!("{count} {unit}s")
    }
}

impl Shell {
    pub(super) fn effective_downloads(&self) -> Option<PathBuf> {
        self.settings
            .downloads_folder
            .clone()
            .or_else(paths::system_downloads)
    }

    pub(super) fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_page.is_some() {
            self.close_settings(window, cx);
        } else {
            self.open_settings(window, cx);
        }
    }

    pub(super) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(page) = &self.settings_page {
            window.focus(&page.focus_handle, cx);
            return;
        }
        self.close_history(window, cx);
        let mut shortcuts = Vec::new();
        let mut subscriptions = Vec::new();
        for row in actions::shortcut_rows(&self.settings.shortcuts) {
            let input = cx.new(|cx| {
                let mut state = InputState::new(window, cx).placeholder(row.default);
                state.set_value(row.keys.clone(), window, cx);
                state
            });
            let default = row.default;
            subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |shell, input, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        shell.commit_shortcut(default, input, window, cx);
                    }
                },
            ));
            shortcuts.push(ShortcutField {
                default,
                label: row.label.into(),
                input,
            });
        }
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(SEARCH));
        subscriptions.push(cx.subscribe_in(
            &search,
            window,
            |shell, input, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change)
                    && let Some(page) = &mut shell.settings_page
                {
                    page.query = input.read(cx).value().trim().to_string();
                    cx.notify();
                }
            },
        ));
        let page = SettingsPage {
            focus_handle: cx.focus_handle(),
            section: Section::General,
            search,
            query: String::new(),
            shortcuts,
            error: None,
            default_browser_error: None,
            confirm_clear: false,
            cleared: false,
            import: ImportState {
                sources: None,
                chosen: 0,
                profile: 0,
                bookmarks: true,
                history: true,
                job: None,
            },
            _subscriptions: subscriptions,
        };
        self.take_keyboard_from_pages();
        window.focus(&page.focus_handle, cx);
        self.settings_page = Some(page);
        self.show_only_active_view();
        self.find_import_sources(window, cx);
        cx.notify();
    }

    pub(super) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_page.take().is_none() {
            return;
        }
        self.show_only_active_view();
        window.focus(&self.focus_handle, cx);
        if let Some(view) = self.active_view() {
            view.focus();
        }
        cx.notify();
    }

    fn commit_shortcut(
        &mut self,
        default: &'static str,
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let typed = input.read(cx).value().to_string();
        match actions::edit_shortcut(&self.settings.shortcuts, default, &typed) {
            Ok(edited) => {
                let keys = edited
                    .get(default)
                    .cloned()
                    .unwrap_or_else(|| default.to_string());
                if typed != keys {
                    input.update(cx, |state, cx| state.set_value(keys, window, cx));
                }
                if let Some(page) = &mut self.settings_page {
                    page.error = None;
                }
                self.update_settings(|settings| settings.shortcuts = edited, cx);
            }
            Err(reason) => {
                let label = self.settings_page.as_ref().and_then(|page| {
                    page.shortcuts
                        .iter()
                        .find(|field| field.default == default)
                        .map(|field| field.label.clone())
                });
                if let Some(page) = &mut self.settings_page {
                    page.error = Some(match label {
                        Some(label) => format!("{label}: {reason}"),
                        None => reason,
                    });
                }
                cx.notify();
            }
        }
    }

    pub(super) fn update_settings(
        &mut self,
        edit: impl FnOnce(&mut Settings),
        cx: &mut Context<Self>,
    ) {
        let before = self.settings.clone();
        edit(&mut self.settings);
        if self.settings == before {
            return;
        }
        if let Err(error) = store::save_json(&self.paths.settings(), &self.settings) {
            tracing::error!("failed to save settings: {error:#}");
        }
        if self.settings.accent != before.accent {
            Theme::set_accent(self.settings.accent, cx);
        }
        if self.settings.archive_after_secs != before.archive_after_secs {
            self.browser.archive_after_ms = archive::archive_after_ms(&self.settings);
        }
        if self.settings.downloads_folder != before.downloads_folder
            && let Some(folder) = self.effective_downloads()
        {
            for view in self
                .runtime
                .values()
                .filter_map(|runtime| runtime.view.as_ref())
            {
                view.set_download_folder(&folder);
            }
        }
        if self.settings.shortcuts != before.shortcuts {
            actions::bind_shortcuts(cx, &self.settings.shortcuts);
        }
        cx.notify();
    }

    fn choose_downloads_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Use this folder".into()),
        });
        cx.spawn_in(window, async move |shell, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(folder) = paths.into_iter().next() else {
                return;
            };
            shell
                .update(cx, |shell, cx| {
                    shell.update_settings(|settings| settings.downloads_folder = Some(folder), cx)
                })
                .ok();
        })
        .detach();
    }

    fn clear_browsing_data(&mut self, cx: &mut Context<Self>) {
        super::persist::log_failure(self.history.clear());
        self.permissions.clear();
        self.save_permissions();
        self.downloads.clear_finished();
        match self.any_view() {
            Some(view) => view.clear_browsing_data(),
            None => self.pending_profile_clear = true,
        }
        if let Some(page) = &mut self.settings_page {
            page.confirm_clear = false;
            page.cleared = true;
        }
        cx.notify();
    }

    fn make_default_browser(&mut self, cx: &mut Context<Self>) {
        let result = crate::default_browser::register();
        if result.is_ok() {
            crate::default_browser::open_default_apps();
        }
        if let Some(page) = &mut self.settings_page {
            page.default_browser_error = result.err().map(|error| format!("{error:#}"));
        }
        cx.notify();
    }

    fn find_import_sources(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |shell, cx| {
            let found = cx
                .background_executor()
                .spawn(async move { import::discover() })
                .await;
            shell
                .update(cx, |shell, cx| {
                    if let Some(page) = &mut shell.settings_page {
                        page.import.sources = Some(found);
                        cx.notify();
                    }
                })
                .ok();
        })
        .detach();
    }

    fn start_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(page) = &mut self.settings_page else {
            return;
        };
        let state = &mut page.import;
        let Some(source) = state
            .sources
            .as_ref()
            .and_then(|found| found.get(state.chosen))
        else {
            return;
        };
        let Some(profile) = source.profiles.get(state.profile) else {
            return;
        };
        let browser = source.browser;
        let dir = profile.dir.clone();
        let want_bookmarks = state.bookmarks && profile.bookmarks;
        let want_history = state.history && profile.history;
        if !want_bookmarks && !want_history {
            return;
        }
        state.job = Some(ImportJob {
            browser,
            finished: false,
            bookmarks: want_bookmarks.then_some(KindResult::Reading),
            history: want_history.then_some(KindResult::Reading),
        });
        cx.notify();
        let started = now();
        cx.spawn_in(window, async move |shell, cx| {
            let (bookmarks, history) = cx
                .background_executor()
                .spawn(async move {
                    let bookmarks = want_bookmarks.then(|| import::bookmarks(browser, &dir));
                    let history = want_history.then(|| import::history(browser, &dir, started));
                    (bookmarks, history)
                })
                .await;
            shell
                .update(cx, |shell, cx| shell.finish_import(bookmarks, history, cx))
                .ok();
        })
        .detach();
    }

    fn finish_import(
        &mut self,
        bookmarks: Option<Result<Vec<import::Bookmark>, ImportError>>,
        history: Option<Result<Vec<browser_core::history::HistoryEntry>, ImportError>>,
        cx: &mut Context<Self>,
    ) {
        let Some(browser) = self
            .settings_page
            .as_ref()
            .and_then(|page| page.import.job.as_ref())
            .map(|job| job.browser)
        else {
            return;
        };
        let bookmarks = bookmarks.map(|read| match read {
            Ok(found) => {
                let added = self
                    .browser
                    .import_bookmarks(&found, import::MAX_BOOKMARKS, now());
                KindResult::Done {
                    added,
                    read: found.len(),
                    partial: added == import::MAX_BOOKMARKS && found.len() > added,
                }
            }
            Err(error) => KindResult::Failed(error.describe(browser)),
        });
        let history = history.map(|read| match read {
            Ok(visits) => match self.history.import(&visits) {
                Ok(added) => KindResult::Done {
                    added,
                    read: visits.len(),
                    partial: false,
                },
                Err(error) => KindResult::Failed(format!("Couldn't save history: {error:#}")),
            },
            Err(error) => KindResult::Failed(error.describe(browser)),
        });
        if let Some(job) = self
            .settings_page
            .as_mut()
            .and_then(|page| page.import.job.as_mut())
        {
            job.finished = true;
            job.bookmarks = bookmarks;
            job.history = history;
        }
        cx.notify();
    }

    fn with_import(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut ImportState)) {
        if let Some(page) = &mut self.settings_page {
            edit(&mut page.import);
            cx.notify();
        }
    }

    /// A menu of `choices` at the click, the current one checked.
    fn open_choice<T: Copy + PartialEq + 'static>(
        &mut self,
        event: &ClickEvent,
        choices: Vec<(SharedString, T)>,
        current: T,
        window: &mut Window,
        cx: &mut Context<Self>,
        apply: impl Fn(&mut Shell, T, &mut Context<Shell>) + Clone + 'static,
    ) {
        let entries = choices
            .into_iter()
            .map(|(label, value)| MenuEntry::item(label, value).with_check(value == current))
            .collect();
        self.open_menu(
            entries,
            event.position(),
            window,
            cx,
            move |shell, value, _, cx| apply(shell, value, cx),
        );
    }

    pub(super) fn render_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(page) = &self.settings_page else {
            return div().into_any_element();
        };
        let theme = Theme::of(cx);

        div()
            .id("settings-page")
            .track_focus(&page.focus_handle)
            .key_context(KEY_CONTEXT)
            .on_action(
                cx.listener(|this, _: &CloseSettings, window, cx| this.close_settings(window, cx)),
            )
            .size_full()
            .flex()
            .flex_row()
            .overflow_hidden()
            .rounded(theme.metrics.radius_frame)
            .bg(theme.colors.surface)
            .border_1()
            .border_color(self.palette(cx).frame_border)
            .child(self.render_settings_nav(page, cx))
            .child(self.render_settings_content(page, cx))
            .into_any_element()
    }

    fn render_settings_nav(&self, page: &SettingsPage, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx);
        let metrics = &theme.metrics;
        let hover_bg = theme.colors.surface_hover;
        let searching = !page.query.is_empty();
        let items: Vec<AnyElement> = Section::ALL
            .into_iter()
            .map(|section| {
                let current = !searching && page.section == section;
                div()
                    .id(SharedString::from(format!(
                        "settings-nav-{}",
                        section.title()
                    )))
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .px(px(10.))
                    .py(px(8.))
                    .my(px(1.))
                    .rounded(metrics.radius_md)
                    .text_size(px(NAV_TEXT))
                    .map(|item| {
                        if current {
                            item.bg(theme.colors.surface_active)
                                .text_color(theme.colors.text)
                        } else {
                            item.text_color(theme.colors.text_muted)
                                .hover(move |style| style.bg(hover_bg))
                        }
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(page) = &mut this.settings_page {
                            page.section = section;
                            page.query.clear();
                            page.search
                                .update(cx, |input, cx| input.set_value("", window, cx));
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .justify_center()
                            .w(metrics.favicon_size)
                            .text_size(metrics.nav_glyph * 1.2)
                            .child(section.icon()),
                    )
                    .child(section.title())
                    .into_any_element()
            })
            .collect();

        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w(px(NAV_WIDTH))
            .h_full()
            .px(px(8.))
            .border_r_1()
            .border_color(theme.colors.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(metrics.title_bar_height + px(8.))
                    .mt(px(4.))
                    .mb(px(6.))
                    .pl(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(PAGE_TITLE),
                    )
                    .child(
                        div()
                            .id("settings-close")
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(metrics.nav_button_size)
                            .rounded(metrics.radius_md)
                            .text_size(metrics.nav_glyph)
                            .text_color(theme.colors.text_muted)
                            .hover(move |style| style.bg(hover_bg))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.close_settings(window, cx)),
                            )
                            .tooltip(tooltip("Back to browsing"))
                            .child(icons::CLOSE),
                    ),
            )
            .child(
                div()
                    .mb(px(10.))
                    .child(Input::new(&page.search).small().aria_label(SEARCH)),
            )
            .child(
                div()
                    .id("settings-nav")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .mt(px(10.))
                            .mb(px(7.))
                            .px(px(11.))
                            .text_size(px(NAV_GROUP))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.colors.text_faint)
                            .child("BROWSER"),
                    )
                    .children(items),
            )
            .into_any_element()
    }

    fn render_settings_content(&self, page: &SettingsPage, cx: &mut Context<Self>) -> AnyElement {
        let muted = Theme::of(cx).colors.text_muted;
        let query = page.query.to_lowercase();
        let (title, description, groups): (&str, Option<&str>, Vec<Group>) = if query.is_empty() {
            (
                page.section.title(),
                Some(page.section.description()),
                self.section_groups(page.section, page, cx),
            )
        } else {
            let found = Section::ALL
                .into_iter()
                .filter_map(|section| {
                    let rows: Vec<Row> = self
                        .section_groups(section, page, cx)
                        .into_iter()
                        .flat_map(|group| group.rows)
                        .filter(|row| row.matches(&query))
                        .collect();
                    (!rows.is_empty()).then_some(Group {
                        title: section.title(),
                        note: None,
                        rows,
                    })
                })
                .collect();
            ("Search results", None, found)
        };
        let body: AnyElement = if groups.is_empty() {
            empty_state(
                icons::SEARCH,
                "No settings found",
                "Try another word, or clear the search.",
                cx,
            )
        } else {
            div()
                .flex()
                .flex_col()
                .children(groups.into_iter().map(|group| render_group(group, cx)))
                .into_any_element()
        };

        div()
            .id("settings-scroll")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .child(
                div()
                    .w_full()
                    .max_w(px(PAGE_MAX_WIDTH))
                    .mx_auto()
                    .pt(px(PAGE_PAD_TOP))
                    .px(px(PAGE_PAD_X))
                    .pb(px(PAGE_PAD_BOTTOM))
                    .child(
                        div()
                            .mb(px(34.))
                            .child(
                                div()
                                    .text_size(px(HEADING))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title.to_string()),
                            )
                            .children(description.map(|text| {
                                div()
                                    .mt(px(8.))
                                    .text_size(px(13.))
                                    .text_color(muted)
                                    .child(text)
                            })),
                    )
                    .child(body),
            )
            .into_any_element()
    }

    fn section_groups(
        &self,
        section: Section,
        page: &SettingsPage,
        cx: &mut Context<Self>,
    ) -> Vec<Group> {
        match section {
            Section::General => vec![
                self.default_browser_group(page, cx),
                self.search_group(cx),
                self.downloads_group(cx),
                self.import_group(page, cx),
            ],
            Section::Appearance => vec![self.colors_group(cx), self.layout_group(cx)],
            Section::Tabs => vec![self.tabs_group(cx)],
            Section::Privacy => vec![
                self.browsing_data_group(page, cx),
                self.site_permissions_group(cx),
            ],
            Section::Extensions => self.extensions_groups(cx),
            Section::Keyboard => vec![self.shortcuts_group(page, cx)],
            Section::About => vec![about_group()],
        }
    }

    fn default_browser_group(&self, page: &SettingsPage, cx: &mut Context<Self>) -> Group {
        let row = Row::new("Default browser");
        let row = if crate::default_browser::is_default() {
            row.describe(format!(
                "{} is your default browser.",
                browser_core::APP_NAME
            ))
        } else {
            row.describe(page.default_browser_error.clone().unwrap_or_else(|| {
                "Windows asks you to confirm the choice in its Default apps settings.".into()
            }))
            .control(
                chip("make-default", "Make default\u{2026}", false, cx)
                    .on_click(cx.listener(|this, _, _, cx| this.make_default_browser(cx))),
            )
        };
        Group {
            title: "Default browser",
            note: None,
            rows: vec![row],
        }
    }

    fn search_group(&self, cx: &mut Context<Self>) -> Group {
        let current = self.settings.engine();
        let control = dropdown("engine", current.name, cx).on_click(cx.listener(
            move |this, event: &ClickEvent, window, cx| {
                let choices = SearchEngine::ALL
                    .into_iter()
                    .map(|engine| (engine.name.into(), engine.name))
                    .collect();
                this.open_choice(
                    event,
                    choices,
                    current.name,
                    window,
                    cx,
                    |shell, name, cx| {
                        shell.update_settings(|s| s.search_engine = name.to_string(), cx)
                    },
                );
            },
        ));
        Group {
            title: "Search",
            note: None,
            rows: vec![
                Row::new("Search engine")
                    .describe("Used for searches typed in the address bar.")
                    .control(control),
            ],
        }
    }

    fn downloads_group(&self, cx: &mut Context<Self>) -> Group {
        let path: SharedString = self
            .effective_downloads()
            .map_or("Not set".into(), |path| path.display().to_string().into());
        let custom = self.settings.downloads_folder.is_some();
        let controls = div()
            .flex()
            .gap(px(8.))
            .when(custom, |controls| {
                controls.child(
                    chip("downloads-reset", "Use Downloads", false, cx).on_click(cx.listener(
                        |this, _, _, cx| this.update_settings(|s| s.downloads_folder = None, cx),
                    )),
                )
            })
            .child(
                chip("downloads-change", "Change\u{2026}", false, cx).on_click(
                    cx.listener(|this, _, window, cx| this.choose_downloads_folder(window, cx)),
                ),
            );
        Group {
            title: "Downloads",
            note: None,
            rows: vec![
                Row::new("Downloads folder")
                    .describe(path)
                    .control(controls),
            ],
        }
    }

    fn import_group(&self, page: &SettingsPage, cx: &mut Context<Self>) -> Group {
        let theme = Theme::of(cx);
        let state = &page.import;
        if let Some(job) = &state.job {
            let name = job.browser.name();
            let heading = if job.finished {
                format!("Imported from {name}")
            } else {
                format!("Importing from {name}")
            };
            let mut row = Row::new(heading);
            if job.finished {
                row = row.control(chip("import-done", "Done", false, cx).on_click(
                    cx.listener(|this, _, _, cx| this.with_import(cx, |state| state.job = None)),
                ));
            }
            let lines: Vec<AnyElement> = [("Bookmarks", &job.bookmarks), ("History", &job.history)]
                .into_iter()
                .filter_map(|(kind, result)| Some((kind, result.as_ref()?)))
                .map(|(kind, result)| {
                    let (mark, color) = match result {
                        KindResult::Reading => ("\u{2022}", theme.colors.text_faint),
                        KindResult::Done { partial: true, .. } | KindResult::Failed(_) => {
                            ("!", theme.colors.warning)
                        }
                        KindResult::Done { .. } => ("\u{2713}", theme.colors.success),
                    };
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .text_size(px(LABEL))
                        .child(div().w(px(14.)).text_color(color).child(mark))
                        .child(div().w(px(80.)).child(kind))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_color(theme.colors.text_muted)
                                .child(result.text()),
                        )
                        .into_any_element()
                })
                .collect();
            row = row.below(div().flex().flex_col().gap(px(6.)).children(lines));
            return Group {
                title: "Import",
                note: None,
                rows: vec![row],
            };
        }

        let Some(sources) = &state.sources else {
            return Group {
                title: "Import",
                note: None,
                rows: vec![
                    Row::new("Import browser data")
                        .describe("Looking for other browsers on this device\u{2026}"),
                ],
            };
        };
        if sources.is_empty() {
            return Group {
                title: "Import",
                note: None,
                rows: vec![
                    Row::new("Import browser data")
                        .describe("No other browsers found on this device."),
                ],
            };
        }
        let source = &sources[state.chosen.min(sources.len() - 1)];
        let profile = &source.profiles[state.profile.min(source.profiles.len() - 1)];
        let can_bookmarks = profile.bookmarks;
        let can_history = profile.history;
        let ready = (state.bookmarks && can_bookmarks) || (state.history && can_history);
        let hover_bg = theme.colors.surface_hover;
        let chips: Vec<AnyElement> = sources
            .iter()
            .enumerate()
            .map(|(index, candidate)| {
                let checked = index == state.chosen;
                let text = theme.colors.text;
                div()
                    .id(("import-source", index))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(34.))
                    .pl(px(9.))
                    .pr(px(14.))
                    .rounded(theme.metrics.radius_md)
                    .text_size(px(LABEL))
                    .font_weight(FontWeight::MEDIUM)
                    .map(|chip| {
                        if checked {
                            chip.border(px(1.5))
                                .border_color(text.opacity(0.55))
                                .bg(theme.colors.surface_active)
                                .text_color(text)
                        } else {
                            chip.border_1()
                                .border_color(theme.colors.border)
                                .text_color(theme.colors.text_muted)
                                .hover(move |style| style.bg(hover_bg).text_color(text))
                        }
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.with_import(cx, |state| {
                            state.chosen = index;
                            state.profile = 0;
                        })
                    }))
                    .child(
                        img(browser_mark(candidate.browser))
                            .size(px(20.))
                            .flex_shrink_0(),
                    )
                    .child(candidate.browser.name())
                    .into_any_element()
            })
            .collect();

        let mut rows = vec![
            Row::new("Import browser data")
                .describe("Bring bookmarks and history from the browser you used before.")
                .control(
                    chip("import-start", "Import", true, cx)
                        .disabled(!ready)
                        .on_click(cx.listener(|this, _, window, cx| this.start_import(window, cx))),
                )
                .below(div().flex().flex_wrap().gap(px(8.)).children(chips)),
        ];
        if source.profiles.len() > 1 {
            let names: Vec<(SharedString, usize)> = source
                .profiles
                .iter()
                .enumerate()
                .map(|(index, profile)| (profile.name.clone().into(), index))
                .collect();
            let current = state.profile;
            rows.push(Row::new("Profile").control(
                dropdown("import-profile", profile.name.clone(), cx).on_click(cx.listener(
                    move |this, event: &ClickEvent, window, cx| {
                        this.open_choice(
                            event,
                            names.clone(),
                            current,
                            window,
                            cx,
                            |shell, index, cx| shell.with_import(cx, |state| state.profile = index),
                        )
                    },
                )),
            ));
        }
        let kinds = div()
            .flex()
            .gap(px(16.))
            .when(can_bookmarks, |kinds| {
                kinds.child(
                    checkbox("import-bookmarks", "Bookmarks", state.bookmarks, cx).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.with_import(cx, |state| state.bookmarks = !state.bookmarks)
                        }),
                    ),
                )
            })
            .when(can_history, |kinds| {
                kinds.child(
                    checkbox("import-history", "History", state.history, cx).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.with_import(cx, |state| state.history = !state.history)
                        },
                    )),
                )
            });
        rows.push(Row::new("What to bring").control(kinds));
        Group {
            title: "Import",
            note: Some("Bookmarks become pinned tabs in the current space, in their folders."),
            rows,
        }
    }

    fn colors_group(&self, cx: &mut Context<Self>) -> Group {
        let theme = Theme::of(cx);
        let ring = theme.colors.text;
        let swatch_size = theme.metrics.nav_button_size - theme.metrics.radius_sm * 2.;
        let swatches: Vec<AnyElement> = ACCENT_PRESETS
            .into_iter()
            .map(|color| {
                let selected = self.settings.accent == color;
                div()
                    .id(("accent", color as usize))
                    .size(swatch_size)
                    .rounded_full()
                    .bg(rgb(color))
                    .border_2()
                    .border_color(if selected {
                        ring
                    } else {
                        gpui::transparent_black()
                    })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.update_settings(|s| s.accent = color, cx)
                    }))
                    .into_any_element()
            })
            .collect();
        Group {
            title: "Colors",
            note: None,
            rows: vec![
                Row::new("Accent")
                    .describe(
                        "Used for selection and highlights outside spaces. Spaces keep their own colors.",
                    )
                    .control(div().flex().flex_wrap().gap(px(6.)).children(swatches)),
            ],
        }
    }

    fn layout_group(&self, cx: &mut Context<Self>) -> Group {
        let hide = self.settings.hide_title_bar;
        let favorites = self.settings.favorites_bar;
        let in_sidebar = self.settings.site_title_in_sidebar;
        let place = if in_sidebar { "Sidebar" } else { "Title bar" };
        Group {
            title: "Layout",
            note: None,
            rows: vec![
                Row::new("Hide title bar")
                    .describe(
                        "The title bar slides in when you move the pointer to the top edge of the window.",
                    )
                    .control(switch("hide-title-bar", hide, cx).on_click(cx.listener(
                        move |this, _, window, cx| {
                            this.update_settings(|s| s.hide_title_bar = !hide, cx);
                            this.sync_hover_poll(window, cx);
                        },
                    ))),
                Row::new("Website name")
                    .describe("Where the current website's name and address are shown.")
                    .control(dropdown("site-title-place", place, cx).on_click(cx.listener(
                        move |this, event: &ClickEvent, window, cx| {
                            let choices =
                                vec![("Title bar".into(), false), ("Sidebar".into(), true)];
                            this.open_choice(
                                event,
                                choices,
                                in_sidebar,
                                window,
                                cx,
                                |shell, sidebar, cx| {
                                    shell.update_settings(|s| s.site_title_in_sidebar = sidebar, cx)
                                },
                            );
                        },
                    ))),
                Row::new("Favorites bar")
                    .describe(
                        "Shows pinned tabs and folders in a bar under the title bar instead of the sidebar.",
                    )
                    .control(switch("favorites-bar", favorites, cx).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.update_settings(|s| s.favorites_bar = !favorites, cx)
                        },
                    ))),
            ],
        }
    }

    fn tabs_group(&self, cx: &mut Context<Self>) -> Group {
        let archive = self.settings.archive_after_secs;
        let peek = self.settings.peek_modifier;
        Group {
            title: "Tabs",
            note: None,
            rows: vec![
                Row::new("Archive idle tabs after")
                    .describe("Unpinned tabs you haven't opened for this long move to the archive.")
                    .control(
                        dropdown("archive-after", archive_label(archive), cx).on_click(
                            cx.listener(move |this, event: &ClickEvent, window, cx| {
                                let choices = ARCHIVE_PRESETS_SECS
                                    .into_iter()
                                    .map(|secs| (archive_label(secs).into(), secs))
                                    .collect();
                                this.open_choice(
                                    event,
                                    choices,
                                    archive,
                                    window,
                                    cx,
                                    |shell, secs, cx| {
                                        shell.update_settings(|s| s.archive_after_secs = secs, cx)
                                    },
                                );
                            }),
                        ),
                    ),
                Row::new("Peek key")
                    .describe(
                        "Hold it while clicking a link to preview the page. Applies to pages loaded after the change.",
                    )
                    .control(dropdown("peek-key", peek.label(), cx).on_click(cx.listener(
                        move |this, event: &ClickEvent, window, cx| {
                            let choices = PeekModifier::ALL
                                .into_iter()
                                .map(|modifier| (modifier.label().into(), modifier))
                                .collect();
                            this.open_choice(
                                event,
                                choices,
                                peek,
                                window,
                                cx,
                                |shell, modifier, cx| {
                                    shell.update_settings(|s| s.peek_modifier = modifier, cx)
                                },
                            );
                        },
                    ))),
            ],
        }
    }

    fn browsing_data_group(&self, page: &SettingsPage, cx: &mut Context<Self>) -> Group {
        let theme = Theme::of(cx);
        let mut row = Row::new("Clear browsing data").describe(if page.cleared {
            "Browsing data cleared."
        } else {
            "History, cookies, cached files, site storage, saved passwords and site permissions."
        });
        if page.confirm_clear {
            row = row.below(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(LABEL))
                            .text_color(theme.colors.warning)
                            .child(
                                "This deletes history, cookies (signing you out of sites), cached files, site storage, saved passwords and site permissions.",
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                chip("clear-confirm", "Clear everything", true, cx).on_click(
                                    cx.listener(|this, _, _, cx| this.clear_browsing_data(cx)),
                                ),
                            )
                            .child(chip("clear-cancel", "Cancel", false, cx).on_click(
                                cx.listener(|this, _, _, cx| {
                                    if let Some(page) = &mut this.settings_page {
                                        page.confirm_clear = false;
                                    }
                                    cx.notify();
                                }),
                            )),
                    ),
            );
        } else {
            row = row.control(chip("clear-start", "Clear\u{2026}", false, cx).on_click(
                cx.listener(|this, _, _, cx| {
                    if let Some(page) = &mut this.settings_page {
                        page.confirm_clear = true;
                        page.cleared = false;
                    }
                    cx.notify();
                }),
            ));
        }
        Group {
            title: "Browsing data",
            note: Some(
                "History, tabs, settings and site permissions are stored unencrypted in your Windows user profile, protected by your account.",
            ),
            rows: vec![row],
        }
    }

    fn site_permissions_group(&self, cx: &mut Context<Self>) -> Group {
        let mut rows = Vec::new();
        for (origin, decisions) in self.permissions.sites() {
            for (permission, decision) in decisions {
                let origin_key = origin.to_string();
                let permission_key = permission.clone();
                let decision_text = match decision {
                    browser_core::permissions::Decision::Allow => "Allowed",
                    browser_core::permissions::Decision::Block => "Blocked",
                };
                rows.push(
                    Row::new(origin.to_string())
                        .describe(format!(
                            "{} \u{00b7} {decision_text}",
                            permission.replace('-', " ")
                        ))
                        .control(
                            chip(
                                SharedString::from(format!("revoke-{origin}-{permission}")),
                                "Reset",
                                false,
                                cx,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.permissions.revoke(&origin_key, &permission_key);
                                    this.save_permissions();
                                    cx.notify();
                                },
                            )),
                        ),
                );
            }
        }
        if rows.is_empty() {
            rows.push(Row::new("No site has asked for a permission yet."));
        }
        Group {
            title: "Site permissions",
            note: Some("Reset a decision and the site asks again next time."),
            rows,
        }
    }

    fn extensions_groups(&self, cx: &mut Context<Self>) -> Vec<Group> {
        let theme = Theme::of(cx);
        let hover_bg = theme.colors.surface_hover;
        let mut rows: Vec<Row> =
            self.extensions
                .installed()
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let folder = item.folder.clone();
                    let enabled = item.enabled;
                    let has_popup = item
                        .manifest
                        .action
                        .as_ref()
                        .and_then(|action| action.default_popup.as_deref())
                        .is_some();
                    let toggle_folder = folder.clone();
                    let popup_folder = folder.clone();
                    let controls = div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .when(enabled && has_popup, |controls| {
                            controls.child(
                                chip(("extension-popup", index), "Popup", false, cx).on_click(
                                    cx.listener(move |this, _, window, cx| {
                                        this.open_extension_popup(&popup_folder, window, cx)
                                    }),
                                ),
                            )
                        })
                        .child(
                            div()
                                .id(SharedString::from(format!("extension-remove-{index}")))
                                .px(theme.metrics.radius_md)
                                .rounded(theme.metrics.radius_sm)
                                .text_color(theme.colors.text_muted)
                                .hover(move |style| style.bg(hover_bg))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.uninstall_extension(folder.clone(), window, cx)
                                }))
                                .child("Remove"),
                        )
                        .child(switch(("extension-toggle", index), enabled, cx).on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.set_extension_enabled(
                                    toggle_folder.clone(),
                                    !enabled,
                                    window,
                                    cx,
                                )
                            }),
                        ));
                    Row::new(item.manifest.name.clone())
                        .describe(format!(
                            "{} \u{00b7} {}",
                            item.manifest.version,
                            item.manifest.generation()
                        ))
                        .control(controls)
                })
                .collect();
        if rows.is_empty() {
            rows.push(Row::new("No extensions installed."));
        }
        vec![
            Group {
                title: "Installed",
                note: Some("Content scripts apply after a reload."),
                rows,
            },
            Group {
                title: "Developer",
                note: None,
                rows: vec![
                    Row::new("Add unpacked extension")
                        .describe("Load an extension from a folder on this computer.")
                        .control(
                            chip("extension-add", "Choose folder\u{2026}", false, cx).on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.choose_extension_folder(window, cx)
                                }),
                            ),
                        ),
                    Row::new("Extensions folder")
                        .describe("Where installed extensions are kept.")
                        .control(chip("extensions-folder", "Open", false, cx).on_click(
                            cx.listener(|this, _, _, _| {
                                crate::native::open_path(&this.paths.extensions())
                            }),
                        )),
                ],
            },
        ]
    }

    fn shortcuts_group(&self, page: &SettingsPage, cx: &mut Context<Self>) -> Group {
        let theme = Theme::of(cx);
        let mut rows: Vec<Row> = page
            .shortcuts
            .iter()
            .map(|field| {
                Row::new(field.label.clone()).control(
                    div().w(px(SHORTCUT_FIELD_WIDTH)).child(
                        Input::new(&field.input)
                            .small()
                            .aria_label(field.label.clone()),
                    ),
                )
            })
            .collect();
        if let Some(error) = page.error.clone() {
            rows.insert(
                0,
                Row::new("Shortcut not saved").below(
                    div()
                        .text_size(px(LABEL))
                        .text_color(theme.colors.danger)
                        .child(error),
                ),
            );
        }
        Group {
            title: "Shortcuts",
            note: Some(
                "Type keys like ctrl-shift-k and press Enter. Clear a field to restore its default.",
            ),
            rows,
        }
    }
}

fn about_group() -> Group {
    let runtime = webview_host::runtime_version().unwrap_or_else(|| "Not found".into());
    Group {
        title: browser_core::APP_NAME,
        note: Some(ICON_CREDIT),
        rows: vec![
            Row::new("Version").describe(env!("CARGO_PKG_VERSION")),
            Row::new("WebView2 runtime").describe(runtime),
        ],
    }
}

fn render_group(group: Group, cx: &Context<Shell>) -> AnyElement {
    let theme = Theme::of(cx);
    let rows: Vec<AnyElement> = group
        .rows
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            div()
                .relative()
                .flex()
                .flex_col()
                .justify_center()
                .gap(px(12.))
                .px(px(18.))
                .py(px(14.))
                .min_h(px(ROW_MIN_HEIGHT))
                .when(index > 0, |line| {
                    line.child(
                        div()
                            .absolute()
                            .top_0()
                            .left(px(HAIRLINE_INSET))
                            .right_0()
                            .h(px(1.))
                            .bg(theme.colors.border),
                    )
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(24.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .min_w_0()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(ROW_TITLE))
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(row.title),
                                )
                                .children(row.description.map(|text| {
                                    div()
                                        .max_w(px(440.))
                                        .text_size(px(LABEL))
                                        .text_color(theme.colors.text_muted)
                                        .child(text)
                                })),
                        )
                        .children(row.control.map(|control| {
                            div()
                                .flex_shrink_0()
                                .max_w(gpui::relative(0.58))
                                .child(control)
                        })),
                )
                .children(row.below)
                .into_any_element()
        })
        .collect();
    div()
        .mb(px(34.))
        .child(
            div()
                .mb(px(10.))
                .px(px(16.))
                .text_size(px(GROUP_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .child(group.title),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .overflow_hidden()
                .rounded(theme.metrics.radius_frame)
                .bg(theme.colors.surface_raised)
                .shadow_sm()
                .children(rows),
        )
        .children(group.note.map(|note| {
            div()
                .mt(px(8.))
                .mx(px(16.))
                .text_size(px(LABEL))
                .text_color(theme.colors.text_muted)
                .child(note)
        }))
        .into_any_element()
}

/// The browser's own logo, from Zephium's bundled artwork (see
/// `assets/browsers/SOURCES.md`).
fn browser_mark(browser: SourceBrowser) -> std::sync::Arc<gpui::Image> {
    use gpui::ImageFormat::{Png, Svg};
    let (format, bytes): (_, &[u8]) = match browser {
        SourceBrowser::Chrome => (Svg, include_bytes!("../../assets/browsers/chrome.svg")),
        SourceBrowser::Edge => (Svg, include_bytes!("../../assets/browsers/edge.svg")),
        SourceBrowser::Brave => (Svg, include_bytes!("../../assets/browsers/brave.svg")),
        SourceBrowser::Vivaldi => (Svg, include_bytes!("../../assets/browsers/vivaldi.svg")),
        SourceBrowser::Opera => (Svg, include_bytes!("../../assets/browsers/opera.svg")),
        SourceBrowser::OperaGx => (Svg, include_bytes!("../../assets/browsers/opera-gx.svg")),
        SourceBrowser::Chromium => (Png, include_bytes!("../../assets/browsers/chromium.png")),
        SourceBrowser::Helium => (Png, include_bytes!("../../assets/browsers/helium.png")),
        SourceBrowser::Firefox => (Svg, include_bytes!("../../assets/browsers/firefox.svg")),
        SourceBrowser::Zen => (Svg, include_bytes!("../../assets/browsers/zen.svg")),
        SourceBrowser::Floorp => (Svg, include_bytes!("../../assets/browsers/floorp.svg")),
        SourceBrowser::LibreWolf => (Svg, include_bytes!("../../assets/browsers/librewolf.svg")),
        SourceBrowser::Waterfox => (Svg, include_bytes!("../../assets/browsers/waterfox.svg")),
    };
    std::sync::Arc::new(gpui::Image::from_bytes(format, bytes.to_vec()))
}

fn switch(id: impl Into<ElementId>, on: bool, cx: &Context<Shell>) -> Stateful<Div> {
    let id = id.into();
    div().id(id.clone()).flex_shrink_0().cursor_pointer().child(
        Switch::new(id)
            .checked(on)
            .color(Theme::of(cx).colors.accent),
    )
}

fn checkbox(id: &'static str, label: &'static str, on: bool, cx: &Context<Shell>) -> Stateful<Div> {
    let theme = Theme::of(cx);
    let accent = theme.colors.accent;
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(8.))
        .cursor_pointer()
        .text_size(px(LABEL))
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(CHECKBOX))
                .rounded(theme.metrics.radius_sm)
                .border_1()
                .map(|mark| {
                    if on {
                        mark.border_color(accent)
                            .bg(accent)
                            .text_color(gpui::white())
                    } else {
                        mark.border_color(theme.colors.text_faint)
                    }
                })
                .text_size(theme.metrics.icon_glyph)
                .when(on, |mark| mark.child(icons::CHECK)),
        )
        .child(label)
}

fn chip(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    _: &Context<Shell>,
) -> Button {
    let button = Button::new(id).label(label).small();
    if selected {
        button.primary()
    } else {
        button.outline()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_presets_have_readable_labels() {
        let labels: Vec<String> = ARCHIVE_PRESETS_SECS
            .into_iter()
            .map(archive_label)
            .collect();
        assert_eq!(labels, ["1 hour", "12 hours", "1 day", "1 week", "Never"]);
    }

    #[test]
    fn search_matches_titles_and_descriptions() {
        let row = Row::new("Peek key").describe("Hold it while clicking a link");
        assert!(row.matches("peek"));
        assert!(row.matches("clicking"));
        assert!(!row.matches("download"));
    }

    #[test]
    fn import_results_say_what_happened() {
        let done = |added, read, partial| KindResult::Done {
            added,
            read,
            partial,
        };
        assert_eq!(done(3, 5, false).text(), "3 added");
        assert_eq!(done(0, 5, false).text(), "Already here");
        assert_eq!(done(0, 0, false).text(), "Nothing to import");
        assert!(done(500, 900, true).text().starts_with("Import stopped"));
    }
}
