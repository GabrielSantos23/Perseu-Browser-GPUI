//! Spaces, shared essentials and the tab archive of one window.

use serde::{Deserialize, Serialize};

use crate::session::{
    SESSION_VERSION, SavedArchivedTab, SavedLayout, SavedPinnedPage, SavedSpace, SavedSplit,
    SavedTab, Session,
};
use crate::tabs::{
    CloseOutcome, OpenPosition, PinnedPage, Section, SplitOrientation, Tab, TabId, TabStrip,
    Timestamp,
};

/// Most essentials shown in the grid.
pub const MAX_ESSENTIALS: usize = 12;
/// Default idle time before an unpinned tab is archived: 12 hours.
pub const DEFAULT_ARCHIVE_AFTER_MS: u64 = 12 * 60 * 60 * 1000;

const ARCHIVE_LIMIT: usize = 500;
const DEFAULT_SPACE_NAME: &str = "Personal";
const PRIVATE_SPACE_NAME: &str = "Private";
const PRIVATE_SPACE_ICON: &str = "incognito";
const PRIVATE_ACCENT: u32 = 0x6e5a9e;
const SPACE_ICONS: &[&str] = &[
    "home",
    "briefcase",
    "palette",
    "book-alt",
    "gamepad",
    "flask",
    "plane-alt",
    "music-alt",
    "shopping-cart",
];
/// Theme intensity of a new space (the picker's opacity slider position).
pub const DEFAULT_THEME_INTENSITY: f32 = 0.27;

/// Preset space colors, in the order "next color" cycles through.
pub const SPACE_THEMES: &[SpaceTheme] = &[
    SpaceTheme::Solid(0x7C8CFF),
    SpaceTheme::Solid(0x4CC3B5),
    SpaceTheme::Solid(0xF28C6B),
    SpaceTheme::Gradient(0xC58CFF, 0x7C8CFF),
    SpaceTheme::Solid(0x6BBF59),
    SpaceTheme::Gradient(0xE8B04B, 0xF28C6B),
];

/// Stable identifier of a space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpaceId(u64);

impl SpaceId {
    /// The raw number, for use as a UI element key.
    pub fn value(self) -> u64 {
        self.0
    }
}

/// A space's color, as 0xRRGGBB values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceTheme {
    /// One accent color.
    Solid(u32),
    /// A gradient; the first color is the accent.
    Gradient(u32, u32),
    /// A gradient of two colors with a third laid over it from the top.
    Triple(u32, u32, u32),
}

/// Background texture laid over a space's colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextureKind {
    /// No texture.
    None,
    /// Fine grain.
    #[default]
    Grain,
    /// Coarse grain.
    Sand,
    /// Diagonal weave.
    Denim,
    /// Crossed weave.
    Tweed,
}

impl TextureKind {
    /// Every texture, in the order clicking the texture dial cycles through.
    pub const ALL: [Self; 5] = [
        Self::None,
        Self::Grain,
        Self::Sand,
        Self::Denim,
        Self::Tweed,
    ];

    /// The texture `step` places after this one, wrapping around.
    pub fn cycled(self, step: isize) -> Self {
        let index = Self::ALL.iter().position(|kind| *kind == self).unwrap_or(0) as isize;
        let count = Self::ALL.len() as isize;
        Self::ALL[(index + step).rem_euclid(count) as usize]
    }
}

impl SpaceTheme {
    /// The color used for highlights and borders.
    pub fn accent(self) -> u32 {
        match self {
            Self::Solid(color) | Self::Gradient(color, _) | Self::Triple(color, _, _) => color,
        }
    }
}

/// A named set of pinned and unpinned tabs.
#[derive(Debug)]
pub struct Space {
    /// Identifier.
    pub id: SpaceId,
    /// Display name.
    pub name: String,
    /// Name of the icon shown in the switcher. Sessions from older versions
    /// may hold an emoji instead.
    pub icon: String,
    /// Icon color as 0xRRGGBB; `None` uses the text color.
    pub icon_color: Option<u32>,
    /// Accent color or gradient.
    pub theme: SpaceTheme,
    /// How strongly the theme tints the sidebar, 0 to 1.
    pub intensity: f32,
    /// Texture strength, 0 to 1 in sixteenths.
    pub texture: f32,
    /// Texture style.
    pub texture_kind: TextureKind,
    /// The theme picker's own state (dot positions and harmony), opaque to
    /// this crate, so reopening the picker shows the same dots.
    pub theme_state: Option<String>,
    /// The space's own tabs.
    pub tabs: TabStrip,
    selected_essential: Option<TabId>,
}

/// An unpinned tab that was archived for being idle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivedTab {
    /// URL when archived.
    pub url: String,
    /// Title when archived.
    pub title: String,
    /// When it was archived.
    pub archived_at: Timestamp,
    /// Space it came from.
    pub space: SpaceId,
}

/// Where a tab lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabHome {
    /// In the shared essentials grid.
    Essentials,
    /// In the space at this index.
    Space(usize),
    /// In the private window.
    Private,
}

