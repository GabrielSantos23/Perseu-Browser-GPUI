//! Browsing history, most recent first, optionally stored in SQLite.

use std::path::Path;

use anyhow::{Context as _, Result};
use rusqlite::{Connection, params};

use crate::tabs::Timestamp;

const HISTORY_LIMIT: usize = 5_000;
const RECORDED_SCHEMES: &[&str] = &["http://", "https://", "file://"];
const SCHEMA: &str = "
    PRAGMA journal_mode = WAL;
    PRAGMA synchronous = NORMAL;
    CREATE TABLE IF NOT EXISTS visits (
        url TEXT PRIMARY KEY,
        title TEXT NOT NULL DEFAULT '',
        visit_count INTEGER NOT NULL,
        last_visit INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS visits_by_time ON visits (last_visit DESC);
";

/// One visited URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    /// The URL.
    pub url: String,
    /// Last known document title.
    pub title: String,
    /// How many times it was committed.
    pub visit_count: u32,
    /// When it was last committed.
    pub last_visit: Timestamp,
}

/// Visited pages, deduplicated by URL. The most recent entries are kept in
/// memory for searching; with a database every visit is also stored there.
#[derive(Debug, Default)]
pub struct History {
    entries: Vec<HistoryEntry>,
    db: Option<Connection>,
}

