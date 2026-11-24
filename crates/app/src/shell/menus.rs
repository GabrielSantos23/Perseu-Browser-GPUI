use browser_core::browser::MAX_ESSENTIALS;
use browser_core::tabs::{OpenPosition, TabId};
use gpui::{
    AnyWindowHandle, AppContext, Bounds, Context, Entity, Pixels, Point, Subscription, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, point, size,
};
use raw_window_handle::HasWindowHandle;

use super::Shell;
use super::panel_popup::Panel;
use super::tab_ops::now;
use crate::actions;
use crate::context_menu::{ContextMenu, MENU_FADE, MenuEntry, MenuEvent, menu_height, menu_width};
use crate::icons;
use crate::native;
use crate::page;
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TabAction {
    Reload,
    Duplicate,
    CopyLink,
    Pin,
    Unpin,
    AddToEssentials,
    RemoveFromEssentials,
    SplitWithCurrent,
    Unsplit,
    MoveToSpace(usize),
    NewFolder,
    MoveToFolder(usize),
    RemoveFromFolder,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SpaceAction {
    Rename,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpaceMenu {
    Switch(usize),
    Edit,
    New,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Feature {
    Update,
    NewTab,
    NewPrivateWindow,
    ReopenClosedTab,
    History,
    Archive,
    Downloads,
    Extensions,
    ToggleSidebar,
    Fullscreen,
    Settings,
}

pub(super) struct OpenMenu {
    window: AnyWindowHandle,
    _events: Subscription,
}

impl Shell {
    pub(super) fn open_tab_menu(
        &mut self,
        id: TabId,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = self.tab_menu_entries(id);
        self.open_menu(
            entries,
            position,
            window,
            cx,
            move |shell, action, window, cx| shell.run_tab_action(id, action, window, cx),
        );
    }

    pub(super) fn open_space_menu(
        &mut self,
        index: usize,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.browser.private_shown() {
            return;
        }
        let mut entries = vec![MenuEntry::item("Rename space", SpaceAction::Rename)];
        if self.browser.spaces().len() > 1 {
            entries.push(MenuEntry::Separator);
            entries.push(MenuEntry::danger(
                "Delete space (tabs to archive)",
                SpaceAction::Delete,
            ));
        }
        self.open_menu(
            entries,
            position,
            window,
            cx,
            move |shell, action, window, cx| shell.run_space_action(index, action, window, cx),
        );
    }

    /// The footer's space button menu: switch spaces, edit the space's look,
    /// or make a new space.
    pub(super) fn open_space_switcher(
        &mut self,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.browser.private_shown() {
            return;
        }
        let active = self.browser.active_space_index();
        let mut entries: Vec<MenuEntry<SpaceMenu>> = self
            .browser
            .spaces()
            .iter()
            .enumerate()
            .map(|(index, space)| {
                MenuEntry::item(space.name.clone(), SpaceMenu::Switch(index))
                    .with_icon(icons::space_icon(&space.icon))
                    .with_check(index == active)
            })
            .collect();
        entries.push(MenuEntry::Separator);
        entries
            .push(MenuEntry::item("Edit space", SpaceMenu::Edit).with_icon(Some(icons::PALETTE)));
        entries.push(MenuEntry::item("New space", SpaceMenu::New).with_icon(Some(icons::ADD)));
        self.open_menu_above(
            entries,
            anchor,
            window,
            cx,
            move |shell, action, window, cx| match action {
                SpaceMenu::Switch(index) => shell.switch_space(index, window, cx),
                SpaceMenu::Edit => shell.open_theme_picker(anchor, true, window, cx),
                SpaceMenu::New => shell.create_space(window, cx),
            },
        );
    }

    /// The footer's features menu: the sidebar panels and settings.
    pub(super) fn open_features_menu(
        &mut self,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keys = |action: &dyn gpui::Action| crate::tooltip::keys_for(action, window);
        let private = self.browser.private_shown();
        let mut entries = Vec::new();
        if let Some(release) = &self.update {
            entries.push(MenuEntry::item(
                format!("Restart to update to {}", release.version),
                Feature::Update,
            ));
            entries.push(MenuEntry::Separator);
        }
        entries.extend([
            MenuEntry::item("New tab", Feature::NewTab).with_shortcut(keys(&actions::NewTab)),
            MenuEntry::item("New private window", Feature::NewPrivateWindow)
                .with_shortcut(keys(&actions::NewPrivateWindow)),
            MenuEntry::item("Reopen closed tab", Feature::ReopenClosedTab)
                .with_shortcut(keys(&actions::ReopenClosedTab)),
            MenuEntry::Separator,
            MenuEntry::item("History", Feature::History)
                .disabled(private)
                .with_shortcut(keys(&actions::ShowHistory))
                .with_check(self.history_page.is_some()),
            MenuEntry::item("Archived tabs", Feature::Archive).with_check(self.archive_open),
            MenuEntry::item("Downloads", Feature::Downloads).with_check(self.downloads_open),
            MenuEntry::item("Extensions", Feature::Extensions).with_check(self.extensions_open),
            MenuEntry::Separator,
            MenuEntry::item("Show/hide sidebar", Feature::ToggleSidebar)
                .with_shortcut(keys(&actions::ToggleCompactMode)),
            MenuEntry::item("Fullscreen", Feature::Fullscreen)
                .with_shortcut(keys(&actions::ToggleFullscreen))
                .with_check(self.fullscreen.is_some()),
            MenuEntry::item("Settings\u{2026}", Feature::Settings)
                .with_check(self.settings_page.is_some()),
        ]);
        self.open_menu_above(entries, anchor, window, cx, |shell, feature, window, cx| {
            match feature {
                Feature::Update => shell.install_update(window, cx),
                Feature::NewTab => shell.new_tab(window, cx),
                Feature::NewPrivateWindow => shell.new_private_window(window, cx),
                Feature::ReopenClosedTab => shell.reopen_closed_tab(window, cx),
                Feature::History => shell.toggle_history(window, cx),
                Feature::Archive => shell.toggle_panel(Panel::Archive, window, cx),
                Feature::Downloads => shell.toggle_panel(Panel::Downloads, window, cx),
                Feature::Extensions => shell.extensions_open = !shell.extensions_open,
                Feature::ToggleSidebar => shell.toggle_compact(window, cx),
                Feature::Fullscreen => shell.toggle_fullscreen(window, cx),
                Feature::Settings => shell.toggle_settings(window, cx),
            }
            cx.notify();
        });
    }

    /// Opens a menu above `anchor` (a click in the sidebar footer), kept
    /// inside the sidebar.
    fn open_menu_above<T: Copy + 'static>(
        &mut self,
        entries: Vec<MenuEntry<T>>,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
        run: impl Fn(&mut Self, T, &mut Window, &mut Context<Self>) + Clone + 'static,
    ) {
        let metrics = &Theme::of(cx).metrics;
        let height = menu_height(&entries, cx);
        let width = menu_width(&entries, cx);
        let sidebar_left = if self.compact {
            metrics.frame_inset
        } else {
            Pixels::ZERO
        };
        let min_x = sidebar_left + metrics.frame_inset;
        let max_x = (sidebar_left + self.sidebar_width - metrics.frame_inset - width).max(min_x);
        let x = (anchor.x - width / 2.).clamp(min_x, max_x);
        let y = (anchor.y - metrics.nav_button_size / 2. - metrics.radius_sm - height)
            .max(metrics.frame_inset);
        self.open_menu(entries, point(x, y), window, cx, run);
    }

    pub(super) fn open_menu<T: Copy + 'static>(
        &mut self,
        entries: Vec<MenuEntry<T>>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
        run: impl Fn(&mut Self, T, &mut Window, &mut Context<Self>) + Clone + 'static,
    ) {
        if let Some(open) = self.menu.take() {
            self.dismiss_popup(open.window, window, cx, |_, _, _| {});
        }
        if entries.is_empty() {
            return;
        }
        let window_bounds = window.bounds();
        let menu_size = size(menu_width(&entries, cx), menu_height(&entries, cx));
        let bottom = window_bounds.origin.y + window_bounds.size.height;
        let mut origin = window_bounds.origin + position;
        if origin.y + menu_size.height > bottom {
            origin.y = (bottom - menu_size.height).max(window_bounds.origin.y);
        }
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(origin.x, origin.y),
                menu_size,
            ))),
            titlebar: None,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Transparent,
            ..WindowOptions::default()
        };
        let mut menu: Option<Entity<ContextMenu<T>>> = None;
        let opened = cx.open_window(options, |popup, cx| {
            if let Ok(handle) = popup.window_handle() {
                native::remove_frame_shadow_and_border(handle.as_raw());
            }
            let view = cx.new(|cx| ContextMenu::new(entries, popup, cx));
            menu = Some(view.clone());
            view
        });
        let (handle, menu) = match (opened, menu) {
            (Ok(handle), Some(menu)) => (handle, menu),
            (Err(error), _) => {
                tracing::error!("failed to open menu: {error:#}");
                return;
            }
            (Ok(_), None) => return,
        };
        let events = cx.subscribe_in(
            &menu,
            window,
            move |shell, menu, event: &MenuEvent<T>, window, cx| {
                let Some(open) = shell.menu.take() else {
                    return;
                };
                menu.update(cx, |menu, cx| menu.close(cx));
                let chosen = match event {
                    MenuEvent::Chosen(value) => Some(*value),
                    MenuEvent::Dismiss => None,
                };
                let run = run.clone();
                shell.fade_out_popup(
                    open.window,
                    MENU_FADE,
                    window,
                    cx,
                    move |shell, window, cx| {
                        if let Some(value) = chosen {
                            run(shell, value, window, cx);
                        }
                    },
                );
            },
        );
        self.menu = Some(OpenMenu {
            window: handle.into(),
            _events: events,
        });
    }

    pub(super) fn run_space_action(
        &mut self,
        index: usize,
        action: SpaceAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            SpaceAction::Rename => {
                self.switch_space(index, window, cx);
                self.start_space_rename(window, cx);
            }
            SpaceAction::Delete => {
                if let Some(ids) = self.browser.remove_space(index, now()) {
                    for id in ids {
                        self.runtime.remove(&id);
                    }
                    self.split_selection.clear();
                    if self.browser.active_tab_id().is_some() {
                        self.show_active_tab(window, cx);
                    } else {
                        self.open_blank_tab(window, cx);
                    }
                }
            }
        }
        cx.notify();
    }

    fn tab_menu_entries(&self, id: TabId) -> Vec<MenuEntry<TabAction>> {
        let Some(tab) = self.browser.tab(id) else {
            return Vec::new();
        };
        let strip = &self.browser.space().tabs;
        let in_space = strip.get(id).is_some();
        let active = self.browser.active_tab_id();
        let can_split_with_active = active.is_some_and(|active| {
            active != id
                && in_space
                && strip.get(active).is_some()
                && strip.split_of(active).is_none()
                && strip.split_of(id).is_none()
        });

        let mut entries = vec![MenuEntry::item("Reload", TabAction::Reload)];
        if !page::is_blank(&tab.url) {
            entries.push(MenuEntry::item("Duplicate", TabAction::Duplicate));
            entries.push(MenuEntry::item("Copy link", TabAction::CopyLink));
        }
        entries.push(MenuEntry::Separator);
        if self.browser.is_essential(id) {
            entries.push(MenuEntry::item(
                "Remove from essentials",
                TabAction::RemoveFromEssentials,
            ));
        } else if in_space {
            entries.push(if tab.is_pinned() {
                MenuEntry::item("Unpin tab", TabAction::Unpin)
            } else {
                MenuEntry::item("Pin tab", TabAction::Pin)
            });
            if self.browser.essentials().len() < MAX_ESSENTIALS {
                entries.push(MenuEntry::item(
                    "Add to essentials",
                    TabAction::AddToEssentials,
                ));
            }
        }
        if in_space && tab.is_pinned() {
            entries.push(MenuEntry::item("New folder with tab", TabAction::NewFolder));
            for (index, folder) in strip.folders().iter().enumerate() {
                if tab.folder() != Some(folder.as_str()) {
                    entries.push(MenuEntry::item(
                        format!("Move to folder {folder}"),
                        TabAction::MoveToFolder(index),
                    ));
                }
            }
            if tab.folder().is_some() {
                entries.push(MenuEntry::item(
                    "Remove from folder",
                    TabAction::RemoveFromFolder,
                ));
            }
        }
        if strip.split_of(id).is_some() {
            entries.push(MenuEntry::item("Unsplit view", TabAction::Unsplit));
        } else if can_split_with_active {
            entries.push(MenuEntry::item(
                "Split with current tab",
                TabAction::SplitWithCurrent,
            ));
        }
        if in_space && !self.browser.is_essential(id) && !self.browser.private_shown() {
            let current = self.browser.active_space_index();
            for (index, space) in self.browser.spaces().iter().enumerate() {
                if index != current {
                    entries.push(MenuEntry::item(
                        format!("Move to {}", space.name),
                        TabAction::MoveToSpace(index),
                    ));
                }
            }
        }
        entries.push(MenuEntry::Separator);
        entries.push(MenuEntry::danger("Close tab", TabAction::Close));
        entries
    }

    fn run_tab_action(
        &mut self,
        id: TabId,
        action: TabAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let url = self.browser.tab(id).map(|tab| tab.url.clone());
        match action {
            TabAction::Reload => {
                if let Some(view) = self
                    .runtime
                    .get(&id)
                    .and_then(|runtime| runtime.view.clone())
                {
                    view.reload();
                }
            }
            TabAction::Duplicate => {
                if let Some(url) = url {
                    self.open_tab(url, OpenPosition::After(id), window, cx);
                }
            }
            TabAction::CopyLink => {
                if let Some(url) = url {
                    self.copy_link(url, window, cx);
                }
            }
            TabAction::Pin => {
                self.browser.space_mut().tabs.pin(id);
            }
            TabAction::Unpin => {
                self.browser.space_mut().tabs.unpin(id);
            }
            TabAction::AddToEssentials => {
                self.browser.make_essential(id, now());
            }
            TabAction::RemoveFromEssentials => {
                self.browser.remove_essential(id, usize::MAX, now());
            }
            TabAction::SplitWithCurrent => {
                if let Some(active) = self.browser.active_tab_id()
                    && self.browser.space_mut().tabs.split(active, id)
                {
                    self.show_active_tab(window, cx);
                }
            }
            TabAction::Unsplit => {
                if self.browser.space_mut().tabs.unsplit(id).is_some() {
                    self.show_active_tab(window, cx);
                }
            }
            TabAction::MoveToSpace(index) => {
                self.deny_prompts_of(id);
                if self.browser.move_to_space(id, index, now()) {
                    if self.browser.active_tab_id().is_some() {
                        self.show_active_tab(window, cx);
                    } else {
                        self.open_blank_tab(window, cx);
                    }
                }
            }
            TabAction::NewFolder => {
                let tabs = &mut self.browser.space_mut().tabs;
                let name = tabs.new_folder_name();
                tabs.set_folder(id, Some(name.clone()));
                self.start_folder_rename(name, window, cx);
            }
            TabAction::MoveToFolder(index) => {
                let tabs = &mut self.browser.space_mut().tabs;
                if let Some(folder) = tabs.folders().get(index).cloned() {
                    tabs.set_folder(id, Some(folder));
                }
            }
            TabAction::RemoveFromFolder => {
                self.browser.space_mut().tabs.set_folder(id, None);
            }
            TabAction::Close => self.close_tab(id, window, cx),
        }
        cx.notify();
    }
}
