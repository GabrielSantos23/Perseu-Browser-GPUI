//! Reads bookmarks and history from other browsers on this computer: the
//! Chromium family (Chrome, Edge, Brave, ...) and the Firefox family
//! (Firefox, Zen, ...). Pure readers: databases are copied before they are
//! opened, so a running browser is never disturbed, and nothing here writes
//! to this browser's own stores.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::{Connection, OpenFlags, params};
use serde_json::Value;

use crate::history::HistoryEntry;

/// How far back history is read, in days.
pub const HISTORY_DAYS: u64 = 180;
/// Visits read from one profile.
pub const MAX_VISITS: usize = 20_000;
/// Bookmarks brought over at once; past this the import stops.
pub const MAX_BOOKMARKS: usize = 500;

/// Seconds between 1601-01-01 (Chromium's epoch) and the Unix epoch.
const CHROMIUM_EPOCH_OFFSET_SECONDS: i64 = 11_644_473_600;
const MS_PER_DAY: u64 = 86_400_000;
const LINK_SCHEMES: &[&str] = &["http://", "https://", "file://"];

/// A browser this one can import from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceBrowser {
    Chrome,
    Edge,
    Brave,
    Vivaldi,
    Opera,
    OperaGx,
    Chromium,
    Helium,
    Firefox,
    Zen,
    Floorp,
    LibreWolf,
    Waterfox,
}

impl SourceBrowser {
    /// In the order they are offered.
    pub const ALL: [SourceBrowser; 13] = [
        SourceBrowser::Chrome,
        SourceBrowser::Edge,
        SourceBrowser::Brave,
        SourceBrowser::Helium,
        SourceBrowser::Firefox,
        SourceBrowser::Zen,
        SourceBrowser::Vivaldi,
        SourceBrowser::Opera,
        SourceBrowser::OperaGx,
        SourceBrowser::Chromium,
        SourceBrowser::Floorp,
        SourceBrowser::LibreWolf,
        SourceBrowser::Waterfox,
    ];

    pub fn name(self) -> &'static str {
        match self {
            SourceBrowser::Chrome => "Chrome",
            SourceBrowser::Edge => "Edge",
            SourceBrowser::Brave => "Brave",
            SourceBrowser::Vivaldi => "Vivaldi",
            SourceBrowser::Opera => "Opera",
            SourceBrowser::OperaGx => "Opera GX",
            SourceBrowser::Chromium => "Chromium",
            SourceBrowser::Helium => "Helium",
            SourceBrowser::Firefox => "Firefox",
            SourceBrowser::Zen => "Zen",
            SourceBrowser::Floorp => "Floorp",
            SourceBrowser::LibreWolf => "LibreWolf",
            SourceBrowser::Waterfox => "Waterfox",
        }
    }

    fn is_firefox_family(self) -> bool {
        matches!(
            self,
            SourceBrowser::Firefox
                | SourceBrowser::Zen
                | SourceBrowser::Floorp
                | SourceBrowser::LibreWolf
                | SourceBrowser::Waterfox
        )
    }

    /// Where the browser keeps its profiles, under `%LOCALAPPDATA%` or
    /// `%APPDATA%`.
    fn root(self, local: &Path, roaming: &Path) -> PathBuf {
        let (base, parts): (&Path, &[&str]) = match self {
            SourceBrowser::Chrome => (local, &["Google", "Chrome", "User Data"]),
            SourceBrowser::Edge => (local, &["Microsoft", "Edge", "User Data"]),
            SourceBrowser::Brave => (local, &["BraveSoftware", "Brave-Browser", "User Data"]),
            SourceBrowser::Vivaldi => (local, &["Vivaldi", "User Data"]),
            SourceBrowser::Chromium => (local, &["Chromium", "User Data"]),
            SourceBrowser::Helium => (local, &["imput", "Helium", "User Data"]),
            SourceBrowser::Opera => (roaming, &["Opera Software", "Opera Stable"]),
            SourceBrowser::OperaGx => (roaming, &["Opera Software", "Opera GX Stable"]),
            SourceBrowser::Firefox => (roaming, &["Mozilla", "Firefox"]),
            SourceBrowser::Zen => (roaming, &["zen"]),
            SourceBrowser::Floorp => (roaming, &["Floorp"]),
            SourceBrowser::LibreWolf => (roaming, &["librewolf"]),
            SourceBrowser::Waterfox => (roaming, &["Waterfox"]),
        };
        parts
            .iter()
            .fold(base.to_path_buf(), |path, part| path.join(part))
    }
}

