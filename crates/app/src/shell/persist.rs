use std::time::Duration;

use browser_core::history::History;
use browser_core::permissions::SitePermissions;
use browser_core::session::{SavedLayout, SavedWindow, Session};
use browser_core::settings::{PeekModifier, Settings};
use browser_core::store;
use gpui::{Context, Window, WindowBounds};
use webview_host::LinkModifier;

use super::Shell;
use crate::paths::Paths;

const SAVE_DELAY: Duration = Duration::from_secs(1);

pub(super) struct LoadedState {
    pub(super) paths: Paths,
    pub(super) settings: Settings,
    pub(super) session: Option<Session>,
    pub(super) history: History,
    pub(super) permissions: SitePermissions,
}

/// The main window's place, for the session.
pub(super) fn placement(window: &Window) -> SavedWindow {
    let (bounds, maximized) = match window.window_bounds() {
        WindowBounds::Maximized(bounds) => (bounds, true),
        WindowBounds::Windowed(bounds) | WindowBounds::Fullscreen(bounds) => (bounds, false),
    };
    SavedWindow {
        x: f32::from(bounds.origin.x),
        y: f32::from(bounds.origin.y),
        width: f32::from(bounds.size.width),
        height: f32::from(bounds.size.height),
        maximized,
    }
}

pub(super) fn load_state() -> LoadedState {
    let paths = Paths::detect();
    let settings = store::load_json(&paths.settings())
        .unwrap_or_else(|error| {
            tracing::error!("settings reset to defaults: {error:#}");
            None
        })
        .unwrap_or_default();
    let session = store::load_json(&paths.session()).unwrap_or_else(|error| {
        tracing::error!("starting without the saved session: {error:#}");
        None
    });
    let history = History::open(&paths.history()).unwrap_or_else(|error| {
        tracing::error!("history is kept in memory only: {error:#}");
        History::new()
    });
    let permissions = store::load_json(&paths.permissions())
        .unwrap_or_else(|error| {
            tracing::error!("site permissions reset: {error:#}");
            None
        })
        .unwrap_or_default();
    if let Err(error) = std::fs::create_dir_all(paths.webview_data()) {
        tracing::warn!("could not create the WebView2 data folder: {error}");
    }
    if let Err(error) = std::fs::create_dir_all(paths.extensions()) {
        tracing::warn!("could not create the extensions folder: {error}");
    }
    LoadedState {
        paths,
        settings,
        session,
        history,
        permissions,
    }
}

pub(super) fn log_failure(result: anyhow::Result<()>) {
    if let Err(error) = result {
        tracing::warn!("{error:#}");
    }
}

impl Shell {
    pub(super) fn schedule_session_save(&mut self, cx: &mut Context<Self>) {
        if self.session_save.is_some() {
            return;
        }
        self.session_save = Some(cx.spawn(async move |shell, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            shell
                .update(cx, |shell, _| {
                    shell.session_save = None;
                    shell.save_session_now();
                })
                .ok();
        }));
    }

    pub(super) fn save_session_now(&mut self) {
        if self.browser.private_shown() {
            return;
        }
        let session = self.browser.to_session(SavedLayout {
            compact: self.compact,
            sidebar_width: Some(f32::from(self.sidebar_width)),
            window: self.window_placement,
            site_zoom: self.site_zoom.clone(),
        });
        if self.saved_session.as_ref() == Some(&session) {
            return;
        }
        match store::save_json(&self.paths.session(), &session) {
            Ok(()) => self.saved_session = Some(session),
            Err(error) => tracing::error!("failed to save session: {error:#}"),
        }
    }

    pub(super) fn link_modifier(&self) -> LinkModifier {
        match self.settings.peek_modifier {
            PeekModifier::Shift => LinkModifier::Shift,
            PeekModifier::Ctrl => LinkModifier::Ctrl,
        }
    }
}