/// The browser model of one window.
#[derive(Debug)]
pub struct Browser {
    spaces: Vec<Space>,
    active_space: usize,
    essentials: Vec<Tab>,
    archive: Vec<ArchivedTab>,
    next_space_id: u64,
    /// The private window's tabs, kept out of every saved list, and whether
    /// the window shows them rather than the regular space.
    private: Option<Space>,
    private_shown: bool,
    /// Idle time before unpinned tabs are archived.
    pub archive_after_ms: u64,
}

impl Default for Browser {
    fn default() -> Self {
        Self::new()
    }
}

impl Browser {
    /// Creates a browser with one space.
    pub fn new() -> Self {
        let mut browser = Self {
            spaces: Vec::new(),
            active_space: 0,
            essentials: Vec::new(),
            archive: Vec::new(),
            next_space_id: 0,
            private: None,
            private_shown: false,
            archive_after_ms: DEFAULT_ARCHIVE_AFTER_MS,
        };
        browser.add_space(DEFAULT_SPACE_NAME);
        browser
    }

    /// All spaces in switcher order.
    pub fn spaces(&self) -> &[Space] {
        &self.spaces
    }

    /// Index of the current space.
    pub fn active_space_index(&self) -> usize {
        self.active_space
    }

    /// The current space: the private window's while it is shown.
    pub fn space(&self) -> &Space {
        match &self.private {
            Some(private) if self.private_shown => private,
            _ => &self.spaces[self.active_space],
        }
    }

    /// The current space, for editing.
    pub fn space_mut(&mut self) -> &mut Space {
        match &mut self.private {
            Some(private) if self.private_shown => private,
            _ => &mut self.spaces[self.active_space],
        }
    }

    fn new_space(&mut self, name: String, preset: usize) -> Space {
        self.next_space_id += 1;
        Space {
            id: SpaceId(self.next_space_id),
            name,
            icon: SPACE_ICONS[preset % SPACE_ICONS.len()].to_string(),
            icon_color: None,
            theme: SPACE_THEMES[preset % SPACE_THEMES.len()],
            intensity: DEFAULT_THEME_INTENSITY,
            texture: 0.0,
            texture_kind: TextureKind::default(),
            theme_state: None,
            tabs: TabStrip::new(),
            selected_essential: None,
        }
    }

    /// Adds a space with the next preset icon and color and returns its index.
    pub fn add_space(&mut self, name: impl Into<String>) -> usize {
        let space = self.new_space(name.into(), self.spaces.len());
        self.spaces.push(space);
        self.spaces.len() - 1
    }

    /// Makes the space at `index` current, setting a shown private window
    /// aside. Returns false if it does not exist.
    pub fn switch_space(&mut self, index: usize) -> bool {
        if index >= self.spaces.len() {
            return false;
        }
        self.active_space = index;
        self.private_shown = false;
        true
    }

    /// Shows the private window, opening an empty one if there is none.
    /// Returns true when it was just opened.
    pub fn show_private(&mut self) -> bool {
        let opened = self.private.is_none();
        if opened {
            let mut space = self.new_space(PRIVATE_SPACE_NAME.to_string(), 0);
            space.theme = SpaceTheme::Solid(PRIVATE_ACCENT);
            space.icon = PRIVATE_SPACE_ICON.to_string();
            self.private = Some(space);
        }
        self.private_shown = true;
        opened
    }

    /// Sets the private window aside, its tabs intact, and shows the
    /// regular space again.
    pub fn show_regular(&mut self) {
        self.private_shown = false;
    }

    /// Whether the private window is in front.
    pub fn private_shown(&self) -> bool {
        self.private_shown && self.private.is_some()
    }

    /// How many tabs the private window holds, while one is open.
    pub fn private_tab_count(&self) -> Option<usize> {
        self.private.as_ref().map(|space| space.tabs.len())
    }

    /// Whether `id` is a tab of the private window.
    pub fn is_private(&self, id: TabId) -> bool {
        self.private
            .as_ref()
            .is_some_and(|space| space.tabs.get(id).is_some())
    }

    /// Pins imported bookmarks in the regular space that is current, each
    /// in its folder, skipping pages already pinned there. Returns how many
    /// were added; at most `limit`.
    pub fn import_bookmarks(
        &mut self,
        bookmarks: &[crate::import::Bookmark],
        limit: usize,
        now: Timestamp,
    ) -> usize {
        let space = &mut self.spaces[self.active_space];
        let mut added = 0;
        for bookmark in bookmarks {
            if added >= limit {
                break;
            }
            let known = space.tabs.section(Section::Pinned).any(|tab| {
                tab.pinned
                    .as_ref()
                    .is_some_and(|page| page.url == bookmark.url)
            });
            if known {
                continue;
            }
            let mut tab = Tab::new(bookmark.url.clone(), now);
            tab.title = bookmark.title.clone();
            let id = tab.id;
            space.tabs.insert(tab, Section::Pinned, usize::MAX);
            space.tabs.set_folder(id, bookmark.folder.clone());
            added += 1;
        }
        added
    }

