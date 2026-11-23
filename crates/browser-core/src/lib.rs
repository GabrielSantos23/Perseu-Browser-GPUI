//! Browser model for perseu: tabs, spaces, essentials, history and session state.
//!
//! This crate has no UI or webview dependency so it can be unit tested in isolation.

/// Name used for the app data folder and window titles.
pub const APP_NAME: &str = "perseu";

pub mod bangs;
pub mod browser;
pub mod command_bar;
pub mod downloads;
pub mod extensions;
pub mod fuzzy;
pub mod history;
pub mod import;
pub mod omnibox;
pub mod permissions;
pub mod security;
pub mod session;
pub mod settings;
pub mod store;
pub mod tabs;
pub mod webstore;
pub mod zoom;
