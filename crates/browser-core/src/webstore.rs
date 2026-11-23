//! Installing extensions from the Chrome Web Store: recognizing a store
//! page, the address its package downloads from, and unpacking that package
//! (a CRX: a signed header followed by a ZIP) into a folder.

use std::fs;
use std::io::Read;
use std::path::{Component, Path};

use anyhow::{Context, Result, bail, ensure};

const STORE_HOSTS: [&str; 2] = [
    "https://chromewebstore.google.com/detail/",
    "https://chrome.google.com/webstore/detail/",
];
const ID_LEN: usize = 32;
const CRX_MAGIC: &[u8; 4] = b"Cr24";
const EOCD_SIGNATURE: u32 = 0x0605_4b50;
const CENTRAL_SIGNATURE: u32 = 0x0201_4b50;
const LOCAL_SIGNATURE: u32 = 0x0403_4b50;
const EOCD_LEN: usize = 22;
const STORED: u16 = 0;
const DEFLATED: u16 = 8;
const MAX_UNPACKED: u64 = 512 * 1024 * 1024;

const STORE_PAGE_SCRIPT: &str = include_str!("store_page.js");

/// Script run (through DevTools, never registered on the page) on a store
/// listing for extension `id`: it puts a button labelled `label` where the
/// store's "Add to Chrome" is and evaluates to the id of the listing whose
/// button got a trusted click since the last run, `""` otherwise. `kind` is
/// `add`, `adding` (disabled) or `remove`.
pub fn store_button_script(id: &str, kind: &str, label: &str) -> String {
    let state = serde_json::json!({ "id": id, "kind": kind, "label": label });
    format!(
        "(()=>{{{STORE_PAGE_SCRIPT}
const store=window.__perseu_store_v1__;if(!store)return '';store.show({state});const id=store.poll();return id==='?'?'':id}})()"
    )
}

/// The extension id of a Chrome Web Store detail page, if `url` is one.
pub fn extension_id(url: &str) -> Option<String> {
    let rest = STORE_HOSTS.iter().find_map(|prefix| {
        url.get(..prefix.len())
            .filter(|head| head.eq_ignore_ascii_case(prefix))
            .map(|_| &url[prefix.len()..])
    })?;
    let path = rest.split(['?', '#']).next()?;
    path.split('/')
        .find(|part| part.len() == ID_LEN && part.bytes().all(|b| (b'a'..=b'p').contains(&b)))
        .map(str::to_string)
}

/// The extension a store package download is for, when `url` is one: the
/// store's own "Add to Chrome" button downloads from the update server, or
/// from where it redirects.
pub fn package_download_id(url: &str) -> Option<String> {
    const PACKAGE_HOSTS: [&str; 2] = [
        "https://clients2.google.com/service/update2/crx",
        "https://clients2.googleusercontent.com/crx/",
    ];
    let lower = url.to_ascii_lowercase();
    if !PACKAGE_HOSTS.iter().any(|host| lower.starts_with(host)) {
        return None;
    }
    lower
        .match_indices("id%3d")
        .map(|(at, marker)| &lower[at + marker.len()..])
        .chain(lower.split('/'))
        .map(|rest| &rest[..rest.len().min(ID_LEN)])
        .find(|candidate| {
            candidate.len() == ID_LEN && candidate.bytes().all(|b| (b'a'..=b'p').contains(&b))
        })
        .map(str::to_string)
}

/// Where the store serves extension `id` as a CRX to a browser of
/// `version` (the WebView2 runtime's Chromium version). An old or missing
/// version is answered with a redirect to `permanently-removed.invalid`.
pub fn crx_url(id: &str, version: &str) -> String {
    format!(
        "https://clients2.google.com/service/update2/crx?response=redirect&prodversion={version}&acceptformat=crx2,crx3&x=id%3D{id}%26installsource%3Dondemand%26uc"
    )
}