    /// Closes the private window and returns the ids of its tabs, which are
    /// gone for good: nothing of them is archived or reopenable.
    pub fn close_private(&mut self) -> Vec<TabId> {
        self.private_shown = false;
        self.private
            .take()
            .map(|space| space.tabs.tabs().iter().map(|tab| tab.id).collect())
            .unwrap_or_default()
    }

    /// Deletes the space at `index`, moving all of its tabs to the archive so
    /// they can be reopened. The last space cannot be deleted. Returns the
    /// ids of the archived tabs.
    pub fn remove_space(&mut self, index: usize, now: Timestamp) -> Option<Vec<TabId>> {
        if self.spaces.len() <= 1 || index >= self.spaces.len() {
            return None;
        }
        let space = self.spaces.remove(index);
        if self.active_space > index || self.active_space >= self.spaces.len() {
            self.active_space = self.active_space.saturating_sub(1);
        }
        let ids = space.tabs.tabs().iter().map(|tab| tab.id).collect();
        for tab in space.tabs.tabs().iter().rev() {
            self.archive.insert(
                0,
                ArchivedTab {
                    url: tab.url.clone(),
                    title: tab.title.clone(),
                    archived_at: now,
                    space: space.id,
                },
            );
        }
        self.archive.truncate(ARCHIVE_LIMIT);
        Some(ids)
    }

    /// Moves a tab of the current space to the end of its section in the
    /// space at `index`. Returns false for essentials, unknown tabs, private
    /// tabs and the current space itself.
    pub fn move_to_space(&mut self, id: TabId, index: usize, now: Timestamp) -> bool {
        if self.private_shown()
            || self.is_private(id)
            || index == self.active_space
            || index >= self.spaces.len()
        {
            return false;
        }
        let Some(tab) = self.space_mut().tabs.remove(id, now) else {
            return false;
        };
        let section = if tab.is_pinned() {
            Section::Pinned
        } else {
            Section::Unpinned
        };
        self.spaces[index].tabs.insert(tab, section, usize::MAX);
        true
    }

    /// Gives the current space the next preset icon.
    pub fn cycle_space_icon(&mut self) {
        let space = self.space_mut();
        let current = SPACE_ICONS.iter().position(|icon| *icon == space.icon);
        let next = current.map_or(0, |index| (index + 1) % SPACE_ICONS.len());
        space.icon = SPACE_ICONS[next].to_string();
    }

    /// Gives the current space the next preset color.
    pub fn cycle_space_theme(&mut self) {
        let space = self.space_mut();
        let current = SPACE_THEMES.iter().position(|theme| *theme == space.theme);
        let next = current.map_or(0, |index| (index + 1) % SPACE_THEMES.len());
        space.theme = SPACE_THEMES[next];
    }

    /// The shared essentials, in grid order.
    pub fn essentials(&self) -> &[Tab] {
        &self.essentials
    }

    /// Idle tabs archived so far, newest first.
    pub fn archive(&self) -> &[ArchivedTab] {
        &self.archive
    }

    /// The tab shown in the current space: a selected essential, or else the
    /// space's selected tab.
    pub fn active_tab_id(&self) -> Option<TabId> {
        let space = self.space();
        space.selected_essential.or(space.tabs.active_id())
    }

    /// The tab shown in the current space.
    pub fn active_tab(&self) -> Option<&Tab> {
        self.active_tab_id().and_then(|id| self.tab(id))
    }

    /// Whether the given tab is an essential.
    pub fn is_essential(&self, id: TabId) -> bool {
        self.essentials.iter().any(|tab| tab.id == id)
    }

    /// Finds a tab in the essentials or any space.
    pub fn tab(&self, id: TabId) -> Option<&Tab> {
        self.essentials
            .iter()
            .find(|tab| tab.id == id)
            .or_else(|| self.spaces.iter().find_map(|space| space.tabs.get(id)))
            .or_else(|| self.private.as_ref().and_then(|space| space.tabs.get(id)))
    }

