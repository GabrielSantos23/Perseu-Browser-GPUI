use std::collections::HashSet;

use browser_core::command_bar::{Candidate, Group};
use browser_core::tabs::{OpenPosition, Tab};
use gpui::{
    AnyWindowHandle, AppContext, Bounds, Context, Entity, Pixels, Subscription, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, point, size,
};

use raw_window_handle::HasWindowHandle;

use super::Shell;
use crate::command_bar::{Command, CommandBar, CommandBarEvent, Target};
use crate::native;
use crate::page;
use crate::theme::Theme;

const WIDTH_SHARE_OF_CONTENT: f32 = 0.6;
/// How many sites "Frequently used" lists before anything is typed.
/// Sites offered to the frequent group; the ranker shows the top few that match.
const FREQUENT_SITES: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenIn {
    NewTab,
    CurrentTab,
}

pub(super) struct OpenCommandBar {
    window: AnyWindowHandle,
    open_in: OpenIn,
    _events: Subscription,
}

impl Shell {
    pub(super) fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let remembered = self.command_query.clone();
        self.open_command_bar(OpenIn::NewTab, remembered, window, cx);
    }

    pub(super) fn edit_current_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let url = self.active_url();
        let initial = if page::is_blank(url) {
            String::new()
        } else {
            url.to_string()
        };
        self.open_command_bar(OpenIn::CurrentTab, initial, window, cx);
    }

    fn open_command_bar(
        &mut self,
        open_in: OpenIn,
        initial_text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_command_bar(cx);
        let candidates = self.command_candidates();
        let engine = self.settings.engine();
        let palette = self.palette(cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(self.command_bar_bounds(window, cx))),
            titlebar: None,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Transparent,
            ..WindowOptions::default()
        };

        let mut bar: Option<Entity<CommandBar>> = None;
        let opened = cx.open_window(options, |popup, cx| {
            if let Ok(handle) = popup.window_handle() {
                native::remove_frame_shadow_and_border(handle.as_raw());
            }
            let view = cx.new(|cx| CommandBar::new(candidates, engine, initial_text, palette, popup, cx));
            bar = Some(view.clone());
            view
        });
        let (handle, bar) = match (opened, bar) {
            (Ok(handle), Some(bar)) => (handle, bar),
            (Err(error), _) => {
                tracing::error!("failed to open command bar: {error:#}");
                return;
            }
            (Ok(_), None) => return,
        };
        self.command_bar = Some(OpenCommandBar {
            window: handle.into(),
            open_in,
            _events: cx.subscribe_in(&bar, window, Self::on_command_bar_event),
        });
    }

    fn close_command_bar(&mut self, cx: &mut Context<Self>) {
        if let Some(open) = self.command_bar.take() {
            cx.spawn(async move |_, cx| {
                open.window
                    .update(cx, |_, popup, _| popup.remove_window())
                    .ok();
            })
            .detach();
        }
    }

    fn command_bar_bounds(&self, window: &Window, cx: &Context<Self>) -> Bounds<Pixels> {
        let metrics = &Theme::of(cx).metrics;
        let window_bounds = window.bounds();
        let content = self.content_bounds.get();
        let content = if content.size.width > Pixels::ZERO {
            content
        } else {
            Bounds::new(point(Pixels::ZERO, Pixels::ZERO), window_bounds.size)
        };
        let width = (content.size.width * WIDTH_SHARE_OF_CONTENT)
            .clamp(metrics.command_bar_min_width, metrics.command_bar_max_width)
            .min(content.size.width);
        let origin = point(
            window_bounds.origin.x + content.origin.x + (content.size.width - width) / 2.,
            window_bounds.origin.y + content.origin.y + metrics.command_bar_top_offset,
        );
        Bounds::new(origin, size(width, metrics.command_bar_height))
    }

    fn on_command_bar_event(
        &mut self,
        bar: &Entity<CommandBar>,
        event: &CommandBarEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(open) = self.command_bar.take() else {
            return;
        };
        if open.open_in == OpenIn::NewTab {
            self.command_query = match event {
                CommandBarEvent::Dismiss => bar.read(cx).query(cx),
                CommandBarEvent::Confirm(_) => String::new(),
            };
        }
        let chosen = match event {
            CommandBarEvent::Dismiss => None,
            CommandBarEvent::Confirm(target) => Some(target.clone()),
        };
        let open_in = open.open_in;
        self.dismiss_popup(open.window, window, cx, move |shell, window, cx| {
            shell.finish_command(chosen, open_in, window, cx)
        });
    }

    fn finish_command(
        &mut self,
        chosen: Option<Target>,
        open_in: OpenIn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match chosen {
            Some(target) => self.run_target(target, open_in, window, cx),
            None => {
                if let Some(view) = self.active_view() {
                    view.focus();
                }
            }
        }
        cx.notify();
    }

    fn run_target(
        &mut self,
        target: Target,
        open_in: OpenIn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match target {
            Target::Tab(id) => self.activate(id, window, cx),
            Target::Open(url) => {
                let reuse_active =
                    open_in == OpenIn::CurrentTab || page::is_blank(self.active_url());
                if reuse_active {
                    self.navigate_active(url, window, cx);
                } else {
                    let id = self.open_tab(url, OpenPosition::End, window, cx);
                    self.update_runtime(id, |runtime| runtime.focus_when_ready = true);
                }
            }
            Target::Run(command) => self.run_command(command, window, cx),
        }
    }

    fn run_command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            Command::NewSpace => self.create_space(window, cx),
            Command::CloseTab => self.close_active_tab(window, cx),
            Command::ReopenClosedTab => self.reopen_closed_tab(window, cx),
            Command::ViewArchive => {
                self.toggle_panel(super::panel_popup::Panel::Archive, window, cx)
            }
            Command::ToggleCompactMode => self.toggle_compact(window, cx),
            Command::Settings => self.open_settings(window, cx),
            Command::SplitView => self.split_tabs(window, cx),
            Command::Unsplit => self.unsplit_active(window, cx),
            Command::RotateSplit => self.rotate_active_split(cx),
            Command::Reload => {
                if let Some(view) = self.active_view() {
                    view.reload();
                }
            }
        }
    }

    fn command_candidates(&self) -> Vec<Candidate<Target>> {
        let active = self.browser.active_tab_id();
        let mut open: Vec<&Tab> = self
            .browser
            .spaces()
            .iter()
            .flat_map(|space| space.tabs.tabs())
            .chain(self.browser.essentials())
            .filter(|tab| Some(tab.id) != active && !page::is_blank(&tab.url))
            .collect();
        open.sort_by_key(|tab| std::cmp::Reverse(tab.last_active));
        let open_urls: HashSet<&str> = open.iter().map(|tab| tab.url.as_str()).collect();

        let tabs = open.iter().map(|tab| Candidate {
            group: if tab.is_pinned() {
                Group::Pinned
            } else {
                Group::Tabs
            },
            title: page::display_title(tab).to_string(),
            subtitle: tab.url.clone(),
            target: Target::Tab(tab.id),
        });
        let in_split = self.active_split().is_some();
        let can_split = self.can_split();
        let available = move |command: &Command| match command {
            Command::SplitView => can_split,
            Command::Unsplit | Command::RotateSplit => in_split,
            _ => true,
        };
        let commands = Command::ALL
            .into_iter()
            .filter(available)
            .map(|command| Candidate {
                group: Group::Commands,
                title: command.label().to_string(),
                subtitle: String::new(),
                target: Target::Run(command),
            });
        let history = self
            .history
            .entries()
            .iter()
            .filter(|entry| !open_urls.contains(entry.url.as_str()))
            .map(|entry| Candidate {
                group: Group::History,
                title: if entry.title.is_empty() {
                    entry.url.clone()
                } else {
                    entry.title.clone()
                },
                subtitle: entry.url.clone(),
                target: Target::Open(entry.url.clone()),
            });
        let frequent = self
            .history
            .frequent_sites(FREQUENT_SITES)
            .into_iter()
            .map(|entry| Candidate {
                group: Group::Frequent,
                title: if entry.title.is_empty() {
                    entry.url.clone()
                } else {
                    entry.title.clone()
                },
                subtitle: entry.url.clone(),
                target: Target::Open(entry.url.clone()),
            });
        frequent
            .chain(tabs)
            .chain(commands)
            .chain(history)
            .collect()
    }
}
