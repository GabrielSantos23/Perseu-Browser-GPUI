//! Chromium extensions, installed as unpacked folders.
//!
//! The WebView2 runtime hosts the extensions themselves; this module only
//! manages what the user installs: reading manifests, copying folders into
//! the extensions directory, and listing what is there.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

const MANIFEST_FILE: &str = "manifest.json";
const IDS_FILE: &str = "ids.json";
const POPUP_TAB_FILE: &str = "sidebrowser-popup-tab.js";
const POPUP_TAB_SCRIPT: &str = include_str!("popup_tab.js");
const WINDOWS_CREATE_SCRIPT: &str = include_str!("windows_create.js");
const TABS_CREATE_SCRIPT: &str = include_str!("tabs_create.js");
const BACKGROUND_SHIM_START: &str = "/* sidebrowser: windows.create gives back its window */
";
const BACKGROUND_SHIM_END: &str = "/* end sidebrowser */
";

/// The manifest fields this browser shows. The runtime reads the rest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// 2 or 3, as the manifest declares.
    pub manifest_version: u32,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    /// MV3 `action` or MV2 `browser_action`.
    #[serde(alias = "browser_action")]
    pub action: Option<Action>,
    /// Locale whose `_locales/<locale>/messages.json` fills `__MSG_*__`.
    #[serde(default)]
    pub default_locale: Option<String>,
}

/// The toolbar button of an extension, when it has one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    /// Page opened when the user asks for the extension.
    #[serde(default)]
    pub default_popup: Option<String>,
}

impl Manifest {
    /// Which manifest format it is, for the user interface.
    pub fn generation(&self) -> &'static str {
        match self.manifest_version {
            2 => "MV2",
            _ => "MV3",
        }
    }
}

/// One installed extension folder, with the manifest it declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// Folder name inside the extensions directory; the settings key.
    pub folder: String,
    pub manifest: Manifest,
    /// Whether the user has it enabled.
    pub enabled: bool,
}

impl Installed {
    /// `chrome-extension://` address of the toolbar popup, when the extension
    /// has one and the runtime gave it an id.
    pub fn popup(&self, id: &str) -> Option<String> {
        let popup = &self.manifest.action.as_ref()?.default_popup.as_ref()?;
        Some(format!(
            "chrome-extension://{id}/{}",
            popup.trim_start_matches('/')
        ))
    }
}

/// Reads the manifest of one unpacked extension folder.
pub fn read_manifest(folder: &Path) -> Result<Manifest> {
    let file = folder.join(MANIFEST_FILE);
    let text = fs::read_to_string(&file)
        .with_context(|| format!("{} is not an extension folder", folder.display()))?;
    let manifest: Manifest = serde_json::from_str(&text).with_context(|| {
        format!(
            "{} has a manifest this browser cannot read",
            folder.display()
        )
    })?;
    Ok(localized(folder, manifest))
}

/// Replaces `__MSG_key__` names and descriptions with the text from the
/// extension's default locale, the way Chrome shows them.
fn localized(folder: &Path, mut manifest: Manifest) -> Manifest {
    let locale = manifest.default_locale.as_deref().unwrap_or("en");
    let messages: Option<BTreeMap<String, serde_json::Value>> =
        fs::read_to_string(folder.join("_locales").join(locale).join("messages.json"))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok());
    let Some(messages) = messages else {
        return manifest;
    };
    let lookup = |text: &str| -> Option<String> {
        let key = text.strip_prefix("__MSG_")?.strip_suffix("__")?;
        messages
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))?
            .1
            .get("message")?
            .as_str()
            .map(str::to_string)
    };
    if let Some(name) = lookup(&manifest.name) {
        manifest.name = name;
    }
    if let Some(description) = lookup(&manifest.description) {
        manifest.description = description;
    }
    manifest
}

/// Copies an unpacked extension folder into the extensions directory and
/// returns the folder name it was installed as. An existing installation of
/// the same name is replaced.
pub fn install(from: &Path, extensions_dir: &Path) -> Result<String> {
    let manifest = read_manifest(from)?;
    let name = folder_name_for(from, &manifest);
    let to = extensions_dir.join(&name);
    if to.exists() {
        fs::remove_dir_all(&to).with_context(|| format!("could not replace {}", to.display()))?;
    }
    copy_dir(from, &to).with_context(|| format!("could not install into {}", to.display()))?;
    adapt(&to)?;
    Ok(name)
}

