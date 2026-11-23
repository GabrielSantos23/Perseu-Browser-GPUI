use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use browser_core::APP_NAME;
use futures::channel::mpsc::UnboundedSender;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

const POLL: Duration = Duration::from_millis(800);
const WEBVIEW_PROCESS: &str = "msedgewebview2";

/// What this app's pages are playing, as Windows' media controls report it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    pub playing: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum MediaCommand {
    TogglePlay,
    Next,
    Previous,
}

/// Watches the system media sessions on a background thread. Snapshots
/// arrive on `updates` when they change; commands go to the returned sender.
pub fn watch(updates: UnboundedSender<Option<MediaInfo>>) -> Sender<MediaCommand> {
    let (commands, receiver) = channel();
    let spawned = std::thread::Builder::new()
        .name("media-sessions".into())
        .spawn(move || run(updates, receiver));
    if let Err(error) = spawned {
        tracing::warn!("media controls unavailable: {error}");
    }
    commands
}

fn run(updates: UnboundedSender<Option<MediaInfo>>, commands: Receiver<MediaCommand>) {
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let manager = match Manager::RequestAsync().and_then(|request| request.join()) {
        Ok(manager) => manager,
        Err(error) => {
            tracing::warn!("media controls unavailable: {error}");
            return;
        }
    };
    let mut last: Option<MediaInfo> = None;
    let mut first = true;
    loop {
        let session = ours(&manager);
        let info = session.as_ref().and_then(|session| info_of(session).ok());
        if first || info != last {
            if updates.unbounded_send(info.clone()).is_err() {
                return;
            }
            last = info;
            first = false;
        }
        match commands.recv_timeout(POLL) {
            Ok(command) => {
                if let Some(session) = session {
                    send(&session, command);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn is_ours(app_id: &str) -> bool {
    let app_id = app_id.to_ascii_lowercase();
    app_id.contains(APP_NAME) || app_id.contains(WEBVIEW_PROCESS)
}

fn ours(manager: &Manager) -> Option<Session> {
    let sessions = manager.GetSessions().ok()?;
    let mut fallback = None;
    for index in 0..sessions.Size().ok()? {
        let Ok(session) = sessions.GetAt(index) else {
            continue;
        };
        let app_id = session.SourceAppUserModelId().unwrap_or_default();
        if !is_ours(&app_id.to_string()) {
            continue;
        }
        let playing = session
            .GetPlaybackInfo()
            .and_then(|info| info.PlaybackStatus())
            .is_ok_and(|status| status == Status::Playing);
        if playing {
            return Some(session);
        }
        fallback.get_or_insert(session);
    }
    fallback
}

fn info_of(session: &Session) -> windows::core::Result<MediaInfo> {
    let properties = session.TryGetMediaPropertiesAsync()?.join()?;
    let status = session.GetPlaybackInfo()?.PlaybackStatus()?;
    Ok(MediaInfo {
        title: properties.Title()?.to_string(),
        artist: properties.Artist()?.to_string(),
        playing: status == Status::Playing,
    })
}

fn send(session: &Session, command: MediaCommand) {
    let request = match command {
        MediaCommand::TogglePlay => session.TryTogglePlayPauseAsync(),
        MediaCommand::Next => session.TrySkipNextAsync(),
        MediaCommand::Previous => session.TrySkipPreviousAsync(),
    };
    if let Err(error) = request.and_then(|request| request.join()) {
        tracing::warn!("media command failed: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_this_app_and_its_webviews_count() {
        assert!(is_ours("perseu.exe"));
        assert!(is_ours("MSEdgeWebView2.exe"));
        assert!(!is_ours("Spotify.exe"));
        assert!(!is_ours("Chrome"));
    }
}
