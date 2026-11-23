//! The saved form of a window's spaces, tabs, essentials and archive.
//!
//! Tab and space ids are not saved; positions are used instead and fresh ids
//! are given on restore. Favicons are not saved; pages fetch them again.

use serde::{Deserialize, Serialize};

use crate::tabs::Timestamp;

/// Format version written into every session file.
pub const SESSION_VERSION: u32 = 1;

/// Everything needed to rebuild a window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// Format version.
    pub version: u32,
    /// Spaces in switcher order.
    pub spaces: Vec<SavedSpace>,
    /// Index of the current space.
    pub active_space: usize,
    /// Essentials in grid order.
    pub essentials: Vec<SavedTab>,
    /// Archived tabs, newest first.
    #[serde(default)]
    pub archive: Vec<SavedArchivedTab>,
    /// Window layout chosen by the user.
    #[serde(default)]
    pub layout: SavedLayout,
}

/// Window layout chosen by the user.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedLayout {
    /// Whether the sidebar is collapsed.
    pub compact: bool,
    /// Sidebar width in logical pixels, if the user resized it.
    pub sidebar_width: Option<f32>,
    /// Where the main window was, to reopen it there.
    #[serde(default)]
    pub window: Option<SavedWindow>,
    /// Page zoom by site origin, for the sites not at 100%.
    #[serde(default)]
    pub site_zoom: std::collections::BTreeMap<String, f64>,
}

/// The main window's place on the screen, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SavedWindow {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// The bounds above are the ones it returns to when restored.
    pub maximized: bool,
}

/// One space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedSpace {
    /// Display name.
    pub name: String,
    /// Icon name (or an emoji, from older versions).
    pub icon: String,
    /// Icon color as 0xRRGGBB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_color: Option<u32>,
    /// Accent color as 0xRRGGBB.
    pub accent: u32,
    /// Gradient end color, for gradient themes.
    #[serde(default)]
    pub gradient_end: Option<u32>,
    /// How strongly the theme tints the sidebar, 0 to 1.
    #[serde(default = "default_intensity")]
    pub intensity: f32,
    /// Third gradient color, laid over the other two.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gradient_third: Option<u32>,
    /// Texture strength, 0 to 1.
    #[serde(default)]
    pub texture: f32,
    /// Texture style.
    #[serde(default)]
    pub texture_kind: crate::browser::TextureKind,
    /// The theme picker's saved state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme_state: Option<String>,
    /// Tabs in display order, pinned first.
    pub tabs: Vec<SavedTab>,
    /// Index into `tabs` of the selected tab.
    #[serde(default)]
    pub active_tab: Option<usize>,
    /// Index into the session's essentials of the essential shown in this space.
    #[serde(default)]
    pub selected_essential: Option<usize>,
    /// Splits, as indexes into `tabs`.
    #[serde(default)]
    pub splits: Vec<SavedSplit>,
    /// Pinned folders in display order, including empty ones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub folders: Vec<String>,
}

fn default_intensity() -> f32 {
    crate::browser::DEFAULT_THEME_INTENSITY
}

/// One tab or essential.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedTab {
    /// Last committed URL.
    pub url: String,
    /// Last title.
    #[serde(default)]
    pub title: String,
    /// The page a pinned tab returns to.
    #[serde(default)]
    pub pinned: Option<SavedPinnedPage>,
    /// When the tab was last selected.
    #[serde(default)]
    pub last_active: Timestamp,
}

/// The page a pinned tab returns to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedPinnedPage {
    /// Pinned URL.
    pub url: String,
    /// Title when pinned.
    #[serde(default)]
    pub title: String,
    /// Folder in the pinned section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

/// Two tabs shown together.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedSplit {
    /// Index of the first pane's tab.
    pub first: usize,
    /// Index of the second pane's tab.
    pub second: usize,
    /// Stacked instead of side by side.
    #[serde(default)]
    pub vertical: bool,
    /// Share of the space given to the first pane.
    pub ratio: f32,
}

/// One archived tab.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedArchivedTab {
    /// URL when archived.
    pub url: String,
    /// Title when archived.
    #[serde(default)]
    pub title: String,
    /// When it was archived.
    pub archived_at: Timestamp,
    /// Index of the space it came from.
    pub space: usize,
}