/// One profile of a source browser that has something to import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceProfile {
    pub name: String,
    /// The profile's own folder.
    pub dir: PathBuf,
    pub bookmarks: bool,
    pub history: bool,
}

/// A browser found on this computer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub browser: SourceBrowser,
    pub profiles: Vec<SourceProfile>,
}

/// A bookmark, under the names of the folders that hold it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    pub title: String,
    pub url: String,
    /// The outermost folder it sits in, if any; deeper folders are folded
    /// into it.
    pub folder: Option<String>,
}

/// Why something could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportError {
    /// The profile or its file is not there any more.
    Missing,
    /// The browser holds the file locked while it runs.
    Busy,
    /// The file is not in the shape this browser writes.
    Unreadable,
}

impl ImportError {
    /// A sentence for the user about `browser`.
    pub fn describe(self, browser: SourceBrowser) -> String {
        let name = browser.name();
        match self {
            ImportError::Busy => format!("Quit {name} first. It keeps its data locked while open."),
            ImportError::Missing | ImportError::Unreadable => {
                format!("Couldn't read what {name} keeps.")
            }
        }
    }
}

impl From<std::io::Error> for ImportError {
    fn from(error: std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::NotFound => ImportError::Missing,
            // ERROR_SHARING_VIOLATION and ERROR_LOCK_VIOLATION.
            _ if matches!(error.raw_os_error(), Some(32 | 33)) => ImportError::Busy,
            _ => ImportError::Unreadable,
        }
    }
}

impl From<rusqlite::Error> for ImportError {
    fn from(_: rusqlite::Error) -> Self {
        ImportError::Unreadable
    }
}

/// Browsers on this computer with at least one profile to import from.
pub fn discover() -> Vec<Source> {
    let (Some(local), Some(roaming)) = (
        std::env::var_os("LOCALAPPDATA"),
        std::env::var_os("APPDATA"),
    ) else {
        return Vec::new();
    };
    discover_in(Path::new(&local), Path::new(&roaming))
}

fn discover_in(local: &Path, roaming: &Path) -> Vec<Source> {
    SourceBrowser::ALL
        .into_iter()
        .filter_map(|browser| {
            let root = browser.root(local, roaming);
            let profiles = if browser.is_firefox_family() {
                firefox_profiles(&root)
            } else {
                chromium_profiles(&root)
            };
            (!profiles.is_empty()).then_some(Source { browser, profiles })
        })
        .collect()
}

fn profile(name: String, dir: PathBuf, bookmarks: &str, history: &str) -> Option<SourceProfile> {
    let bookmarks = dir.join(bookmarks).is_file();
    let history = dir.join(history).is_file();
    (bookmarks || history).then_some(SourceProfile {
        name,
        dir,
        bookmarks,
        history,
    })
}

