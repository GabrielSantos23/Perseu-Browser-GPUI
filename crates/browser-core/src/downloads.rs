//! Downloads of the current run, newest first.

use std::path::PathBuf;

use crate::tabs::Timestamp;

const DOWNLOAD_LIMIT: usize = 100;
const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
const UNIT_STEP: f64 = 1024.0;

/// Where a download stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadState {
    /// Still receiving bytes.
    InProgress,
    /// Saved to disk.
    Completed,
    /// Stopped by the user.
    Cancelled,
    /// Stopped by a network, disk or server problem.
    Failed,
}

/// One download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    /// Identifier given by the webview host.
    pub id: u64,
    /// Where it came from.
    pub url: String,
    /// Where it is saved.
    pub path: PathBuf,
    /// Bytes received so far.
    pub received: u64,
    /// Expected size, when the server said.
    pub total: Option<u64>,
    /// Current state.
    pub state: DownloadState,
    /// Why a failed download stopped, as the engine put it.
    pub reason: Option<&'static str>,
    /// When it started.
    pub started_at: Timestamp,
}

impl Download {
    /// The saved file's name.
    pub fn file_name(&self) -> String {
        self.path.file_name().map_or_else(
            || self.url.clone(),
            |name| name.to_string_lossy().into_owned(),
        )
    }

    /// Share received, between 0 and 1, when the size is known.
    pub fn fraction(&self) -> Option<f32> {
        let total = self.total.filter(|total| *total > 0)?;
        Some((self.received as f64 / total as f64).clamp(0.0, 1.0) as f32)
    }
}

/// The downloads list.
#[derive(Debug, Default)]
pub struct Downloads {
    items: Vec<Download>,
}

impl Downloads {
    /// Creates an empty list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Downloads, newest first.
    pub fn items(&self) -> &[Download] {
        &self.items
    }

    /// How many are still receiving.
    pub fn active_count(&self) -> usize {
        self.items
            .iter()
            .filter(|download| download.state == DownloadState::InProgress)
            .count()
    }

    /// Combined share received of the running downloads whose size is known.
    pub fn overall_fraction(&self) -> Option<f32> {
        let (received, total) = self
            .items
            .iter()
            .filter(|download| download.state == DownloadState::InProgress)
            .filter_map(|download| Some((download.received, download.total?)))
            .fold((0u64, 0u64), |(received, total), (r, t)| {
                (received + r, total + t)
            });
        (total > 0).then(|| (received as f64 / total as f64).clamp(0.0, 1.0) as f32)
    }

    /// Adds a download that just started.
    pub fn start(
        &mut self,
        id: u64,
        url: String,
        path: PathBuf,
        total: Option<u64>,
        now: Timestamp,
    ) {
        self.items.retain(|download| download.id != id);
        self.items.insert(
            0,
            Download {
                id,
                url,
                path,
                received: 0,
                total,
                state: DownloadState::InProgress,
                reason: None,
                started_at: now,
            },
        );
        self.items.truncate(DOWNLOAD_LIMIT);
    }

    /// Records progress. Ignored once the download has finished.
    pub fn progress(&mut self, id: u64, received: u64, total: Option<u64>) {
        if let Some(download) = self.in_progress_mut(id) {
            download.received = received;
            if total.is_some() {
                download.total = total;
            }
        }
    }

    /// Records how a download ended, with the engine's reason when it failed.
    /// A completed download counts as fully received.
    pub fn finish(&mut self, id: u64, state: DownloadState, reason: Option<&'static str>) {
        if let Some(download) = self.in_progress_mut(id) {
            download.state = state;
            download.reason = reason;
            if state == DownloadState::Completed {
                download.total = Some(
                    download
                        .total
                        .unwrap_or(download.received)
                        .max(download.received),
                );
                download.received = download.total.unwrap_or(download.received);
            }
        }
    }