/// Unpacks a CRX package into `to`, which must not exist yet. The store's
/// signature folder (`_metadata`) is left out: unpacked extensions may not
/// contain names starting with `_`.
pub fn unpack_crx(crx: &[u8], to: &Path) -> Result<()> {
    let zip = zip_part(crx)?;
    fs::create_dir_all(to).with_context(|| format!("could not create {}", to.display()))?;
    let mut total = 0u64;
    for entry in central_directory(zip)? {
        if entry.name.ends_with('/') || entry.name.starts_with("_metadata/") {
            continue;
        }
        let relative = safe_path(&entry.name)?;
        let data = entry_data(zip, &entry)?;
        total += data.len() as u64;
        ensure!(total <= MAX_UNPACKED, "the package is too large");
        let path = to.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, data).with_context(|| format!("could not write {}", path.display()))?;
    }
    ensure!(
        to.join("manifest.json").is_file(),
        "the package has no manifest.json"
    );
    Ok(())
}

fn zip_part(crx: &[u8]) -> Result<&[u8]> {
    ensure!(crx.get(..4) == Some(CRX_MAGIC), "not an extension package");
    let version = u32_at(crx, 4)?;
    let start = match version {
        3 => 12 + u32_at(crx, 8)? as usize,
        2 => 16 + u32_at(crx, 8)? as usize + u32_at(crx, 12)? as usize,
        other => bail!("unsupported package version {other}"),
    };
    crx.get(start..).context("the package is cut short")
}

struct Entry {
    name: String,
    method: u16,
    compressed: usize,
    size: usize,
    local: usize,
}

fn central_directory(zip: &[u8]) -> Result<Vec<Entry>> {
    let eocd = (0..=zip.len().saturating_sub(EOCD_LEN))
        .rev()
        .find(|&at| u32_at(zip, at).is_ok_and(|sig| sig == EOCD_SIGNATURE))
        .context("the package's archive has no directory")?;
    let count = u16_at(zip, eocd + 10)? as usize;
    let mut at = u32_at(zip, eocd + 16)? as usize;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        ensure!(
            u32_at(zip, at)? == CENTRAL_SIGNATURE,
            "the archive is damaged"
        );
        let name_len = u16_at(zip, at + 28)? as usize;
        let extra_len = u16_at(zip, at + 30)? as usize;
        let comment_len = u16_at(zip, at + 32)? as usize;
        let name = zip
            .get(at + 46..at + 46 + name_len)
            .context("the archive is damaged")?;
        entries.push(Entry {
            name: String::from_utf8_lossy(name).into_owned(),
            method: u16_at(zip, at + 10)?,
            compressed: u32_at(zip, at + 20)? as usize,
            size: u32_at(zip, at + 24)? as usize,
            local: u32_at(zip, at + 42)? as usize,
        });
        at += 46 + name_len + extra_len + comment_len;
    }
    Ok(entries)
}

fn entry_data(zip: &[u8], entry: &Entry) -> Result<Vec<u8>> {
    let at = entry.local;
    ensure!(
        u32_at(zip, at)? == LOCAL_SIGNATURE,
        "the archive is damaged"
    );
    let start = at + 30 + u16_at(zip, at + 26)? as usize + u16_at(zip, at + 28)? as usize;
    let raw = zip
        .get(start..start + entry.compressed)
        .context("the archive is cut short")?;
    match entry.method {
        STORED => Ok(raw.to_vec()),
        DEFLATED => {
            let mut data = Vec::with_capacity(entry.size.min(MAX_UNPACKED as usize));
            flate2::read::DeflateDecoder::new(raw)
                .take(MAX_UNPACKED + 1)
                .read_to_end(&mut data)
                .context("could not decompress the package")?;
            Ok(data)
        }
        other => bail!("unsupported compression {other} in the package"),
    }
}