    /// Finds a tab in the essentials or any space, for editing.
    pub fn tab_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        if let Some(tab) = self.essentials.iter_mut().find(|tab| tab.id == id) {
            return Some(tab);
        }
        self.spaces
            .iter_mut()
            .chain(self.private.as_mut())
            .find_map(|space| space.tabs.get_mut(id))
    }

    /// Where a tab lives.
    pub fn home_of(&self, id: TabId) -> Option<TabHome> {
        if self.is_essential(id) {
            return Some(TabHome::Essentials);
        }
        if self.is_private(id) {
            return Some(TabHome::Private);
        }
        self.spaces
            .iter()
            .position(|space| space.tabs.get(id).is_some())
            .map(TabHome::Space)
    }

    /// Opens an unpinned tab in the current space and selects it.
    pub fn open(
        &mut self,
        url: impl Into<String>,
        position: OpenPosition,
        now: Timestamp,
    ) -> TabId {
        self.adopt(Tab::new(url, now), position, now)
    }

    /// Adds an existing tab to the current space and selects it.
    pub fn adopt(&mut self, tab: Tab, position: OpenPosition, now: Timestamp) -> TabId {
        let space = self.space_mut();
        let id = space.tabs.open_tab(tab, position);
        space.tabs.select(id, now);
        space.selected_essential = None;
        id
    }

    /// Selects a tab. Essentials are shown in the current space; a tab of
    /// another space switches to that space first.
    pub fn select(&mut self, id: TabId, now: Timestamp) -> bool {
        match self.home_of(id) {
            Some(TabHome::Essentials) => {
                if let Some(tab) = self.essentials.iter_mut().find(|tab| tab.id == id) {
                    tab.last_active = now;
                }
                self.space_mut().selected_essential = Some(id);
                true
            }
            Some(TabHome::Private) => {
                self.private_shown = true;
                let space = self.space_mut();
                space.selected_essential = None;
                space.tabs.select(id, now)
            }
            Some(TabHome::Space(index)) => {
                self.active_space = index;
                self.private_shown = false;
                let space = self.space_mut();
                space.selected_essential = None;
                space.tabs.select(id, now)
            }
            None => false,
        }
    }

    /// Closes a tab. Essentials and pinned tabs reset to their pinned page
    /// and report [`CloseOutcome::Unloaded`]; unpinned tabs are removed.
    pub fn close(&mut self, id: TabId, now: Timestamp) -> Option<CloseOutcome> {
        match self.home_of(id)? {
            TabHome::Essentials => {
                let tab = self.essentials.iter_mut().find(|tab| tab.id == id)?;
                tab.reset_to_pinned();
                for space in &mut self.spaces {
                    if space.selected_essential == Some(id) {
                        space.selected_essential = None;
                    }
                }
                Some(CloseOutcome::Unloaded)
            }
            TabHome::Space(index) => self.spaces[index].tabs.close(id, now),
            TabHome::Private => self.private.as_mut()?.tabs.close(id, now),
        }
    }

    /// Turns a tab of the current space into an essential at the end of the grid.
    /// Returns false when the grid is full or the tab is not in the current space.
    pub fn make_essential(&mut self, id: TabId, now: Timestamp) -> bool {
        if self.essentials.len() >= MAX_ESSENTIALS || self.private_shown() {
            return false;
        }
        let was_active = self.space().tabs.active_id() == Some(id);
        let Some(mut tab) = self.space_mut().tabs.remove(id, now) else {
            return false;
        };
        tab.pin();
        self.essentials.push(tab);
        if was_active {
            self.space_mut().selected_essential = Some(id);
        }
        true
    }

    /// Moves an essential into the current space's pinned section at `index`.
    pub fn remove_essential(&mut self, id: TabId, index: usize, now: Timestamp) -> bool {
        if self.private_shown() {
            return false;
        }
        let Some(position) = self.essentials.iter().position(|tab| tab.id == id) else {
            return false;
        };
        let tab = self.essentials.remove(position);
        let was_shown = self.space().selected_essential == Some(id);
        for space in &mut self.spaces {
            if space.selected_essential == Some(id) {
                space.selected_essential = None;
            }
        }
        let space = self.space_mut();
        space.tabs.insert(tab, Section::Pinned, index);
        if was_shown {
            space.tabs.select(id, now);
        }
        true
    }

    /// Moves an essential to just before another in the grid.
    pub fn move_essential_before(&mut self, id: TabId, target: TabId) -> bool {
        if id == target {
            return false;
        }
        let Some(from) = self.essentials.iter().position(|tab| tab.id == id) else {
            return false;
        };
        if !self.is_essential(target) {
            return false;
        }
        let tab = self.essentials.remove(from);
        let to = self
            .essentials
            .iter()
            .position(|tab| tab.id == target)
            .unwrap_or(self.essentials.len());
        self.essentials.insert(to, tab);
        true
    }

    /// Archives unpinned tabs idle since before `now - archive_after_ms` in
    /// every space, except each space's selected tab. Returns their ids.
    pub fn archive_idle(&mut self, now: Timestamp) -> Vec<TabId> {
        let cutoff = now.saturating_sub(self.archive_after_ms);
        let mut archived = Vec::new();
        for space in &mut self.spaces {
            for tab in space.tabs.take_idle_unpinned(cutoff) {
                archived.push(tab.id);
                self.archive.insert(
                    0,
                    ArchivedTab {
                        url: tab.url,
                        title: tab.title,
                        archived_at: now,
                        space: space.id,
                    },
                );
            }
        }
        self.archive.truncate(ARCHIVE_LIMIT);
        archived
    }

    /// Reopens an archived tab in the current space, selects it and removes it
    /// from the archive.
    pub fn restore_archived(&mut self, index: usize, now: Timestamp) -> Option<TabId> {
        if index >= self.archive.len() {
            return None;
        }
        let entry = self.archive.remove(index);
        let id = self.open(entry.url, OpenPosition::End, now);
        if let Some(tab) = self.tab_mut(id) {
            tab.title = entry.title;
        }
        Some(id)
    }
}

