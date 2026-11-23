use std::path::PathBuf;

use browser_core::APP_NAME;
use directories::{BaseDirs, UserDirs};

const SESSION_FILE: &str = "session.json";
const SETTINGS_FILE: &str = "settings.json";
const HISTORY_FILE: &str = "history.sqlite";
const PERMISSIONS_FILE: &str = "permissions.json";
const WEBVIEW_FOLDER: &str = "WebView2";
const EXTENSIONS_FOLDER: &str = "Extensions";
const SHAZAM_FOLDER: &str = "Shazam";
const LOG_EXTENSION: &str = "log";
/// The app's name before it became Perseu; its data moves over once.
const OLD_APP_NAME: &str = "sidebrowser";

pub struct Paths {
    roaming: PathBuf,
    local: PathBuf,
}

impl Paths {
    pub fn detect() -> Self {
        if let Some(root) = sandbox_root() {
            let app = root.join(APP_NAME);
            tracing::warn!("sandboxed by PERSEU_DATA_ROOT; data in {}", app.display());
            return Self {
                roaming: app.clone(),
                local: app,
            };
        }
        match BaseDirs::new() {
            Some(dirs) => Self {
                roaming: adopt_old_folder(dirs.config_dir()),
                local: adopt_old_folder(dirs.data_local_dir()),
            },
            None => {
                let fallback = std::env::temp_dir().join(APP_NAME);
                tracing::warn!(
                    "no user profile folders; keeping data in {}",
                    fallback.display()
                );
                Self {
                    roaming: fallback.clone(),
                    local: fallback,
                }
            }
        }
    }

    pub fn session(&self) -> PathBuf {
        self.roaming.join(SESSION_FILE)
    }

    pub fn settings(&self) -> PathBuf {
        self.roaming.join(SETTINGS_FILE)
    }

    pub fn history(&self) -> PathBuf {
        self.roaming.join(HISTORY_FILE)
    }

    pub fn permissions(&self) -> PathBuf {
        self.roaming.join(PERMISSIONS_FILE)
    }

    pub fn log_file(&self) -> PathBuf {
        self.local.join(APP_NAME).with_extension(LOG_EXTENSION)
    }

    pub fn webview_data(&self) -> PathBuf {
        self.local.join(WEBVIEW_FOLDER)
    }

    pub fn extensions(&self) -> PathBuf {
        self.local.join(EXTENSIONS_FOLDER)
    }

    /// The Shazam extension package, downloaded for song identification.
    pub fn shazam(&self) -> PathBuf {
        self.local.join(SHAZAM_FOLDER)
    }
}

pub fn system_downloads() -> Option<PathBuf> {
    UserDirs::new().and_then(|dirs| dirs.download_dir().map(PathBuf::from))
}

/// An alternate data root, so a benchmark or verification run can live beside
/// the daily instance instead of inside it. `PERSEU_DATA_ROOT` moves
/// every file the app writes — session, settings, history, log, WebView2
/// data, extensions — under one folder, and names its own single-instance
/// mutex, so the two apps do not collide.
fn sandbox_root() -> Option<PathBuf> {
    std::env::var_os("PERSEU_DATA_ROOT")
        .map(PathBuf::from)
        .filter(|root| !root.as_os_str().is_empty())
}

/// `base\perseu`, moving in what the app kept under its old name: the
/// whole folder when there is none yet, or else each item it does not
/// already hold (another app may own a folder of the same name).
fn adopt_old_folder(base: &std::path::Path) -> PathBuf {
    let folder = base.join(APP_NAME);
    let old = base.join(OLD_APP_NAME);
    if !old.is_dir() {
        return folder;
    }
    let moved = if folder.exists() {
        std::fs::read_dir(&old).and_then(|entries| {
            for entry in entries {
                let entry = entry?;
                let target = folder.join(entry.file_name());
                if !target.exists() {
                    std::fs::rename(entry.path(), target)?;
                }
            }
            Ok(())
        })
    } else {
        std::fs::rename(&old, &folder)
    };
    match moved {
        Ok(()) => {
            let _ = std::fs::remove_dir(&old);
        }
        Err(error) => tracing::warn!(
            "could not move {} into {}: {error}",
            old.display(),
            folder.display()
        ),
    }
    folder
}
