//! Ordered tab list with pinned and unpinned sections, selection and a
//! reopen-closed stack.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const CLOSED_HISTORY_LIMIT: usize = 25;
const MIN_SPLIT_RATIO: f32 = 0.15;
const MAX_SPLIT_RATIO: f32 = 0.85;
const EVEN_SPLIT: f32 = 0.5;

static NEXT_TAB_ID: AtomicU64 = AtomicU64::new(1);

/// Stable identifier of a tab, unique across all spaces for the lifetime of the process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(u64);

impl TabId {
    /// Allocates a fresh id.
    pub fn next() -> Self {
        Self(NEXT_TAB_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// The raw number, for use as a UI element key.
    pub fn value(self) -> u64 {
        self.0
    }
}

/// Milliseconds since the Unix epoch.
pub type Timestamp = u64;

/// Which part of the tab list a tab lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// Kept tabs shown above the divider.
    Pinned,
    /// Everyday tabs shown below the divider.
    Unpinned,
}

/// Where a newly opened tab goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenPosition {
    /// At the end of the unpinned section.
    End,
    /// Directly after the given tab, or at the top of the unpinned section
    /// when that tab is pinned.
    After(TabId),
}

/// The page a pinned tab or essential returns to when it is closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedPage {
    /// URL the tab was pinned with.
    pub url: String,
    /// Title the tab had when it was pinned.
    pub title: String,
    /// Folder in the pinned section, by name.
    pub folder: Option<String>,
}

/// What closing a tab did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseOutcome {
    /// An unpinned tab was removed and can be reopened.
    Removed,
    /// A pinned tab was kept and reset to its pinned page; its page should be unloaded.
    Unloaded,
}

/// One browser tab.
#[derive(Debug, Clone, PartialEq)]
pub struct Tab {
    /// Identifier.
    pub id: TabId,
    /// Last committed URL.
    pub url: String,
    /// Last document title.
    pub title: String,
    /// Favicon as PNG bytes.
    pub favicon: Option<Arc<[u8]>>,
    /// Set when the tab is pinned.
    pub pinned: Option<PinnedPage>,
    /// When the tab was last selected.
    pub last_active: Timestamp,
}

impl Tab {
    /// Creates an unpinned tab with a fresh id.
    pub fn new(url: impl Into<String>, now: Timestamp) -> Self {
        Self {
            id: TabId::next(),
            url: url.into(),
            title: String::new(),
            favicon: None,
            pinned: None,
            last_active: now,
        }
    }

    /// Whether the tab is pinned.
    pub fn is_pinned(&self) -> bool {
        self.pinned.is_some()
    }

    /// The pinned folder the tab is in.
    pub fn folder(&self) -> Option<&str> {
        self.pinned.as_ref()?.folder.as_deref()
    }

    /// Whether a pinned tab has navigated away from its pinned URL.
    pub fn has_drifted(&self) -> bool {
        self.pinned
            .as_ref()
            .is_some_and(|page| page.url != self.url)
    }

    /// Pins the tab to its current page, or keeps the existing pinned page.
    pub fn pin(&mut self) {
        if self.pinned.is_none() {
            self.pinned = Some(PinnedPage {
                url: self.url.clone(),
                title: self.title.clone(),
                folder: None,
            });
        }
    }

    /// Makes the tab an ordinary tab again.
    pub fn unpin(&mut self) {
        self.pinned = None;
    }

    /// Returns a pinned tab to its pinned page. Does nothing for unpinned tabs.
    pub fn reset_to_pinned(&mut self) {
        if let Some(page) = &self.pinned {
            self.url = page.url.clone();
            self.title = page.title.clone();
        }
    }

    fn set_section(&mut self, section: Section) {
        match section {
            Section::Pinned => self.pin(),
            Section::Unpinned => self.unpin(),
        }
    }
}

/// How the two panes of a split are arranged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitOrientation {
    /// Side by side.
    Horizontal,
    /// One above the other.
    Vertical,
}