impl Browser {
    /// A snapshot of every space, essential and archived tab, for saving.
    pub fn to_session(&self, layout: SavedLayout) -> Session {
        let essential_index = |id: Option<TabId>| {
            id.and_then(|id| self.essentials.iter().position(|tab| tab.id == id))
        };
        let spaces = self
            .spaces
            .iter()
            .map(|space| {
                let tabs = space.tabs.tabs();
                let index_of = |id: TabId| tabs.iter().position(|tab| tab.id == id);
                let (accent, gradient_end, gradient_third) = match space.theme {
                    SpaceTheme::Solid(accent) => (accent, None, None),
                    SpaceTheme::Gradient(accent, end) => (accent, Some(end), None),
                    SpaceTheme::Triple(accent, end, third) => (accent, Some(end), Some(third)),
                };
                SavedSpace {
                    name: space.name.clone(),
                    icon: space.icon.clone(),
                    icon_color: space.icon_color,
                    accent,
                    gradient_end,
                    gradient_third,
                    intensity: space.intensity,
                    texture: space.texture,
                    texture_kind: space.texture_kind,
                    theme_state: space.theme_state.clone(),
                    tabs: tabs.iter().map(save_tab).collect(),
                    active_tab: space.tabs.active_id().and_then(index_of),
                    selected_essential: essential_index(space.selected_essential),
                    splits: space
                        .tabs
                        .splits()
                        .iter()
                        .filter_map(|split| {
                            Some(SavedSplit {
                                first: index_of(split.first)?,
                                second: index_of(split.second)?,
                                vertical: split.orientation == SplitOrientation::Vertical,
                                ratio: split.ratio,
                            })
                        })
                        .collect(),
                    folders: space.tabs.folders(),
                }
            })
            .collect();
        let space_index = |id: SpaceId| {
            self.spaces
                .iter()
                .position(|space| space.id == id)
                .unwrap_or(0)
        };
        Session {
            version: SESSION_VERSION,
            spaces,
            active_space: self.active_space,
            essentials: self.essentials.iter().map(save_tab).collect(),
            archive: self
                .archive
                .iter()
                .map(|entry| SavedArchivedTab {
                    url: entry.url.clone(),
                    title: entry.title.clone(),
                    archived_at: entry.archived_at,
                    space: space_index(entry.space),
                })
                .collect(),
            layout,
        }
    }

    /// Rebuilds a browser from a saved session with fresh ids. Indexes that
    /// point nowhere are ignored; an empty session gives one default space.
    pub fn from_session(session: &Session) -> Self {
        let mut browser = Self {
            spaces: Vec::new(),
            active_space: 0,
            essentials: session
                .essentials
                .iter()
                .map(|saved| {
                    let mut tab = restore_tab(saved);
                    tab.pin();
                    tab
                })
                .collect(),
            archive: Vec::new(),
            next_space_id: 0,
            private: None,
            private_shown: false,
            archive_after_ms: DEFAULT_ARCHIVE_AFTER_MS,
        };
        for saved in &session.spaces {
            let tabs = restore_strip(saved);
            browser.next_space_id += 1;
            browser.spaces.push(Space {
                id: SpaceId(browser.next_space_id),
                name: saved.name.clone(),
                icon: saved.icon.clone(),
                icon_color: saved.icon_color,
                theme: match (saved.gradient_end, saved.gradient_third) {
                    (Some(end), Some(third)) => SpaceTheme::Triple(saved.accent, end, third),
                    (Some(end), None) => SpaceTheme::Gradient(saved.accent, end),
                    _ => SpaceTheme::Solid(saved.accent),
                },
                intensity: saved.intensity.clamp(0.0, 1.0),
                texture: saved.texture.clamp(0.0, 1.0),
                texture_kind: saved.texture_kind,
                theme_state: saved.theme_state.clone(),
                tabs,
                selected_essential: saved
                    .selected_essential
                    .and_then(|index| browser.essentials.get(index))
                    .map(|tab| tab.id),
            });
        }
        if browser.spaces.is_empty() {
            browser.add_space(DEFAULT_SPACE_NAME);
        }
        browser.active_space = session.active_space.min(browser.spaces.len() - 1);
        let first_space = browser.spaces[0].id;
        browser.archive = session
            .archive
            .iter()
            .take(ARCHIVE_LIMIT)
            .map(|entry| ArchivedTab {
                url: entry.url.clone(),
                title: entry.title.clone(),
                archived_at: entry.archived_at,
                space: browser
                    .spaces
                    .get(entry.space)
                    .map_or(first_space, |space| space.id),
            })
            .collect();
        browser
    }
}

fn restore_strip(saved: &SavedSpace) -> TabStrip {
    let mut tabs = TabStrip::new();
    for folder in &saved.folders {
        tabs.add_folder(folder.clone());
    }
    let ids: Vec<TabId> = saved
        .tabs
        .iter()
        .map(|saved_tab| {
            let tab = restore_tab(saved_tab);
            let id = tab.id;
            let section = if tab.is_pinned() {
                Section::Pinned
            } else {
                Section::Unpinned
            };
            tabs.insert(tab, section, usize::MAX);
            id
        })
        .collect();
    for split in &saved.splits {
        if let (Some(&first), Some(&second)) = (ids.get(split.first), ids.get(split.second))
            && tabs.split(first, second)
        {
            tabs.set_split_ratio(first, split.ratio);
            if split.vertical {
                tabs.toggle_split_orientation(first);
            }
        }
    }
    if let Some(&id) = saved.active_tab.and_then(|index| ids.get(index)) {
        let last_active = tabs.get(id).map_or(0, |tab| tab.last_active);
        tabs.select(id, last_active);
    }
    tabs
}

