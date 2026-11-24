//! User preferences, saved as JSON.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::browser::DEFAULT_ARCHIVE_AFTER_MS;
use crate::omnibox::SearchEngine;

/// Accent used when none is chosen, as 0xRRGGBB.
pub const DEFAULT_ACCENT: u32 = 0x7C8CFF;
/// Idle-time presets offered for auto-archiving, in seconds. 0 means never.
pub const ARCHIVE_PRESETS_SECS: [u64; 5] = [3_600, 43_200, 86_400, 604_800, 0];

const MS_PER_SEC: u64 = 1_000;

/// Modifier that turns a link click into a peek.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PeekModifier {
    /// Control.
    Ctrl,
    /// Shift. Also used for values this version does not know.
    #[default]
    #[serde(other)]
    Shift,
}

impl PeekModifier {
    /// Every choice, in display order.
    pub const ALL: [Self; 2] = [Self::Shift, Self::Ctrl];

    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Shift => "Shift",
            Self::Ctrl => "Ctrl",
        }
    }
}

/// Light or dark look, for the app and for pages that offer both.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorScheme {
    /// Always light.
    Light,
    /// Always dark.
    Dark,
    /// Follow Windows. Also used for values this version does not know.
    #[default]
    #[serde(other)]
    System,
}

/// User preferences. Missing fields take their defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Name of the search engine, one of [`SearchEngine::ALL`].
    pub search_engine: String,
    /// App accent as 0xRRGGBB.
    pub accent: u32,
    /// Idle time before unpinned tabs are archived, in seconds. 0 means never.
    pub archive_after_secs: u64,
    /// Modifier for peeking links.
    pub peek_modifier: PeekModifier,
    /// Where downloads go. `None` means the system Downloads folder.
    pub downloads_folder: Option<PathBuf>,
    /// Shortcut overrides, keyed by the default keys (`"ctrl-t"`) with the
    /// replacement keys as value.
    pub shortcuts: BTreeMap<String, String>,
    /// Hide the title bar until the pointer reaches the window's top edge.
    pub hide_title_bar: bool,
    /// Show pinned tabs in a bar under the title bar instead of the sidebar.
    pub favorites_bar: bool,
    /// Show the website name and address at the top of the sidebar instead
    /// of in the title bar.
    pub site_title_in_sidebar: bool,
    /// Colors added in the theme picker's "Custom colour" list, as 0xRRGGBB.
    pub custom_colors: Vec<u32>,
    /// Installed extensions keyed by their folder name, with whether they are
    /// enabled. Folders absent from the map are not installed or disabled.
    pub extensions: BTreeMap<String, bool>,
    /// Check downloads with SmartScreen. With it on, files the check flags
    /// are held and never saved; with it off, every download is saved.
    pub download_safety_checks: bool,
    /// Light, dark, or whatever Windows uses.
    pub color_scheme: ColorScheme,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            search_engine: SearchEngine::default().name.to_string(),
            accent: DEFAULT_ACCENT,
            archive_after_secs: DEFAULT_ARCHIVE_AFTER_MS / MS_PER_SEC,
            peek_modifier: PeekModifier::default(),
            downloads_folder: None,
            shortcuts: BTreeMap::new(),
            hide_title_bar: false,
            favorites_bar: false,
            site_title_in_sidebar: false,
            custom_colors: Vec::new(),
            extensions: BTreeMap::new(),
            download_safety_checks: true,
            color_scheme: ColorScheme::default(),
        }
    }
}

impl Settings {
    /// The chosen search engine, or the default when the name is unknown.
    pub fn engine(&self) -> SearchEngine {
        SearchEngine::ALL
            .into_iter()
            .find(|engine| engine.name == self.search_engine)
            .unwrap_or_default()
    }

    /// Idle time before archiving in milliseconds; `u64::MAX` when archiving is off.
    pub fn archive_after_ms(&self) -> u64 {
        match self.archive_after_secs {
            0 => u64::MAX,
            secs => secs.saturating_mul(MS_PER_SEC),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_peek_keys_fall_back_to_shift() {
        let settings: Settings = serde_json::from_str(r#"{ "peek_modifier": "alt" }"#).unwrap();
        assert_eq!(settings.peek_modifier, PeekModifier::Shift);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let settings: Settings = serde_json::from_str(r#"{ "accent": 255 }"#).unwrap();
        assert_eq!(settings.accent, 255);
        assert_eq!(settings.engine(), SearchEngine::GOOGLE);
        assert_eq!(settings.peek_modifier, PeekModifier::Shift);
        assert_eq!(settings.archive_after_ms(), DEFAULT_ARCHIVE_AFTER_MS);
    }

    #[test]
    fn unknown_engine_falls_back_and_zero_disables_archiving() {
        let settings = Settings {
            search_engine: "Nope".into(),
            archive_after_secs: 0,
            ..Settings::default()
        };
        assert_eq!(settings.engine(), SearchEngine::default());
        assert_eq!(settings.archive_after_ms(), u64::MAX);
    }

    #[test]
    fn round_trips_through_json() {
        let mut settings = Settings {
            search_engine: SearchEngine::BRAVE.name.into(),
            peek_modifier: PeekModifier::Ctrl,
            downloads_folder: Some(PathBuf::from(r"D:\Downloads")),
            ..Settings::default()
        };
        settings
            .shortcuts
            .insert("ctrl-s".into(), "ctrl-shift-s".into());
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains(r#""peek_modifier":"ctrl""#));
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), settings);
    }
}