/// Two tabs shown together. They stay next to each other in the list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Split {
    /// Left or top pane; listed first.
    pub first: TabId,
    /// Right or bottom pane; listed right after `first`.
    pub second: TabId,
    /// Pane arrangement.
    pub orientation: SplitOrientation,
    /// Share of the space given to `first`, between 0.15 and 0.85.
    pub ratio: f32,
}

impl Split {
    /// Whether the split shows this tab.
    pub fn contains(&self, id: TabId) -> bool {
        self.first == id || self.second == id
    }

    /// The other pane's tab.
    pub fn partner_of(&self, id: TabId) -> Option<TabId> {
        match id {
            id if id == self.first => Some(self.second),
            id if id == self.second => Some(self.first),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
struct ClosedTab {
    url: String,
    title: String,
    index: usize,
}

/// The tabs of one space, pinned tabs first.
#[derive(Debug, Default)]
pub struct TabStrip {
    tabs: Vec<Tab>,
    active: Option<TabId>,
    closed: Vec<ClosedTab>,
    splits: Vec<Split>,
    folders: Vec<String>,
}

impl TabStrip {
    /// Creates an empty strip.
    pub fn new() -> Self {
        Self::default()
    }

    /// All tabs in display order: pinned, then unpinned.
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// Tabs of one section in display order.
    pub fn section(&self, section: Section) -> impl Iterator<Item = &Tab> {
        let pinned = section == Section::Pinned;
        self.tabs
            .iter()
            .filter(move |tab| tab.is_pinned() == pinned)
    }

    /// The selected tab, if any.
    pub fn active(&self) -> Option<&Tab> {
        self.active.and_then(|id| self.get(id))
    }

    /// The selected tab's id, if any.
    pub fn active_id(&self) -> Option<TabId> {
        self.active
    }

    /// Looks up a tab.
    pub fn get(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|tab| tab.id == id)
    }

    /// Looks up a tab for editing.
    pub fn get_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|tab| tab.id == id)
    }