fn save_tab(tab: &Tab) -> SavedTab {
    SavedTab {
        url: tab.url.clone(),
        title: tab.title.clone(),
        pinned: tab.pinned.as_ref().map(|page| SavedPinnedPage {
            url: page.url.clone(),
            title: page.title.clone(),
            folder: page.folder.clone(),
        }),
        last_active: tab.last_active,
    }
}

fn restore_tab(saved: &SavedTab) -> Tab {
    let mut tab = Tab::new(saved.url.clone(), saved.last_active);
    tab.title = saved.title.clone();
    tab.pinned = saved.pinned.as_ref().map(|page| PinnedPage {
        url: page.url.clone(),
        title: page.title.clone(),
        folder: page.folder.clone(),
    });
    tab
}

#[cfg(test)]
mod tests {
    use super::*;

    fn browser_with(urls: &[&str]) -> (Browser, Vec<TabId>) {
        let mut browser = Browser::new();
        let ids = urls
            .iter()
            .map(|url| browser.open(*url, OpenPosition::End, 0))
            .collect();
        (browser, ids)
    }

    fn sample_browser() -> Browser {
        let (mut browser, ids) = browser_with(&["https://a", "https://b", "https://c"]);
        browser.space_mut().tabs.pin(ids[0]);
        browser.space_mut().tabs.split(ids[1], ids[2]);
        browser.space_mut().tabs.set_split_ratio(ids[1], 0.3);
        browser.space_mut().tabs.toggle_split_orientation(ids[1]);
        let essential = browser.open("https://mail", OpenPosition::End, 4);
        browser.make_essential(essential, 5);
        let work = browser.add_space("Work");
        browser.switch_space(work);
        browser.open("https://w", OpenPosition::End, 7);
        browser.archive_after_ms = 1;
        browser.open("https://old", OpenPosition::End, 0);
        browser.open("https://new", OpenPosition::End, 100);
        browser.archive_idle(100);
        browser
    }

    #[test]
    fn moving_a_tab_to_another_space_keeps_it_and_its_pin() {
        let (mut browser, ids) = browser_with(&["https://a", "https://b"]);
        browser.space_mut().tabs.pin(ids[0]);
        let work = browser.add_space("Work");
        assert!(browser.move_to_space(ids[0], work, 3));
        assert!(!browser.move_to_space(ids[1], 0, 3));
        assert!(browser.space().tabs.get(ids[0]).is_none());
        let moved = browser.spaces()[work].tabs.get(ids[0]).unwrap();
        assert!(moved.is_pinned());
        assert_eq!(browser.active_tab_id(), Some(ids[1]));
    }

    #[test]
    fn removing_a_space_archives_its_tabs_and_keeps_selection_valid() {
        let (mut browser, ids) = browser_with(&["https://a", "https://b"]);
        browser.space_mut().tabs.pin(ids[0]);
        let work = browser.add_space("Work");
        let play = browser.add_space("Play");
        browser.switch_space(play);
        let removed = browser.remove_space(0, 9).unwrap();
        assert_eq!(removed, ids);
        assert_eq!(browser.spaces().len(), 2);
        assert_eq!(browser.space().name, "Play");
        let urls: Vec<&str> = browser
            .archive()
            .iter()
            .map(|entry| entry.url.as_str())
            .collect();
        assert_eq!(urls, ["https://a", "https://b"]);
        assert_eq!(browser.remove_space(work, 9).map(|ids| ids.len()), Some(0));
        assert_eq!(browser.active_space_index(), 0);
        assert_eq!(browser.remove_space(0, 9), None);
    }

    #[test]
    fn session_round_trips_through_json() {
        let browser = sample_browser();
        let layout = SavedLayout {
            compact: true,
            sidebar_width: Some(300.0),
            ..SavedLayout::default()
        };
        let saved = browser.to_session(layout.clone());
        let json = serde_json::to_string(&saved).unwrap();
        let parsed: Session = serde_json::from_str(&json).unwrap();
        let restored = Browser::from_session(&parsed);
        assert_eq!(restored.to_session(layout), saved);
        assert_eq!(restored.active_space_index(), 1);
        assert_eq!(restored.archive().len(), 2);
        assert_eq!(restored.archive()[0].space, restored.spaces()[1].id);
        let personal = &restored.spaces()[0].tabs;
        assert!(personal.tabs()[0].is_pinned());
        let split = personal.splits()[0];
        assert_eq!(split.orientation, SplitOrientation::Vertical);
        assert!((split.ratio - 0.3).abs() < f32::EPSILON);
    }