impl History {
    /// Creates an empty history kept only in memory.
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens (or creates) the history database at `path` and loads the most
    /// recent entries.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)
                .with_context(|| format!("creating {}", folder.display()))?;
        }
        let db = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        db.execute_batch(SCHEMA)
            .context("creating history tables")?;
        let entries = {
            let mut query = db.prepare(
                "SELECT url, title, visit_count, last_visit FROM visits
                 ORDER BY last_visit DESC LIMIT ?1",
            )?;
            query
                .query_map(params![HISTORY_LIMIT as i64], |row| {
                    Ok(HistoryEntry {
                        url: row.get(0)?,
                        title: row.get(1)?,
                        visit_count: row.get(2)?,
                        last_visit: row.get::<_, i64>(3)?.max(0) as Timestamp,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        Ok(Self {
            entries,
            db: Some(db),
        })
    }

    /// Entries, most recently visited first.
    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    /// Records a committed navigation. Internal and blank pages are ignored.
    /// The in-memory list is updated even when the database write fails.
    pub fn record_visit(&mut self, url: &str, now: Timestamp) -> Result<()> {
        if !RECORDED_SCHEMES
            .iter()
            .any(|scheme| url.starts_with(scheme))
        {
            return Ok(());
        }
        let mut entry = match self.entries.iter().position(|entry| entry.url == url) {
            Some(index) => self.entries.remove(index),
            None => HistoryEntry {
                url: url.to_string(),
                title: String::new(),
                visit_count: 0,
                last_visit: now,
            },
        };
        entry.visit_count += 1;
        entry.last_visit = now;
        self.entries.insert(0, entry);
        self.entries.truncate(HISTORY_LIMIT);
        if let Some(db) = &self.db {
            db.execute(
                "INSERT INTO visits (url, visit_count, last_visit) VALUES (?1, 1, ?2)
                 ON CONFLICT (url) DO UPDATE SET
                     visit_count = visit_count + 1, last_visit = excluded.last_visit",
                params![url, now as i64],
            )
            .context("saving visit")?;
        }
        Ok(())
    }

    /// Deletes every recorded visit, in memory and in the database.
    pub fn clear(&mut self) -> Result<()> {
        self.entries.clear();
        if let Some(db) = &self.db {
            db.execute("DELETE FROM visits", [])
                .context("clearing history")?;
            db.execute_batch("VACUUM").context("compacting history")?;
        }
        Ok(())
    }

    /// Forgets one URL.
    pub fn forget(&mut self, url: &str) -> Result<()> {
        self.entries.retain(|entry| entry.url != url);
        if let Some(db) = &self.db {
            db.execute("DELETE FROM visits WHERE url = ?1", params![url])
                .context("forgetting visit")?;
        }
        Ok(())
    }

    /// Forgets every URL last visited at or after `since`; `0` clears it all.
    pub fn clear_since(&mut self, since: Timestamp) -> Result<()> {
        if since == 0 {
            return self.clear();
        }
        self.entries.retain(|entry| entry.last_visit < since);
        if let Some(db) = &self.db {
            db.execute(
                "DELETE FROM visits WHERE last_visit >= ?1",
                params![since as i64],
            )
            .context("clearing recent history")?;
        }
        Ok(())
    }

    /// Adds visits brought from another browser. A URL already here keeps
    /// its own title and gets the later visit and the larger count. Returns
    /// how many URLs are new.
    pub fn import(&mut self, visits: &[HistoryEntry]) -> Result<usize> {
        let visits: Vec<&HistoryEntry> = visits
            .iter()
            .filter(|visit| {
                RECORDED_SCHEMES
                    .iter()
                    .any(|scheme| visit.url.starts_with(scheme))
            })
            .collect();
        let mut added = 0;
        for visit in &visits {
            match self.entries.iter_mut().find(|entry| entry.url == visit.url) {
                Some(entry) => {
                    entry.last_visit = entry.last_visit.max(visit.last_visit);
                    entry.visit_count = entry.visit_count.max(visit.visit_count);
                }
                None => {
                    added += 1;
                    self.entries.push((*visit).clone());
                }
            }
        }
        self.entries
            .sort_by_key(|entry| std::cmp::Reverse(entry.last_visit));
        self.entries.truncate(HISTORY_LIMIT);
        if let Some(db) = &mut self.db {
            let batch = db.transaction().context("starting import")?;
            for visit in visits {
                batch
                    .execute(
                        "INSERT INTO visits (url, title, visit_count, last_visit)
                         VALUES (?1, ?2, ?3, ?4)
                         ON CONFLICT (url) DO UPDATE SET
                             visit_count = max(visit_count, excluded.visit_count),
                             last_visit = max(last_visit, excluded.last_visit)",
                        params![
                            visit.url,
                            visit.title,
                            visit.visit_count.max(1),
                            visit.last_visit as i64
                        ],
                    )
                    .context("importing visit")?;
            }
            batch.commit().context("saving import")?;
        }
        Ok(added)
    }

    /// Updates the title of a recorded URL.
    pub fn set_title(&mut self, url: &str, title: &str) -> Result<()> {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.url == url) else {
            return Ok(());
        };
        entry.title = title.to_string();
        if let Some(db) = &self.db {
            db.execute(
                "UPDATE visits SET title = ?2 WHERE url = ?1",
                params![url, title],
            )
            .context("saving title")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::temp_dir;

    #[test]
    fn revisits_move_to_front_and_count() {
        let mut history = History::new();
        history.record_visit("https://a.com/", 1).unwrap();
        history.record_visit("https://b.com/", 2).unwrap();
        history.record_visit("https://a.com/", 3).unwrap();
        let entries = history.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].url, "https://a.com/");
        assert_eq!((entries[0].visit_count, entries[0].last_visit), (2, 3));
    }

    #[test]
    fn ignores_blank_and_internal_pages() {
        let mut history = History::new();
        history.record_visit("about:blank", 1).unwrap();
        history.record_visit("data:text/html,x", 1).unwrap();
        history.record_visit("perseu://settings", 1).unwrap();
        assert!(history.entries().is_empty());
    }

    #[test]
    fn titles_attach_to_recorded_urls() {
        let mut history = History::new();
        history.record_visit("https://a.com/", 1).unwrap();
        history.set_title("https://a.com/", "A").unwrap();
        history.set_title("https://unknown.com/", "X").unwrap();
        assert_eq!(history.entries()[0].title, "A");
        assert_eq!(history.entries().len(), 1);
    }

    #[test]
    fn is_bounded() {
        let mut history = History::new();
        for index in 0..HISTORY_LIMIT + 10 {
            history
                .record_visit(&format!("https://site{index}.com/"), index as u64)
                .unwrap();
        }
        assert_eq!(history.entries().len(), HISTORY_LIMIT);
        assert_eq!(
            history.entries()[0].url,
            format!("https://site{}.com/", HISTORY_LIMIT + 9)
        );
    }

    #[test]
    fn database_keeps_visits_and_titles_across_opens() {
        let path = temp_dir("history-db").join("history.sqlite");
        {
            let mut history = History::open(&path).unwrap();
            history.record_visit("https://a.com/", 1).unwrap();
            history.record_visit("https://b.com/", 2).unwrap();
            history.record_visit("https://a.com/", 3).unwrap();
            history.set_title("https://a.com/", "A").unwrap();
        }
        let history = History::open(&path).unwrap();
        let entries = history.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].url, "https://a.com/");
        assert_eq!(entries[0].title, "A");
        assert_eq!((entries[0].visit_count, entries[0].last_visit), (2, 3));
        assert_eq!(entries[1].url, "https://b.com/");
    }

    #[test]
    fn forgetting_and_clearing_a_range_reach_the_database() {
        let path = temp_dir("history-range").join("history.sqlite");
        {
            let mut history = History::open(&path).unwrap();
            history.record_visit("https://old.com/", 1).unwrap();
            history.record_visit("https://new.com/", 10).unwrap();
            history.record_visit("https://gone.com/", 11).unwrap();
            history.forget("https://gone.com/").unwrap();
            history.clear_since(5).unwrap();
        }
        let urls: Vec<String> = History::open(&path)
            .unwrap()
            .entries()
            .iter()
            .map(|entry| entry.url.clone())
            .collect();
        assert_eq!(urls, ["https://old.com/"]);
    }

    #[test]
    fn imported_visits_merge_with_our_own() {
        let path = temp_dir("history-import").join("history.sqlite");
        let visit = |url: &str, count, at| HistoryEntry {
            url: url.to_string(),
            title: "Imported".to_string(),
            visit_count: count,
            last_visit: at,
        };
        {
            let mut history = History::open(&path).unwrap();
            history.record_visit("https://a.com/", 5).unwrap();
            history.set_title("https://a.com/", "Ours").unwrap();
            let added = history
                .import(&[
                    visit("https://a.com/", 3, 2),
                    visit("https://b.com/", 1, 9),
                    visit("chrome://settings", 1, 9),
                ])
                .unwrap();
            assert_eq!(added, 1);
        }
        let history = History::open(&path).unwrap();
        let entries = history.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].url, "https://b.com/");
        assert_eq!(
            (
                entries[1].title.as_str(),
                entries[1].visit_count,
                entries[1].last_visit
            ),
            ("Ours", 3, 5)
        );
    }

    #[test]
    fn clearing_removes_stored_visits() {
        let path = temp_dir("history-clear").join("history.sqlite");
        {
            let mut history = History::open(&path).unwrap();
            history.record_visit("https://a.com/", 1).unwrap();
            history.clear().unwrap();
            assert!(history.entries().is_empty());
        }
        assert!(History::open(&path).unwrap().entries().is_empty());
    }
}