/// Fills two gaps in WebView2's extension support with scripts that run
/// inside the installed (unpacked) extension itself, with its own
/// permissions, and talk to nothing but the extension API:
/// the popup's "active tab" ([`prepare_popup`]) and the window
/// `windows.create` should give back ([`prepare_background`]). Running it
/// again refreshes both scripts. Returns whether the service worker
/// changed: the runtime keeps its own copy of a loaded extension's worker,
/// so a loaded extension must then be reloaded.
pub fn adapt(folder: &Path) -> Result<bool> {
    prepare_popup(folder)?;
    prepare_background(folder)
}

/// `relative`, from a manifest, as a path inside `folder`; anything that
/// would leave the folder is refused.
fn inside(folder: &Path, relative: &str) -> Result<PathBuf> {
    let relative = Path::new(relative.trim_start_matches('/'));
    if !relative
        .components()
        .all(|part| matches!(part, std::path::Component::Normal(_)))
    {
        anyhow::bail!("{} names a file outside its folder", folder.display());
    }
    Ok(folder.join(relative))
}

/// WebView2's `windows.create` opens the window but resolves with nothing,
/// and extensions use what it returns (Bitwarden reads the id of its
/// passkey window). A script put first in the MV3 service worker finds the
/// tab the call opened and answers with its window.
fn prepare_background(folder: &Path) -> Result<bool> {
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(folder.join(MANIFEST_FILE))?)?;
    let Some(worker) = manifest
        .pointer("/background/service_worker")
        .and_then(serde_json::Value::as_str)
    else {
        return Ok(false);
    };
    let path = inside(folder, worker)?;
    let source = fs::read_to_string(&path)
        .with_context(|| format!("could not read the service worker {}", path.display()))?;
    let body = match source.strip_prefix(BACKGROUND_SHIM_START) {
        Some(shimmed) => shimmed
            .find(BACKGROUND_SHIM_END)
            .map_or(source.as_str(), |end| {
                &shimmed[end + BACKGROUND_SHIM_END.len()..]
            }),
        None => source.as_str(),
    };
    let updated = format!(
        "{BACKGROUND_SHIM_START}{WINDOWS_CREATE_SCRIPT}{TABS_CREATE_SCRIPT}{BACKGROUND_SHIM_END}{body}"
    );
    if updated == source {
        return Ok(false);
    }
    fs::write(&path, updated)
        .with_context(|| format!("could not update the service worker {}", path.display()))?;
    Ok(true)
}

/// Makes an installed extension's toolbar popup see the page it was opened
/// for. In Chrome the popup belongs to the page's window; here it shows in a
/// view of its own, so `tabs.query({active: true})` would find the popup
/// itself. A script file added to the extension's own folder, loaded first
/// by the popup page, answers that query with the most recently used tab
/// that is not an extension page. It runs only in that extension's pages
/// and talks to nothing but the extension API. Running it again refreshes
/// the script and leaves the page as it is.
fn prepare_popup(folder: &Path) -> Result<()> {
    let manifest = read_manifest(folder)?;
    let Some(popup) = manifest.action.and_then(|action| action.default_popup) else {
        return Ok(());
    };
    let page = inside(folder, &popup)?;
    let html = fs::read_to_string(&page)
        .with_context(|| format!("could not read the popup {}", page.display()))?;
    fs::write(folder.join(POPUP_TAB_FILE), POPUP_TAB_SCRIPT)?;
    let tag = format!("<script src=\"/{POPUP_TAB_FILE}\"></script>");
    if html.contains(&tag) {
        return Ok(());
    }
    let at = html
        .to_ascii_lowercase()
        .find("<head")
        .and_then(|head| html[head..].find('>').map(|end| head + end + 1))
        .unwrap_or(0);
    fs::write(&page, format!("{}{tag}{}", &html[..at], &html[at..]))
        .with_context(|| format!("could not update the popup {}", page.display()))
}