/// Refuses archive names that would land outside the target folder.
fn safe_path(name: &str) -> Result<&Path> {
    let path = Path::new(name);
    let safe = path
        .components()
        .all(|part| matches!(part, Component::Normal(_)));
    ensure!(
        safe && !name.contains(':'),
        "unsafe file name {name:?} in the package"
    );
    Ok(path)
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16> {
    let raw = bytes.get(at..at + 2).context("the package is cut short")?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    let raw = bytes.get(at..at + 4).context("the package is cut short")?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "ddkjiahejlhfcafbddmgiahcphecmpfh";

    #[test]
    fn the_store_button_is_offered_for_its_listing() {
        let script = store_button_script(ID, "add", "Add to perseu");
        assert!(script.contains(&format!("\"id\":\"{ID}\"")));
        assert!(script.starts_with("(()=>{") && script.ends_with("})()"));
    }

    /// A ZIP of stored (uncompressed) files.
    fn stored_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = Vec::new();
        let mut central = Vec::new();
        for (name, data) in files {
            let offset = zip.len() as u32;
            let len = data.len() as u32;
            zip.extend(LOCAL_SIGNATURE.to_le_bytes());
            zip.extend([0u8; 14]);
            zip.extend(len.to_le_bytes());
            zip.extend(len.to_le_bytes());
            zip.extend((name.len() as u16).to_le_bytes());
            zip.extend(0u16.to_le_bytes());
            zip.extend(name.as_bytes());
            zip.extend(*data);
            central.extend(CENTRAL_SIGNATURE.to_le_bytes());
            central.extend([0u8; 16]);
            central.extend(len.to_le_bytes());
            central.extend(len.to_le_bytes());
            central.extend((name.len() as u16).to_le_bytes());
            central.extend([0u8; 12]);
            central.extend(offset.to_le_bytes());
            central.extend(name.as_bytes());
        }
        let central_at = zip.len() as u32;
        let central_len = central.len() as u32;
        zip.extend(central);
        zip.extend(EOCD_SIGNATURE.to_le_bytes());
        zip.extend([0u8; 4]);
        zip.extend((files.len() as u16).to_le_bytes());
        zip.extend((files.len() as u16).to_le_bytes());
        zip.extend(central_len.to_le_bytes());
        zip.extend(central_at.to_le_bytes());
        zip.extend(0u16.to_le_bytes());
        zip
    }

    fn crx3(zip: &[u8]) -> Vec<u8> {
        let header = [7u8; 5];
        let mut crx = CRX_MAGIC.to_vec();
        crx.extend(3u32.to_le_bytes());
        crx.extend((header.len() as u32).to_le_bytes());
        crx.extend(header);
        crx.extend(zip);
        crx
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("perseu-webstore-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn store_pages_give_their_extension_id() {
        let page =
            format!("https://chromewebstore.google.com/detail/ublock-origin-lite/{ID}?hl=pt-BR");
        assert_eq!(extension_id(&page).as_deref(), Some(ID));
        let old = format!("https://chrome.google.com/webstore/detail/x/{ID}");
        assert_eq!(extension_id(&old).as_deref(), Some(ID));
        assert_eq!(
            extension_id("https://chromewebstore.google.com/category/extensions"),
            None
        );
        assert_eq!(
            extension_id(&format!("https://example.com/detail/{ID}")),
            None
        );
    }

    #[test]
    fn store_package_downloads_give_their_extension_id() {
        let button = format!(
            "https://clients2.google.com/service/update2/crx?response=redirect&prodversion=120&x=id%3D{ID}%26uc"
        );
        assert_eq!(package_download_id(&button).as_deref(), Some(ID));
        assert_eq!(
            package_download_id(&crx_url(ID, "157.0")).as_deref(),
            Some(ID)
        );
        assert_eq!(package_download_id("https://example.com/file.crx"), None);
    }

    #[test]
    fn crx_url_names_the_extension_and_version() {
        let url = crx_url(ID, "131.0.2903.70");
        assert!(url.contains("prodversion=131.0.2903.70"));
        assert!(url.contains(&format!("id%3D{ID}")));
    }

    #[test]
    fn unpacks_a_crx3_without_store_metadata() {
        let zip = stored_zip(&[
            ("manifest.json", b"{\"name\":\"x\"}"),
            ("js/app.js", b"1"),
            ("_metadata/verified_contents.json", b"[]"),
        ]);
        let to = temp("unpack");
        unpack_crx(&crx3(&zip), &to).unwrap();
        assert_eq!(fs::read(to.join("js/app.js")).unwrap(), b"1");
        assert!(!to.join("_metadata").exists());
        fs::remove_dir_all(to).unwrap();
    }

    #[test]
    fn refuses_names_outside_the_folder_and_non_packages() {
        let zip = stored_zip(&[("../evil.txt", b"x"), ("manifest.json", b"{}")]);
        let to = temp("slip");
        assert!(unpack_crx(&crx3(&zip), &to).is_err());
        let _ = fs::remove_dir_all(to);
        assert!(unpack_crx(b"PK\x03\x04", &temp("magic")).is_err());
    }
}