/// Profiles named in `Local State`; Opera keeps its one profile in the root.
fn chromium_profiles(root: &Path) -> Vec<SourceProfile> {
    let named: Vec<(String, String)> = std::fs::read(root.join("Local State"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|state| {
            let cache = state.pointer("/profile/info_cache")?.as_object()?.clone();
            Some(
                cache
                    .into_iter()
                    .map(|(dir, info)| {
                        let name = info["name"].as_str().unwrap_or(&dir).to_string();
                        (dir, name)
                    })
                    .collect(),
            )
        })
        .unwrap_or_default();
    let mut profiles: Vec<SourceProfile> = named
        .into_iter()
        .filter(|(dir, _)| !dir.contains(['/', '\\']) && dir != "..")
        .filter_map(|(dir, name)| profile(name, root.join(dir), "Bookmarks", "History"))
        .collect();
    if profiles.is_empty()
        && let Some(own) = profile("Default".into(), root.to_path_buf(), "Bookmarks", "History")
    {
        profiles.push(own);
    }
    profiles.sort_by(|a, b| a.dir.cmp(&b.dir));
    profiles
}

/// Profiles listed in `profiles.ini`.
fn firefox_profiles(root: &Path) -> Vec<SourceProfile> {
    let Ok(ini) = std::fs::read_to_string(root.join("profiles.ini")) else {
        return Vec::new();
    };
    let mut profiles = Vec::new();
    let mut section: Option<(Option<String>, Option<String>, bool)> = None;
    let mut finish = |section: Option<(Option<String>, Option<String>, bool)>| {
        if let Some((Some(name), Some(path), relative)) = section {
            let dir = if relative {
                root.join(path.replace('/', "\\"))
            } else {
                PathBuf::from(path)
            };
            if let Some(found) = profile(name, dir, "places.sqlite", "places.sqlite") {
                profiles.push(found);
            }
        }
    };
    for line in ini.lines().map(str::trim) {
        if line.starts_with('[') {
            finish(section.take());
            if line.to_ascii_lowercase().starts_with("[profile") {
                section = Some((None, None, true));
            }
        } else if let (Some(current), Some((key, value))) = (section.as_mut(), line.split_once('='))
        {
            match key.trim() {
                "Name" => current.0 = Some(value.trim().to_string()),
                "Path" => current.1 = Some(value.trim().to_string()),
                "IsRelative" => current.2 = value.trim() == "1",
                _ => {}
            }
        }
    }
    finish(section);
    profiles
}

/// The bookmarks of a profile, in the browser's own order.
pub fn bookmarks(browser: SourceBrowser, profile: &Path) -> Result<Vec<Bookmark>, ImportError> {
    let mut found = if browser.is_firefox_family() {
        firefox_bookmarks(profile)?
    } else {
        let bytes = std::fs::read(profile.join("Bookmarks"))?;
        chromium_bookmarks(&bytes)?
    };
    found.retain(|bookmark| is_link(&bookmark.url));
    Ok(found)
}

fn is_link(url: &str) -> bool {
    LINK_SCHEMES.iter().any(|scheme| url.starts_with(scheme))
}

/// The bar's top-level links stay loose, its folders keep their names, and
/// the other roots each become a folder of their own.
fn chromium_bookmarks(bytes: &[u8]) -> Result<Vec<Bookmark>, ImportError> {
    let file: Value = serde_json::from_slice(bytes).map_err(|_| ImportError::Unreadable)?;
    let roots = file.get("roots").ok_or(ImportError::Unreadable)?;
    let mut found = Vec::new();
    if let Some(bar) = roots.get("bookmark_bar") {
        for child in children(bar) {
            match child["type"].as_str() {
                Some("url") => push_link(&mut found, child, None),
                Some("folder") => {
                    let name = child["name"].as_str().unwrap_or_default().to_string();
                    collect_links(&mut found, child, &name);
                }
                _ => {}
            }
        }
    }
    for root in ["other", "synced"] {
        if let Some(folder) = roots.get(root) {
            let name = folder["name"].as_str().unwrap_or(root).to_string();
            collect_links(&mut found, folder, &name);
        }
    }
    Ok(found)
}

fn children(folder: &Value) -> impl Iterator<Item = &Value> {
    folder["children"].as_array().into_iter().flatten()
}

fn push_link(found: &mut Vec<Bookmark>, node: &Value, folder: Option<&str>) {
    if let Some(url) = node["url"].as_str() {
        found.push(Bookmark {
            title: node["name"].as_str().unwrap_or_default().to_string(),
            url: url.to_string(),
            folder: folder.map(str::to_string),
        });
    }
}

fn collect_links(found: &mut Vec<Bookmark>, folder: &Value, name: &str) {
    for child in children(folder) {
        match child["type"].as_str() {
            Some("url") => push_link(found, child, Some(name)),
            Some("folder") => collect_links(found, child, name),
            _ => {}
        }
    }
}

const FIREFOX_TOOLBAR: &str = "toolbar_____";
const FIREFOX_ROOTS: [(&str, &str); 2] = [
    ("menu________", "Bookmarks Menu"),
    ("unfiled_____", "Other Bookmarks"),
];

/// Same layout as Chromium's: the toolbar's top level stays loose, the
/// menu and other bookmarks become folders.
fn firefox_bookmarks(profile: &Path) -> Result<Vec<Bookmark>, ImportError> {
    let snapshot = Snapshot::open(&profile.join("places.sqlite"))?;
    let mut query = snapshot.conn.prepare(
        "SELECT b.id, b.parent, b.type, coalesce(b.title, ''), coalesce(p.url, ''), b.guid
         FROM moz_bookmarks b LEFT JOIN moz_places p ON p.id = b.fk
         ORDER BY b.parent, b.position",
    )?;
    struct Node {
        id: i64,
        parent: i64,
        folder: bool,
        title: String,
        url: String,
        guid: String,
    }
    let nodes: Vec<Node> = query
        .query_map([], |row| {
            Ok(Node {
                id: row.get(0)?,
                parent: row.get(1)?,
                folder: row.get::<_, i64>(2)? == 2,
                title: row.get(3)?,
                url: row.get(4)?,
                guid: row.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    fn walk(
        nodes: &[Node],
        parent: i64,
        folder: Option<&str>,
        found: &mut Vec<Bookmark>,
        depth: u8,
    ) {
        if depth > 32 {
            return;
        }
        for node in nodes.iter().filter(|node| node.parent == parent) {
            if node.folder {
                walk(
                    nodes,
                    node.id,
                    Some(folder.unwrap_or(&node.title)),
                    found,
                    depth + 1,
                );
            } else if !node.url.is_empty() {
                found.push(Bookmark {
                    title: node.title.clone(),
                    url: node.url.clone(),
                    folder: folder.map(str::to_string),
                });
            }
        }
    }
    let root_id = |guid: &str| {
        nodes
            .iter()
            .find(|node| node.guid == guid)
            .map(|node| node.id)
    };
    let mut found = Vec::new();
    if let Some(toolbar) = root_id(FIREFOX_TOOLBAR) {
        walk(&nodes, toolbar, None, &mut found, 0);
    }
    for (guid, name) in FIREFOX_ROOTS {
        if let Some(root) = root_id(guid) {
            walk(&nodes, root, Some(name), &mut found, 0);
        }
    }
    Ok(found)
}

/// Pages a profile visited in the last [`HISTORY_DAYS`], newest first, with
/// `last_visit` in Unix milliseconds.
pub fn history(
    browser: SourceBrowser,
    profile: &Path,
    now_ms: u64,
) -> Result<Vec<HistoryEntry>, ImportError> {
    let since_ms = now_ms.saturating_sub(HISTORY_DAYS * MS_PER_DAY) as i64;
    let (file, sql, since, to_ms): (&str, &str, i64, fn(i64) -> i64) =
        if browser.is_firefox_family() {
            (
                "places.sqlite",
                "SELECT url, coalesce(title, ''), visit_count, last_visit_date FROM moz_places
                 WHERE last_visit_date >= ?1 ORDER BY last_visit_date DESC LIMIT ?2",
                since_ms * 1_000,
                |micros| micros / 1_000,
            )
        } else {
            (
                "History",
                "SELECT url, coalesce(title, ''), visit_count, last_visit_time FROM urls
                 WHERE last_visit_time >= ?1 ORDER BY last_visit_time DESC LIMIT ?2",
                (since_ms / 1_000 + CHROMIUM_EPOCH_OFFSET_SECONDS) * 1_000_000,
                |micros| micros / 1_000 - CHROMIUM_EPOCH_OFFSET_SECONDS * 1_000,
            )
        };
    let snapshot = Snapshot::open(&profile.join(file))?;
    let mut query = snapshot.conn.prepare(sql)?;
    let visits = query
        .query_map(params![since, MAX_VISITS as i64], |row| {
            Ok(HistoryEntry {
                url: row.get(0)?,
                title: row.get(1)?,
                visit_count: row.get::<_, i64>(2)?.clamp(1, i64::from(u32::MAX)) as u32,
                last_visit: to_ms(row.get(3)?).max(0) as u64,
            })
        })?
        .filter_map(Result::ok)
        .filter(|visit| is_link(&visit.url))
        .collect();
    Ok(visits)
}

/// A private copy of a database and its write-ahead log, removed when
/// dropped. A running browser is never disturbed and SQLite recovers the copy
/// as it would after a crash.
struct Snapshot {
    conn: Connection,
    dir: PathBuf,
}

impl Snapshot {
    fn open(path: &Path) -> Result<Self, ImportError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        if !path.is_file() {
            return Err(ImportError::Missing);
        }
        let dir = std::env::temp_dir().join(format!(
            "perseu-import-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir)?;
        let copy = dir.join("snapshot.sqlite");
        let copied = (|| -> Result<(), ImportError> {
            std::fs::copy(path, &copy)?;
            for suffix in ["-wal", "-journal"] {
                let mut companion = path.as_os_str().to_owned();
                companion.push(suffix);
                let mut target = copy.as_os_str().to_owned();
                target.push(suffix);
                match std::fs::copy(&companion, &target) {
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
            Ok(())
        })();
        if let Err(error) = copied {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(error);
        }
        // Read-write only so SQLite can replay the copied log into the copy.
        let conn = match Connection::open_with_flags(
            &copy,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) {
            Ok(conn) => conn,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(error.into());
            }
        };
        Ok(Self { conn, dir })
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::temp_dir;

    #[test]
    fn chromium_bar_links_stay_loose_and_folders_fold_into_their_top() {
        let file = r#"{"roots":{
            "bookmark_bar":{"children":[
                {"type":"url","name":"Docs","url":"https://docs.rs/"},
                {"type":"folder","name":"Work","children":[
                    {"type":"folder","name":"Deep","children":[
                        {"type":"url","name":"Mail","url":"https://mail.example/"}]}]},
                {"type":"url","name":"Settings","url":"chrome://settings"}]},
            "other":{"name":"Other bookmarks","children":[
                {"type":"url","name":"News","url":"https://news.example/"}]},
            "synced":{"name":"Mobile bookmarks","children":[]}}}"#;
        let found = chromium_bookmarks(file.as_bytes()).unwrap();
        let shape: Vec<(&str, Option<&str>)> = found
            .iter()
            .filter(|bookmark| is_link(&bookmark.url))
            .map(|bookmark| (bookmark.title.as_str(), bookmark.folder.as_deref()))
            .collect();
        assert_eq!(
            shape,
            [
                ("Docs", None),
                ("Mail", Some("Work")),
                ("News", Some("Other bookmarks"))
            ]
        );
    }

    #[test]
    fn profiles_are_found_where_each_family_keeps_them() {
        let home = temp_dir("import-discover");
        let local = home.join("Local");
        let roaming = home.join("Roaming");
        let chrome = local.join("Google").join("Chrome").join("User Data");
        std::fs::create_dir_all(chrome.join("Profile 1")).unwrap();
        std::fs::create_dir_all(chrome.join("Empty")).unwrap();
        std::fs::write(
            chrome.join("Local State"),
            r#"{"profile":{"info_cache":{"Profile 1":{"name":"Work"},"Empty":{"name":"X"},"..\\..":{"name":"Escape"}}}}"#,
        )
        .unwrap();
        std::fs::write(chrome.join("Profile 1").join("Bookmarks"), "{}").unwrap();
        let firefox = roaming.join("Mozilla").join("Firefox");
        std::fs::create_dir_all(firefox.join("Profiles").join("abc.default")).unwrap();
        std::fs::write(
            firefox.join("profiles.ini"),
            "[General]\nVersion=2\n\n[Profile0]\nName=default\nIsRelative=1\nPath=Profiles/abc.default\n",
        )
        .unwrap();
        std::fs::write(
            firefox
                .join("Profiles")
                .join("abc.default")
                .join("places.sqlite"),
            "",
        )
        .unwrap();

        let found = discover_in(&local, &roaming);
        let summary: Vec<(&str, Vec<&str>)> = found
            .iter()
            .map(|source| {
                (
                    source.browser.name(),
                    source
                        .profiles
                        .iter()
                        .map(|profile| profile.name.as_str())
                        .collect(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [("Chrome", vec!["Work"]), ("Firefox", vec!["default"])]
        );
        assert!(found[0].profiles[0].bookmarks && !found[0].profiles[0].history);
    }

    #[test]
    fn chromium_history_is_read_from_a_copy_in_unix_milliseconds() {
        let profile = temp_dir("import-history");
        std::fs::create_dir_all(&profile).unwrap();
        let db = Connection::open(profile.join("History")).unwrap();
        db.execute_batch(
            "CREATE TABLE urls (url TEXT, title TEXT, visit_count INTEGER, last_visit_time INTEGER);",
        )
        .unwrap();
        let now_ms: u64 = 1_795_000_000_000;
        let chromium = |ms: u64| (ms as i64 / 1_000 + CHROMIUM_EPOCH_OFFSET_SECONDS) * 1_000_000;
        db.execute(
            "INSERT INTO urls VALUES ('https://a.example/', 'A', 3, ?1), ('https://old.example/', 'Old', 1, ?2), ('chrome://flags', 'F', 1, ?1)",
            params![chromium(now_ms - 1_000), chromium(now_ms - 400 * MS_PER_DAY)],
        )
        .unwrap();
        drop(db);
        let visits = history(SourceBrowser::Chrome, &profile, now_ms).unwrap();
        assert_eq!(visits.len(), 1);
        assert_eq!(visits[0].url, "https://a.example/");
        assert_eq!(visits[0].visit_count, 3);
        assert_eq!(visits[0].last_visit, now_ms - 1_000);
    }
}