/// What the add-extension card shows before an unpacked package installs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    pub manifest: Manifest,
    /// The largest PNG icon the manifest names, inside the folder.
    pub icon: Option<PathBuf>,
    /// What the extension may do, as Chrome words its install warnings,
    /// broadest first.
    pub access: Vec<&'static str>,
}

/// Reads what an unpacked package asks for, for the user to confirm.
pub fn review(folder: &Path) -> Result<Review> {
    let manifest = read_manifest(folder)?;
    let raw: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(folder.join(MANIFEST_FILE))?)?;
    let icon = raw
        .get("icons")
        .and_then(serde_json::Value::as_object)
        .and_then(|icons| {
            icons
                .iter()
                .filter_map(|(size, path)| Some((size.parse::<u32>().ok()?, path.as_str()?)))
                .filter(|(_, path)| path.to_ascii_lowercase().ends_with(".png"))
                .max_by_key(|(size, _)| *size)
                .and_then(|(_, path)| inside(folder, path.trim_start_matches('/')).ok())
        })
        .filter(|path| path.is_file());
    Ok(Review {
        manifest,
        icon,
        access: access(&raw),
    })
}

fn access(raw: &serde_json::Value) -> Vec<&'static str> {
    const WARNINGS: [(&str, &str); 13] = [
        ("tabs", "Read your browsing history"),
        ("history", "Read and change your browsing history"),
        ("bookmarks", "Read and change your bookmarks"),
        ("downloads", "Manage your downloads"),
        ("clipboardRead", "Read data you copy and paste"),
        ("clipboardWrite", "Modify data you copy and paste"),
        ("notifications", "Display notifications"),
        ("geolocation", "Detect your physical location"),
        (
            "nativeMessaging",
            "Communicate with cooperating native applications",
        ),
        ("management", "Manage your apps, extensions, and themes"),
        ("privacy", "Change your privacy-related settings"),
        (
            "topSites",
            "Read a list of your most frequently visited websites",
        ),
        ("declarativeNetRequest", "Block content on any page"),
    ];
    let strings = |key: &str| -> Vec<String> {
        raw.get(key)
            .and_then(serde_json::Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|value| value.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let permissions = strings("permissions");
    let mut hosts: Vec<String> = strings("host_permissions");
    hosts.extend(
        permissions
            .iter()
            .filter(|p| p.contains("://") || *p == "<all_urls>")
            .cloned(),
    );
    if let Some(scripts) = raw
        .get("content_scripts")
        .and_then(serde_json::Value::as_array)
    {
        for script in scripts {
            if let Some(matches) = script.get("matches").and_then(serde_json::Value::as_array) {
                hosts.extend(
                    matches
                        .iter()
                        .filter_map(|m| m.as_str().map(str::to_string)),
                );
            }
        }
    }
    let everywhere = hosts.iter().any(|host| {
        host == "<all_urls>"
            || host
                .split_once("://")
                .is_some_and(|(_, rest)| rest.starts_with("*/"))
    });
    let mut access = Vec::new();
    if everywhere {
        access.push("Read and change all your data on all websites");
    } else if !hosts.is_empty() {
        access.push("Read and change your data on some websites");
    }
    access.extend(
        WARNINGS
            .iter()
            .filter(|(permission, _)| permissions.iter().any(|p| p == permission))
            .map(|(_, warning)| *warning),
    );
    access
}

/// Lists the extension folders in the extensions directory, newest first.
pub fn scan(extensions_dir: &Path, enabled: &BTreeMap<String, bool>) -> Vec<Installed> {
    let Ok(entries) = fs::read_dir(extensions_dir) else {
        return Vec::new();
    };
    let mut installed: Vec<Installed> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let folder = entry.file_name().to_string_lossy().into_owned();
            let manifest = read_manifest(&entry.path()).ok()?;
            Some(Installed {
                enabled: *enabled.get(&folder).unwrap_or(&false),
                folder,
                manifest,
            })
        })
        .collect();
    installed.sort_by(|a, b| {
        a.manifest
            .name
            .to_lowercase()
            .cmp(&b.manifest.name.to_lowercase())
    });
    installed
}