    /// Display index of a tab.
    pub fn index_of(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == id)
    }

    /// Number of tabs.
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Whether there are no tabs.
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Opens an unpinned tab without selecting it.
    pub fn open(
        &mut self,
        url: impl Into<String>,
        position: OpenPosition,
        now: Timestamp,
    ) -> TabId {
        self.open_tab(Tab::new(url, now), position)
    }

    /// Adds an existing tab, such as a promoted preview, at `position`.
    pub fn open_tab(&mut self, mut tab: Tab, position: OpenPosition) -> TabId {
        let id = tab.id;
        tab.set_section(Section::Unpinned);
        let index = self.insertion_index(position);
        self.tabs.insert(index, tab);
        self.keep_splits_together();
        id
    }

    /// Inserts an existing tab into `section` at `index` within that section (clamped).
    pub fn insert(&mut self, mut tab: Tab, section: Section, index: usize) {
        tab.set_section(section);
        let start = self.section_start(section);
        let len = self.section(section).count();
        self.tabs.insert(start + index.min(len), tab);
        self.keep_splits_together();
    }

    /// Current splits.
    pub fn splits(&self) -> &[Split] {
        &self.splits
    }

    /// The split showing this tab, if any.
    pub fn split_of(&self, id: TabId) -> Option<&Split> {
        self.splits.iter().find(|split| split.contains(id))
    }

    /// Shows two different, unsplit tabs side by side. `second` moves right
    /// after `first`, into its section. Returns false if not allowed.
    pub fn split(&mut self, first: TabId, second: TabId) -> bool {
        let allowed = first != second
            && self.get(first).is_some()
            && self.get(second).is_some()
            && self.split_of(first).is_none()
            && self.split_of(second).is_none();
        if !allowed {
            return false;
        }
        self.splits.push(Split {
            first,
            second,
            orientation: SplitOrientation::Horizontal,
            ratio: EVEN_SPLIT,
        });
        self.keep_splits_together();
        true
    }

    /// Ends the split showing this tab. Both tabs stay open.
    pub fn unsplit(&mut self, id: TabId) -> Option<Split> {
        let index = self.splits.iter().position(|split| split.contains(id))?;
        Some(self.splits.remove(index))
    }

    /// Sets how much space the first pane gets (clamped to 0.15–0.85).
    pub fn set_split_ratio(&mut self, id: TabId, ratio: f32) -> bool {
        let Some(split) = self.splits.iter_mut().find(|split| split.contains(id)) else {
            return false;
        };
        split.ratio = ratio.clamp(MIN_SPLIT_RATIO, MAX_SPLIT_RATIO);
        true
    }

    /// Switches a split between side by side and stacked.
    pub fn toggle_split_orientation(&mut self, id: TabId) -> bool {
        let Some(split) = self.splits.iter_mut().find(|split| split.contains(id)) else {
            return false;
        };
        split.orientation = match split.orientation {
            SplitOrientation::Horizontal => SplitOrientation::Vertical,
            SplitOrientation::Vertical => SplitOrientation::Horizontal,
        };
        true
    }

    fn keep_splits_together(&mut self) {
        for split in self.splits.clone() {
            let (Some(_), Some(from)) = (self.index_of(split.first), self.index_of(split.second))
            else {
                continue;
            };
            let mut second = self.tabs.remove(from);
            let first = self.index_of(split.first).unwrap_or(0);
            let section = if self.tabs[first].is_pinned() {
                Section::Pinned
            } else {
                Section::Unpinned
            };
            second.set_section(section);
            self.tabs.insert(first + 1, second);
        }
    }

    /// Takes a tab out of the strip without remembering it as closed.
    /// If it was selected, the tab used before it becomes selected.
    pub fn remove(&mut self, id: TabId, now: Timestamp) -> Option<Tab> {
        let index = self.index_of(id)?;
        self.unsplit(id);
        let tab = self.tabs.remove(index);
        if self.active == Some(id) {
            self.active = None;
            self.select_last_used(None, now);
        }
        Some(tab)
    }

    /// Selects a tab and stamps its activity time. Returns false for unknown ids.
    pub fn select(&mut self, id: TabId, now: Timestamp) -> bool {
        let Some(tab) = self.get_mut(id) else {
            return false;
        };
        tab.last_active = now;
        self.active = Some(id);
        true
    }

    /// Closes a tab. Unpinned tabs are removed and remembered for
    /// [`TabStrip::reopen_closed`]; pinned tabs stay and reset to their pinned
    /// page. If the tab was selected, the tab used before it becomes selected.
    pub fn close(&mut self, id: TabId, now: Timestamp) -> Option<CloseOutcome> {
        let index = self.index_of(id)?;
        if self.tabs[index].is_pinned() {
            self.unsplit(id);
            self.tabs[index].reset_to_pinned();
            if self.active == Some(id) {
                self.select_last_used(Some(id), now);
            }
            return Some(CloseOutcome::Unloaded);
        }
        let tab = self.remove(id, now)?;
        self.remember_closed(tab, index);
        Some(CloseOutcome::Removed)
    }

    /// Reopens the most recently closed tab near its old position and selects it.
    pub fn reopen_closed(&mut self, now: Timestamp) -> Option<TabId> {
        let closed = self.closed.pop()?;
        let mut tab = Tab::new(closed.url, now);
        tab.title = closed.title;
        let id = tab.id;
        let index = closed
            .index
            .clamp(self.section_start(Section::Unpinned), self.tabs.len());
        self.tabs.insert(index, tab);
        self.select(id, now);
        Some(id)
    }

    /// Moves a tab into `section` at `index` within that section (clamped).
    /// This is how drag and drop pins and unpins tabs.
    pub fn move_tab(&mut self, id: TabId, section: Section, index: usize) -> bool {
        let Some(from) = self.index_of(id) else {
            return false;
        };
        let tab = self.tabs.remove(from);
        self.insert(tab, section, index);
        true
    }

    /// Moves a tab to just before `target`, taking on the target's section.
    /// This is what dropping a dragged tab onto another tab does.
    pub fn move_before(&mut self, id: TabId, target: TabId) -> bool {
        if id == target || self.get(target).is_none() {
            return false;
        }
        let Some(from) = self.index_of(id) else {
            return false;
        };
        let mut tab = self.tabs.remove(from);
        let target = match self.split_of(target) {
            Some(split) if split.second == target && !split.contains(id) => split.first,
            _ => target,
        };
        let to = self.index_of(target).unwrap_or(self.tabs.len());
        let section = if self.tabs[to].is_pinned() {
            Section::Pinned
        } else {
            Section::Unpinned
        };
        tab.set_section(section);
        self.tabs.insert(to, tab);
        self.keep_splits_together();
        true
    }

    /// Moves a tab to the end of the pinned section.
    pub fn pin(&mut self, id: TabId) -> bool {
        self.move_tab(id, Section::Pinned, usize::MAX)
    }

    /// Moves a tab to the top of the unpinned section.
    pub fn unpin(&mut self, id: TabId) -> bool {
        self.move_tab(id, Section::Unpinned, 0)
    }

    /// Pinned folders in display order, each named once, including empty ones.
    pub fn folders(&self) -> Vec<String> {
        let mut folders = self.folders.clone();
        for tab in self.section(Section::Pinned) {
            if let Some(folder) = tab.folder()
                && !folders.iter().any(|known| known == folder)
            {
                folders.push(folder.to_string());
            }
        }
        folders
    }

    /// Puts a pinned tab in `folder`, or takes it out with `None`.
    pub fn set_folder(&mut self, id: TabId, folder: Option<String>) -> bool {
        let Some(page) = self.get_mut(id).and_then(|tab| tab.pinned.as_mut()) else {
            return false;
        };
        page.folder = folder.clone();
        if let Some(folder) = folder {
            self.add_folder(folder);
        }
        true
    }

    /// Adds an empty folder at the end, if no folder has that name.
    pub fn add_folder(&mut self, name: String) {
        if !self.folders().contains(&name) {
            self.folders.push(name);
        }
    }

    /// Renames a folder. Merges into `to` if that folder exists.
    pub fn rename_folder(&mut self, from: &str, to: &str) {
        self.each_in_folder(from, |page| page.folder = Some(to.to_string()));
        let merging = self.folders.iter().any(|name| name == to);
        if merging {
            self.folders.retain(|name| name != from);
        } else if let Some(name) = self.folders.iter_mut().find(|name| *name == from) {
            *name = to.to_string();
        }
    }

    /// Removes a folder; its tabs stay pinned.
    pub fn remove_folder(&mut self, name: &str) {
        self.each_in_folder(name, |page| page.folder = None);
        self.folders.retain(|known| known != name);
    }

    /// A folder name not used yet, like "Folder 2".
    pub fn new_folder_name(&self) -> String {
        let folders = self.folders();
        (1..)
            .map(|n| format!("Folder {n}"))
            .find(|name| !folders.contains(name))
            .unwrap_or_default()
    }

    fn each_in_folder(&mut self, name: &str, mut edit: impl FnMut(&mut PinnedPage)) {
        for tab in &mut self.tabs {
            if let Some(page) = tab.pinned.as_mut()
                && page.folder.as_deref() == Some(name)
            {
                edit(page);
            }
        }
    }

    /// Selects the tab `offset` steps away in display order, wrapping around.
    pub fn select_relative(&mut self, offset: isize, now: Timestamp) -> Option<TabId> {
        let len = self.tabs.len() as isize;
        if len == 0 {
            return None;
        }
        let current = self.active.and_then(|id| self.index_of(id)).unwrap_or(0) as isize;
        let target = (current + offset).rem_euclid(len) as usize;
        let id = self.tabs[target].id;
        self.select(id, now);
        Some(id)
    }

    /// Selects the tab at a display index.
    pub fn select_index(&mut self, index: usize, now: Timestamp) -> Option<TabId> {
        let id = self.tabs.get(index)?.id;
        self.select(id, now);
        Some(id)
    }

    /// Selects the last tab in display order.
    pub fn select_last(&mut self, now: Timestamp) -> Option<TabId> {
        let last = self.tabs.len().checked_sub(1)?;
        self.select_index(last, now)
    }

    /// Removes unpinned tabs last used before `cutoff`, except the selected
    /// tab and tabs in a split.
    pub fn take_idle_unpinned(&mut self, cutoff: Timestamp) -> Vec<Tab> {
        let active = self.active;
        let splits = self.splits.clone();
        let (idle, kept): (Vec<Tab>, Vec<Tab>) =
            std::mem::take(&mut self.tabs).into_iter().partition(|tab| {
                !tab.is_pinned()
                    && tab.last_active < cutoff
                    && Some(tab.id) != active
                    && !splits.iter().any(|split| split.contains(tab.id))
            });
        self.tabs = kept;
        idle
    }

    fn section_start(&self, section: Section) -> usize {
        match section {
            Section::Pinned => 0,
            Section::Unpinned => self.tabs.iter().take_while(|tab| tab.is_pinned()).count(),
        }
    }

    fn insertion_index(&self, position: OpenPosition) -> usize {
        let unpinned_start = self.section_start(Section::Unpinned);
        match position {
            OpenPosition::End => self.tabs.len(),
            OpenPosition::After(opener) => self
                .index_of(opener)
                .map_or(self.tabs.len(), |index| (index + 1).max(unpinned_start)),
        }
    }

    /// Selects the tab used most recently, other than `except`.
    fn select_last_used(&mut self, except: Option<TabId>, now: Timestamp) {
        let last = self
            .tabs
            .iter()
            .filter(|tab| Some(tab.id) != except)
            .max_by_key(|tab| tab.last_active)
            .map(|tab| tab.id);
        if let Some(next) = last {
            self.select(next, now);
        }
    }

    fn remember_closed(&mut self, tab: Tab, index: usize) {
        if self.closed.len() == CLOSED_HISTORY_LIMIT {
            self.closed.remove(0);
        }
        self.closed.push(ClosedTab {
            url: tab.url,
            title: tab.title,
            index,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(strip: &TabStrip) -> Vec<&str> {
        strip.tabs().iter().map(|tab| tab.url.as_str()).collect()
    }

    fn strip_with(urls: &[&str]) -> (TabStrip, Vec<TabId>) {
        let mut strip = TabStrip::new();
        let ids = urls
            .iter()
            .map(|url| strip.open(*url, OpenPosition::End, 0))
            .collect();
        (strip, ids)
    }

    #[test]
    fn open_appends_unpinned_tabs_without_selecting() {
        let (strip, ids) = strip_with(&["a", "b"]);
        assert_eq!(urls(&strip), ["a", "b"]);
        assert_ne!(ids[0], ids[1]);
        assert!(strip.tabs().iter().all(|tab| !tab.is_pinned()));
        assert_eq!(strip.active_id(), None);
    }

    #[test]
    fn open_after_inserts_next_to_opener() {
        let (mut strip, ids) = strip_with(&["a", "b", "c"]);
        strip.open("child", OpenPosition::After(ids[0]), 0);
        assert_eq!(urls(&strip), ["a", "child", "b", "c"]);
    }

    #[test]
    fn open_after_pinned_opener_goes_to_top_of_unpinned() {
        let (mut strip, ids) = strip_with(&["a", "b", "c"]);
        strip.pin(ids[1]);
        strip.pin(ids[2]);
        strip.open("child", OpenPosition::After(ids[1]), 0);
        assert_eq!(urls(&strip), ["b", "c", "child", "a"]);
    }

    #[test]
    fn select_sets_active_and_stamps_time() {
        let (mut strip, ids) = strip_with(&["a", "b"]);
        assert!(strip.select(ids[1], 42));
        assert_eq!(strip.active_id(), Some(ids[1]));
        assert_eq!(strip.get(ids[1]).unwrap().last_active, 42);
        assert!(!strip.select(TabId(999), 43));
    }

    #[test]
    fn closing_active_tab_selects_the_tab_used_before_it() {
        let (mut strip, ids) = strip_with(&["a", "b", "c"]);
        strip.select(ids[0], 1);
        strip.select(ids[2], 2);
        strip.select(ids[1], 3);
        strip.close(ids[1], 4);
        assert_eq!(strip.active_id(), Some(ids[2]));
        strip.close(ids[2], 5);
        assert_eq!(strip.active_id(), Some(ids[0]));
        strip.close(ids[0], 0);
        assert_eq!(strip.active_id(), None);
        assert!(strip.is_empty());
    }

    #[test]
    fn closing_inactive_tab_keeps_selection() {
        let (mut strip, ids) = strip_with(&["a", "b"]);
        strip.select(ids[0], 0);
        strip.close(ids[1], 0);
        assert_eq!(strip.active_id(), Some(ids[0]));
    }

    #[test]
    fn reopen_restores_position_and_selects() {
        let (mut strip, ids) = strip_with(&["a", "b", "c"]);
        strip.close(ids[0], 0);
        strip.close(ids[2], 0);

        let c = strip.reopen_closed(5).unwrap();
        assert_eq!(urls(&strip), ["b", "c"]);
        assert_eq!(strip.active_id(), Some(c));

        strip.reopen_closed(6).unwrap();
        assert_eq!(urls(&strip), ["a", "b", "c"]);
        assert_eq!(strip.reopen_closed(7), None);
    }

    #[test]
    fn closing_a_pinned_tab_resets_it_instead_of_removing() {
        let (mut strip, ids) = strip_with(&["home", "b"]);
        strip.get_mut(ids[0]).unwrap().title = "Home".into();
        strip.pin(ids[0]);
        strip.select(ids[0], 0);
        let tab = strip.get_mut(ids[0]).unwrap();
        tab.url = "home/elsewhere".into();
        tab.title = "Elsewhere".into();
        assert!(tab.has_drifted());

        assert_eq!(strip.close(ids[0], 1), Some(CloseOutcome::Unloaded));
        let tab = strip.get(ids[0]).unwrap();
        assert_eq!((tab.url.as_str(), tab.title.as_str()), ("home", "Home"));
        assert!(!tab.has_drifted());
        assert_eq!(strip.len(), 2);
        assert_eq!(strip.active_id(), Some(ids[1]));
        assert_eq!(strip.reopen_closed(2), None);
    }

    #[test]
    fn closing_the_only_pinned_tab_keeps_it_selected() {
        let (mut strip, ids) = strip_with(&["a"]);
        strip.pin(ids[0]);
        strip.select(ids[0], 0);
        assert_eq!(strip.close(ids[0], 0), Some(CloseOutcome::Unloaded));
        assert_eq!(strip.active_id(), Some(ids[0]));
    }

    #[test]
    fn unpinning_forgets_the_pinned_page() {
        let (mut strip, ids) = strip_with(&["a"]);
        strip.pin(ids[0]);
        strip.unpin(ids[0]);
        assert_eq!(strip.close(ids[0], 0), Some(CloseOutcome::Removed));
        assert!(strip.is_empty());
    }

    #[test]
    fn idle_unpinned_tabs_are_taken_except_pinned_and_selected() {
        let mut strip = TabStrip::new();
        let old = strip.open("old", OpenPosition::End, 10);
        let pinned = strip.open("pinned", OpenPosition::End, 10);
        let selected = strip.open("selected", OpenPosition::End, 10);
        let fresh = strip.open("fresh", OpenPosition::End, 500);
        strip.pin(pinned);
        strip.get_mut(pinned).unwrap().last_active = 10;
        strip.active = Some(selected);

        let idle = strip.take_idle_unpinned(100);
        assert_eq!(idle.iter().map(|tab| tab.id).collect::<Vec<_>>(), [old]);
        assert!(
            strip.get(pinned).is_some()
                && strip.get(selected).is_some()
                && strip.get(fresh).is_some()
        );
    }

    #[test]
    fn remove_and_insert_move_tabs_between_strips() {
        let (mut from, ids) = strip_with(&["a", "b"]);
        let (mut to, _) = strip_with(&["x"]);
        from.select(ids[0], 0);
        let tab = from.remove(ids[0], 0).unwrap();
        assert_eq!(from.active_id(), Some(ids[1]));
        to.insert(tab, Section::Pinned, 0);
        assert_eq!(urls(&to), ["a", "x"]);
        assert!(to.get(ids[0]).unwrap().is_pinned());
        assert_eq!(from.reopen_closed(0), None);
    }

    #[test]
    fn move_reorders_within_a_section() {
        let (mut strip, ids) = strip_with(&["a", "b", "c"]);
        strip.move_tab(ids[0], Section::Unpinned, 2);
        assert_eq!(urls(&strip), ["b", "c", "a"]);
        strip.move_tab(ids[0], Section::Unpinned, 0);
        assert_eq!(urls(&strip), ["a", "b", "c"]);
    }

    #[test]
    fn move_across_divider_pins_and_unpins() {
        let (mut strip, ids) = strip_with(&["a", "b", "c"]);
        strip.move_tab(ids[2], Section::Pinned, 0);
        assert_eq!(urls(&strip), ["c", "a", "b"]);
        assert!(strip.get(ids[2]).unwrap().is_pinned());

        strip.move_tab(ids[2], Section::Unpinned, 1);
        assert_eq!(urls(&strip), ["a", "c", "b"]);
        assert!(!strip.get(ids[2]).unwrap().is_pinned());
    }

    #[test]
    fn move_before_places_tab_ahead_of_target_in_either_direction() {
        let (mut strip, ids) = strip_with(&["a", "b", "c", "d"]);
        assert!(strip.move_before(ids[0], ids[2]));
        assert_eq!(urls(&strip), ["b", "a", "c", "d"]);
        assert!(strip.move_before(ids[3], ids[1]));
        assert_eq!(urls(&strip), ["d", "b", "a", "c"]);
        assert!(!strip.move_before(ids[0], ids[0]));
    }

    #[test]
    fn move_before_a_pinned_tab_pins_it() {
        let (mut strip, ids) = strip_with(&["a", "b", "c"]);
        strip.pin(ids[0]);
        assert!(strip.move_before(ids[2], ids[0]));
        assert_eq!(urls(&strip), ["c", "a", "b"]);
        assert!(strip.get(ids[2]).unwrap().is_pinned());
        assert!(strip.move_before(ids[0], ids[1]));
        assert!(!strip.get(ids[0]).unwrap().is_pinned());
        assert_eq!(urls(&strip), ["c", "a", "b"]);
    }

    #[test]
    fn pin_appends_to_pinned_and_unpin_goes_to_top_of_unpinned() {
        let (mut strip, ids) = strip_with(&["a", "b", "c"]);
        strip.pin(ids[1]);
        strip.pin(ids[2]);
        assert_eq!(urls(&strip), ["b", "c", "a"]);
        assert_eq!(strip.section(Section::Pinned).count(), 2);

        strip.unpin(ids[1]);
        assert_eq!(urls(&strip), ["c", "b", "a"]);
        assert_eq!(strip.section(Section::Pinned).count(), 1);
    }

    #[test]
    fn select_relative_wraps_in_display_order() {
        let (mut strip, ids) = strip_with(&["a", "b", "c"]);
        strip.select(ids[2], 0);
        assert_eq!(strip.select_relative(1, 0), Some(ids[0]));
        assert_eq!(strip.select_relative(-1, 0), Some(ids[2]));
    }

    #[test]
    fn select_index_and_last() {
        let (mut strip, ids) = strip_with(&["a", "b", "c"]);
        assert_eq!(strip.select_index(1, 0), Some(ids[1]));
        assert_eq!(strip.select_index(9, 0), None);
        assert_eq!(strip.select_last(0), Some(ids[2]));
    }

    #[test]
    fn split_pairs_tabs_next_to_each_other() {
        let (mut strip, ids) = strip_with(&["a", "b", "c"]);
        assert!(strip.split(ids[0], ids[2]));
        assert_eq!(urls(&strip), ["a", "c", "b"]);
        let split = *strip.split_of(ids[2]).unwrap();
        assert_eq!(
            (split.first, split.second, split.ratio),
            (ids[0], ids[2], 0.5)
        );
        assert_eq!(split.partner_of(ids[0]), Some(ids[2]));
    }

    #[test]
    fn split_rejects_same_or_already_split_tabs() {
        let (mut strip, ids) = strip_with(&["a", "b", "c"]);
        assert!(!strip.split(ids[0], ids[0]));
        assert!(strip.split(ids[0], ids[1]));
        assert!(!strip.split(ids[1], ids[2]));
    }

    #[test]
    fn second_pane_follows_the_first_into_its_section() {
        let (mut strip, ids) = strip_with(&["a", "b", "c"]);
        strip.pin(ids[0]);
        strip.split(ids[0], ids[2]);
        assert_eq!(urls(&strip), ["a", "c", "b"]);
        assert!(strip.get(ids[2]).unwrap().is_pinned());
    }

    #[test]
    fn moving_a_split_tab_moves_its_partner_and_nothing_lands_between() {
        let (mut strip, ids) = strip_with(&["a", "b", "c", "d"]);
        strip.split(ids[0], ids[1]);
        strip.move_tab(ids[0], Section::Unpinned, usize::MAX);
        assert_eq!(urls(&strip), ["c", "d", "a", "b"]);
        strip.move_before(ids[2], ids[1]);
        assert_eq!(urls(&strip), ["d", "c", "a", "b"]);
    }

    #[test]
    fn unsplit_and_close_dissolve_splits() {
        let (mut strip, ids) = strip_with(&["a", "b", "c"]);
        strip.split(ids[0], ids[1]);
        assert!(strip.unsplit(ids[1]).is_some());
        assert!(strip.split_of(ids[0]).is_none());
        strip.split(ids[0], ids[1]);
        strip.close(ids[1], 0);
        assert!(strip.splits().is_empty());
        assert_eq!(strip.len(), 2);
    }

    #[test]
    fn split_ratio_is_clamped_and_orientation_toggles() {
        let (mut strip, ids) = strip_with(&["a", "b"]);
        strip.split(ids[0], ids[1]);
        strip.set_split_ratio(ids[1], 0.99);
        strip.toggle_split_orientation(ids[0]);
        let split = strip.split_of(ids[0]).unwrap();
        assert_eq!(split.ratio, MAX_SPLIT_RATIO);
        assert_eq!(split.orientation, SplitOrientation::Vertical);
    }

    #[test]
    fn split_tabs_are_never_archived() {
        let mut strip = TabStrip::new();
        let a = strip.open("a", OpenPosition::End, 0);
        let b = strip.open("b", OpenPosition::End, 0);
        strip.split(a, b);
        assert!(strip.take_idle_unpinned(100).is_empty());
    }

    #[test]
    fn closed_history_is_bounded() {
        let mut strip = TabStrip::new();
        for _ in 0..CLOSED_HISTORY_LIMIT + 5 {
            let id = strip.open("x", OpenPosition::End, 0);
            strip.close(id, 0);
        }
        let mut reopened = 0;
        while strip.reopen_closed(0).is_some() {
            reopened += 1;
        }
        assert_eq!(reopened, CLOSED_HISTORY_LIMIT);
    }

    #[test]
    fn folders_group_pinned_tabs_by_name() {
        let mut strip = TabStrip::new();
        let a = strip.open("https://a.com", OpenPosition::End, 0);
        let b = strip.open("https://b.com", OpenPosition::End, 0);
        let loose = strip.open("https://c.com", OpenPosition::End, 0);
        for id in [a, b, loose] {
            strip.pin(id);
        }
        let name = strip.new_folder_name();
        assert_eq!(name, "Folder 1");
        assert!(strip.set_folder(b, Some(name.clone())));
        assert!(strip.set_folder(a, Some(name.clone())));
        assert_eq!(strip.folders(), vec![name.clone()]);
        assert_eq!(strip.new_folder_name(), "Folder 2");

        strip.rename_folder(&name, "Work");
        assert_eq!(strip.get(a).and_then(Tab::folder), Some("Work"));
        strip.unpin(b);
        assert_eq!(strip.get(b).and_then(Tab::folder), None);
        assert!(!strip.set_folder(b, Some("Work".into())));
        strip.remove_folder("Work");
        assert!(strip.folders().is_empty());
        assert!(strip.get(a).is_some_and(Tab::is_pinned));

        strip.add_folder("Empty".into());
        strip.add_folder("Empty".into());
        assert_eq!(strip.folders(), vec!["Empty".to_string()]);
        strip.rename_folder("Empty", "Later");
        assert_eq!(strip.folders(), vec!["Later".to_string()]);
    }
}