    #[test]
    fn restore_ignores_indexes_that_point_nowhere() {
        let mut saved = sample_browser().to_session(SavedLayout::default());
        saved.active_space = 99;
        saved.spaces[0].active_tab = Some(99);
        saved.spaces[0].selected_essential = Some(99);
        saved.spaces[0].splits[0].second = 99;
        saved.archive[0].space = 99;
        let restored = Browser::from_session(&saved);
        assert_eq!(restored.active_space_index(), restored.spaces().len() - 1);
        assert!(restored.spaces()[0].tabs.splits().is_empty());
        assert_eq!(restored.archive()[0].space, restored.spaces()[0].id);
    }

    #[test]
    fn empty_session_gives_one_default_space() {
        let empty = Session {
            version: SESSION_VERSION,
            spaces: Vec::new(),
            active_space: 3,
            essentials: Vec::new(),
            archive: Vec::new(),
            layout: SavedLayout::default(),
        };
        let restored = Browser::from_session(&empty);
        assert_eq!(restored.spaces().len(), 1);
        assert_eq!(restored.active_space_index(), 0);
    }

    #[test]
    fn adopted_tab_keeps_its_id_and_becomes_active() {
        let (mut browser, ids) = browser_with(&["a", "b"]);
        let peek = Tab::new("https://peek.example", 5);
        let id = peek.id;
        assert_eq!(browser.adopt(peek, OpenPosition::After(ids[0]), 6), id);
        assert_eq!(browser.active_tab_id(), Some(id));
        assert_eq!(browser.space().tabs.index_of(id), Some(1));
    }

    #[test]
    fn opening_after_a_split_pane_keeps_the_pair_adjacent() {
        let (mut browser, ids) = browser_with(&["a", "b"]);
        browser.space_mut().tabs.split(ids[0], ids[1]);
        let c = browser.open("c", OpenPosition::After(ids[0]), 1);
        let tabs = &browser.space().tabs;
        assert_eq!(
            tabs.index_of(ids[1]),
            Some(tabs.index_of(ids[0]).unwrap() + 1)
        );
        assert!(tabs.index_of(c).is_some());
    }

    #[test]
    fn starts_with_one_space() {
        let browser = Browser::new();
        assert_eq!(browser.spaces().len(), 1);
        assert_eq!(browser.active_tab_id(), None);
    }

    #[test]
    fn each_space_keeps_its_own_tabs_and_selection() {
        let (mut browser, ids) = browser_with(&["a"]);
        let work = browser.add_space("Work");
        browser.switch_space(work);
        assert_eq!(browser.active_tab_id(), None);
        let b = browser.open("b", OpenPosition::End, 0);
        assert_eq!(browser.space().tabs.len(), 1);

        browser.switch_space(0);
        assert_eq!(browser.active_tab_id(), Some(ids[0]));
        browser.switch_space(work);
        assert_eq!(browser.active_tab_id(), Some(b));
        assert!(!browser.switch_space(9));
    }

    #[test]
    fn spaces_get_distinct_presets() {
        let mut browser = Browser::new();
        let second = browser.add_space("Work");
        let spaces = browser.spaces();
        assert_ne!(spaces[0].id, spaces[second].id);
        assert_ne!(spaces[0].icon, spaces[second].icon);
        assert_ne!(spaces[0].theme, spaces[second].theme);
    }

    #[test]
    fn cycling_icon_and_theme_changes_them() {
        let mut browser = Browser::new();
        let (icon, theme) = (browser.space().icon.clone(), browser.space().theme);
        browser.cycle_space_icon();
        browser.cycle_space_theme();
        assert_ne!(browser.space().icon, icon);
        assert_ne!(browser.space().theme, theme);
    }

    #[test]
    fn essentials_are_shared_and_shown_in_the_current_space() {
        let (mut browser, ids) = browser_with(&["mail", "b"]);
        assert!(browser.make_essential(ids[0], 0));
        assert!(browser.is_essential(ids[0]));
        assert_eq!(browser.space().tabs.len(), 1);

        let work = browser.add_space("Work");
        browser.switch_space(work);
        assert!(browser.select(ids[0], 5));
        assert_eq!(browser.active_tab_id(), Some(ids[0]));
        assert_eq!(browser.active_space_index(), work);
    }

    #[test]
    fn closing_an_essential_resets_it_and_falls_back_to_the_space_tab() {
        let (mut browser, ids) = browser_with(&["mail", "b"]);
        browser.make_essential(ids[0], 0);
        browser.select(ids[1], 0);
        browser.select(ids[0], 0);
        browser.tab_mut(ids[0]).unwrap().url = "mail/inbox/42".into();

        assert_eq!(browser.close(ids[0], 0), Some(CloseOutcome::Unloaded));
        assert_eq!(browser.tab(ids[0]).unwrap().url, "mail");
        assert_eq!(browser.active_tab_id(), Some(ids[1]));
    }

    #[test]
    fn essentials_are_capped() {
        let mut browser = Browser::new();
        for index in 0..MAX_ESSENTIALS {
            let id = browser.open(format!("site{index}"), OpenPosition::End, 0);
            assert!(browser.make_essential(id, 0));
        }
        let extra = browser.open("extra", OpenPosition::End, 0);
        assert!(!browser.make_essential(extra, 0));
        assert!(browser.space().tabs.get(extra).is_some());
    }