/// Deletes one installed extension folder.
pub fn uninstall(extensions_dir: &Path, folder: &str) -> Result<()> {
    let path = extensions_dir.join(folder);
    fs::remove_dir_all(&path).with_context(|| format!("could not remove {}", path.display()))
}

/// The runtime ids installed folders loaded as, persisted beside the
/// extensions so a session can recognize what the runtime kept from an
/// earlier one — a disabled extension may report no name at all.
pub fn load_ids(extensions_dir: &Path) -> Result<BTreeMap<String, String>> {
    Ok(crate::store::load_json(&extensions_dir.join(IDS_FILE))?.unwrap_or_default())
}

/// Saves the folder → runtime id mapping.
pub fn save_ids(extensions_dir: &Path, ids: &BTreeMap<String, String>) -> Result<()> {
    crate::store::save_json(&extensions_dir.join(IDS_FILE), ids)
}

/// Keeps a folder name to letters, digits, dashes, dots and underscores, so
/// that whatever the picker handed over stays a plain directory name.
fn folder_name_for(from: &Path, manifest: &Manifest) -> String {
    let raw = from.file_name().map_or_else(
        || manifest.name.clone(),
        |name| name.to_string_lossy().into_owned(),
    );
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "extension".to_string()
    } else {
        trimmed
    }
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target: PathBuf = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory for one test, removed when the guard drops.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("perseu-extensions-{name}"));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn extension(&self, folder: &str, manifest: &str) -> PathBuf {
            let path = self.0.join("src").join(folder);
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join(MANIFEST_FILE), manifest).unwrap();
            fs::write(path.join("content.js"), "// script").unwrap();
            fs::write(path.join("popup.html"), "<head></head>").unwrap();
            path
        }

        fn dir(&self) -> PathBuf {
            self.0.join("installed")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const MV3: &str = r#"{"manifest_version":3,"name":"Test Blocker","version":"1.2","description":"blocks","action":{"default_popup":"popup.html"}}"#;
    const MV2: &str = r#"{"manifest_version":2,"name":"Old Style","version":"0.1","browser_action":{"default_popup":"/popup.html"}}"#;

    #[test]
    fn parses_mv3_and_mv2_manifests() {
        let scratch = Scratch::new("parse");
        let manifest = read_manifest(&scratch.extension("blocker", MV3)).unwrap();
        assert_eq!(manifest.manifest_version, 3);
        assert_eq!(manifest.generation(), "MV3");
        assert_eq!(
            manifest.action.unwrap().default_popup.as_deref(),
            Some("popup.html")
        );

        let manifest = read_manifest(&scratch.extension("old", MV2)).unwrap();
        assert_eq!(manifest.generation(), "MV2");
        assert_eq!(
            manifest.action.unwrap().default_popup.as_deref(),
            Some("/popup.html")
        );
    }

    #[test]
    fn installs_copies_and_scans() {
        let scratch = Scratch::new("install");
        let source = scratch.extension("My Blocker!", MV3);
        let folder = install(&source, &scratch.dir()).unwrap();
        assert_eq!(folder, "My-Blocker");

        let enabled = BTreeMap::from([("My-Blocker".to_string(), true)]);
        let installed = scan(&scratch.dir(), &enabled);
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].manifest.name, "Test Blocker");
        assert!(installed[0].enabled);
    }

    #[test]
    fn translated_names_come_from_the_default_locale() {
        let scratch = Scratch::new("locale");
        let folder = scratch.extension(
            "ubol",
            r#"{"manifest_version":3,"name":"__MSG_extName__","version":"1","default_locale":"en"}"#,
        );
        let messages = folder.join("_locales").join("en");
        fs::create_dir_all(&messages).unwrap();
        fs::write(
            messages.join("messages.json"),
            r#"{"extName":{"message":"uBlock Origin Lite"}}"#,
        )
        .unwrap();
        assert_eq!(read_manifest(&folder).unwrap().name, "uBlock Origin Lite");
    }

    #[test]
    fn popup_address_uses_the_runtime_id() {
        let scratch = Scratch::new("popup");
        let folder = install(&scratch.extension("popup", MV3), &scratch.dir()).unwrap();
        assert_eq!(folder, "popup");
        let installed = scan(&scratch.dir(), &BTreeMap::new());
        let address = installed[0].popup("abcdef").unwrap();
        assert_eq!(address, "chrome-extension://abcdef/popup.html");

        // MV2's leading slash does not turn into a double one.
        install(&scratch.extension("old", MV2), &scratch.dir()).unwrap();
        let installed = scan(&scratch.dir(), &BTreeMap::new());
        let old = installed.iter().find(|item| item.folder == "old").unwrap();
        assert_eq!(
            old.popup("abcdef").unwrap(),
            "chrome-extension://abcdef/popup.html"
        );
    }

    #[test]
    fn popup_pages_load_the_tab_script_first_once() {
        let scratch = Scratch::new("popup-script");
        let source = scratch.extension("blocker", MV3);
        fs::write(
            source.join("popup.html"),
            "<html><HEAD lang=x><script src=app.js></script></head></html>",
        )
        .unwrap();
        let folder = install(&source, &scratch.dir()).unwrap();
        let installed = scratch.dir().join(folder);
        prepare_popup(&installed).unwrap();
        let html = fs::read_to_string(installed.join("popup.html")).unwrap();
        assert_eq!(
            html,
            "<html><HEAD lang=x><script src=\"/sidebrowser-popup-tab.js\"></script><script src=app.js></script></head></html>"
        );
        assert!(installed.join(POPUP_TAB_FILE).exists());
    }

    #[test]
    fn service_workers_get_the_window_script_once() {
        let scratch = Scratch::new("worker-script");
        let folder = scratch.extension(
            "worker",
            r#"{"manifest_version":3,"name":"w","version":"1","background":{"service_worker":"bg.js"}}"#,
        );
        fs::write(
            folder.join("bg.js"),
            "import './x.js';
run();
",
        )
        .unwrap();
        assert!(adapt(&folder).unwrap());
        assert!(!adapt(&folder).unwrap());
        let worker = fs::read_to_string(folder.join("bg.js")).unwrap();
        assert_eq!(worker.matches(BACKGROUND_SHIM_START).count(), 1);
        assert!(worker.ends_with(&format!(
            "{BACKGROUND_SHIM_END}import './x.js';
run();
"
        )));
    }

    #[test]
    fn review_words_what_the_extension_may_do() {
        let raw = serde_json::json!({
            "permissions": ["storage", "tabs", "clipboardWrite"],
            "content_scripts": [{ "matches": ["https://*/*"] }],
        });
        assert_eq!(
            access(&raw),
            [
                "Read and change all your data on all websites",
                "Read your browsing history",
                "Modify data you copy and paste",
            ]
        );
        let raw = serde_json::json!({ "host_permissions": ["https://example.com/*"] });
        assert_eq!(access(&raw), ["Read and change your data on some websites"]);
        assert!(access(&serde_json::json!({ "permissions": ["storage"] })).is_empty());
    }

    #[test]
    fn popups_outside_the_folder_are_refused() {
        let scratch = Scratch::new("popup-escape");
        let folder = scratch.extension(
            "escape",
            r#"{"manifest_version":3,"name":"x","version":"1","action":{"default_popup":"../outside.html"}}"#,
        );
        assert!(prepare_popup(&folder).is_err());
    }

    #[test]
    fn uninstall_removes_the_folder() {
        let scratch = Scratch::new("uninstall");
        let source = scratch.extension("gone", MV3);
        let folder = install(&source, &scratch.dir()).unwrap();
        uninstall(&scratch.dir(), &folder).unwrap();
        assert!(scan(&scratch.dir(), &BTreeMap::new()).is_empty());
        assert!(uninstall(&scratch.dir(), "missing").is_err());
    }

    #[test]
    fn install_refuses_folders_without_a_manifest() {
        let scratch = Scratch::new("refuse");
        let bare = scratch.0.join("src").join("bare");
        fs::create_dir_all(&bare).unwrap();
        assert!(install(&bare, &scratch.dir()).is_err());
    }
}