    /// Removes one finished download from the list. Downloads in progress stay.
    pub fn remove(&mut self, id: u64) {
        self.items
            .retain(|download| download.id != id || download.state == DownloadState::InProgress);
    }

    /// Removes every finished download from the list.
    pub fn clear_finished(&mut self) {
        self.items
            .retain(|download| download.state == DownloadState::InProgress);
    }

    fn in_progress_mut(&mut self, id: u64) -> Option<&mut Download> {
        self.items
            .iter_mut()
            .find(|download| download.id == id && download.state == DownloadState::InProgress)
    }
}

/// Formats a byte count for people: `"512 B"`, `"1.5 MB"`.
pub fn format_bytes(bytes: u64) -> String {
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= UNIT_STEP && unit < UNITS.len() - 1 {
        value /= UNIT_STEP;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn started() -> Downloads {
        let mut downloads = Downloads::new();
        downloads.start(
            1,
            "https://a/f.zip".into(),
            PathBuf::from(r"C:\D\f.zip"),
            Some(200),
            5,
        );
        downloads
    }

    #[test]
    fn progress_and_completion_update_the_entry() {
        let mut downloads = started();
        downloads.progress(1, 50, None);
        let download = &downloads.items()[0];
        assert_eq!(download.fraction(), Some(0.25));
        assert_eq!(download.file_name(), "f.zip");
        assert_eq!(downloads.active_count(), 1);

        downloads.finish(1, DownloadState::Completed, None);
        let download = &downloads.items()[0];
        assert_eq!(download.state, DownloadState::Completed);
        assert_eq!(download.received, 200);
        assert_eq!(downloads.active_count(), 0);
    }

    #[test]
    fn finished_downloads_ignore_late_events() {
        let mut downloads = started();
        downloads.finish(1, DownloadState::Cancelled, None);
        downloads.progress(1, 150, None);
        downloads.finish(1, DownloadState::Completed, None);
        let download = &downloads.items()[0];
        assert_eq!(download.state, DownloadState::Cancelled);
        assert_eq!(download.received, 0);
    }

    #[test]
    fn unknown_size_has_no_fraction_until_complete() {
        let mut downloads = Downloads::new();
        downloads.start(2, "u".into(), PathBuf::from("x.bin"), None, 0);
        downloads.progress(2, 300, None);
        assert_eq!(downloads.items()[0].fraction(), None);
        downloads.finish(2, DownloadState::Completed, None);
        assert_eq!(downloads.items()[0].fraction(), Some(1.0));
    }

    #[test]
    fn failed_downloads_keep_the_engine_reason() {
        let mut downloads = started();
        downloads.finish(1, DownloadState::Failed, Some("flagged as dangerous"));
        let download = &downloads.items()[0];
        assert_eq!(download.reason, Some("flagged as dangerous"));
    }

    #[test]
    fn clearing_keeps_downloads_in_progress() {
        let mut downloads = started();
        downloads.start(2, "u".into(), PathBuf::from("y"), None, 6);
        downloads.finish(2, DownloadState::Failed, None);
        downloads.remove(1);
        assert_eq!(downloads.items().len(), 2);
        downloads.clear_finished();
        assert_eq!(downloads.items().len(), 1);
        assert_eq!(downloads.items()[0].id, 1);
    }

    #[test]
    fn overall_fraction_combines_running_downloads_of_known_size() {
        let mut downloads = started();
        downloads.start(2, "u".into(), PathBuf::from("y"), Some(600), 6);
        downloads.start(3, "u".into(), PathBuf::from("z"), None, 7);
        downloads.progress(1, 200, None);
        downloads.progress(2, 0, None);
        assert_eq!(downloads.overall_fraction(), Some(0.25));
        downloads.finish(1, DownloadState::Completed, None);
        downloads.finish(2, DownloadState::Cancelled, None);
        assert_eq!(downloads.overall_fraction(), None);
    }

    #[test]
    fn bytes_are_formatted_readably() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(25 * 1024 * 1024), "25 MB");
    }
}