    #[test]
    fn removing_an_essential_pins_it_in_the_current_space() {
        let (mut browser, ids) = browser_with(&["mail"]);
        browser.make_essential(ids[0], 0);
        assert!(browser.remove_essential(ids[0], 0, 0));
        assert!(browser.essentials().is_empty());
        assert!(browser.space().tabs.get(ids[0]).unwrap().is_pinned());
        assert_eq!(browser.active_tab_id(), Some(ids[0]));
    }

    #[test]
    fn essentials_reorder() {
        let (mut browser, ids) = browser_with(&["a", "b", "c"]);
        for id in &ids {
            browser.make_essential(*id, 0);
        }
        assert!(browser.move_essential_before(ids[2], ids[0]));
        let order: Vec<&str> = browser
            .essentials()
            .iter()
            .map(|tab| tab.url.as_str())
            .collect();
        assert_eq!(order, ["c", "a", "b"]);
    }

    #[test]
    fn selecting_a_tab_of_another_space_switches_space() {
        let (mut browser, ids) = browser_with(&["a"]);
        let work = browser.add_space("Work");
        browser.switch_space(work);
        assert!(browser.select(ids[0], 0));
        assert_eq!(browser.active_space_index(), 0);
    }

    #[test]
    fn idle_unpinned_tabs_are_archived_across_spaces_and_restorable() {
        let (mut browser, ids) = browser_with(&["old", "current"]);
        browser.archive_after_ms = 100;
        browser.tab_mut(ids[0]).unwrap().last_active = 0;
        browser.select(ids[1], 0);
        let work = browser.add_space("Work");
        browser.switch_space(work);
        let stale = browser.open("work-old", OpenPosition::End, 0);
        browser.open("work-current", OpenPosition::End, 0);

        let archived = browser.archive_idle(1_000);
        assert_eq!(archived.len(), 2);
        assert!(archived.contains(&ids[0]) && archived.contains(&stale));
        assert!(browser.tab(ids[1]).is_some());
        assert_eq!(browser.archive()[0].url, "work-old");

        let restored = browser.restore_archived(1, 2_000).unwrap();
        assert_eq!(browser.tab(restored).unwrap().url, "old");
        assert_eq!(browser.active_tab_id(), Some(restored));
        assert_eq!(browser.archive().len(), 1);
    }

    #[test]
    fn recent_tabs_are_not_archived() {
        let (mut browser, _) = browser_with(&["a", "b"]);
        assert!(
            browser
                .archive_idle(DEFAULT_ARCHIVE_AFTER_MS - 1)
                .is_empty()
        );
    }

    #[test]
    fn texture_kinds_cycle_both_ways() {
        assert_eq!(TextureKind::None.cycled(1), TextureKind::Grain);
        assert_eq!(TextureKind::None.cycled(-1), TextureKind::Tweed);
        assert_eq!(TextureKind::Tweed.cycled(1), TextureKind::None);
    }

    #[test]
    fn the_private_window_is_never_saved_and_closes_without_a_trace() {
        let mut browser = Browser::new();
        let regular = browser.open("https://example.com", OpenPosition::End, 1);
        assert!(browser.show_private());
        let private = browser.open("https://secret.example", OpenPosition::End, 2);
        assert!(browser.is_private(private));
        assert!(!browser.is_private(regular));
        assert_eq!(browser.active_tab_id(), Some(private));
        assert!(!browser.move_to_space(private, 0, 3));
        let saved = browser.to_session(SavedLayout::default());
        let urls: Vec<&str> = saved.spaces[0]
            .tabs
            .iter()
            .map(|tab| tab.url.as_str())
            .collect();
        assert_eq!(urls, ["https://example.com"]);
        browser.show_regular();
        assert_eq!(browser.active_tab_id(), Some(regular));
        assert!(browser.select(private, 4));
        assert!(browser.private_shown());
        assert_eq!(browser.close_private(), vec![private]);
        assert!(browser.tab(private).is_none());
        assert_eq!(browser.active_tab_id(), Some(regular));
        assert!(browser.archive().is_empty());
    }

    #[test]
    fn imported_bookmarks_are_pinned_once_in_their_folders() {
        use crate::import::Bookmark;
        let mut browser = Browser::new();
        let mark = |url: &str, folder: Option<&str>| Bookmark {
            title: url.to_string(),
            url: url.to_string(),
            folder: folder.map(str::to_string),
        };
        let marks = [
            mark("https://a.example/", None),
            mark("https://b.example/", Some("Work")),
            mark("https://a.example/", Some("Again")),
            mark("https://c.example/", None),
        ];
        assert_eq!(browser.import_bookmarks(&marks, 2, 1), 2);
        assert_eq!(browser.import_bookmarks(&marks, 10, 1), 1);
        let pinned: Vec<(&str, Option<&str>)> = browser
            .space()
            .tabs
            .section(Section::Pinned)
            .map(|tab| (tab.url.as_str(), tab.folder()))
            .collect();
        assert_eq!(
            pinned,
            [
                ("https://a.example/", None),
                ("https://b.example/", Some("Work")),
                ("https://c.example/", None)
            ]
        );
    }
}
