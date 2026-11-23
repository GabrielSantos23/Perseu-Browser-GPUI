//! Updates from GitHub Releases. The release workflow builds the installer,
//! attaches it to a release and bakes the repository's name into the app
//! (`PERSEU_UPDATE_REPO`); builds without it never check.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;

use anyhow::{Context as _, Result};

/// `owner/name` of the GitHub repository releases come from.
const REPO: Option<&str> = option_env!("PERSEU_UPDATE_REPO");
const CURRENT: &str = env!("CARGO_PKG_VERSION");
const INSTALLER_PREFIX: &str = "perseu-setup-";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CHECK_TIMEOUT_SECS: &str = "20";
const DOWNLOAD_TIMEOUT_SECS: &str = "600";

/// A newer release with an installer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    installer: String,
}

/// Whether this build knows where its updates come from.
pub fn enabled() -> bool {
    REPO.is_some()
}

/// The latest release, when it is newer than this build.
pub fn check() -> Result<Option<Release>> {
    let Some(repo) = REPO else {
        return Ok(None);
    };
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let reply = curl(&[
        "--max-time",
        CHECK_TIMEOUT_SECS,
        "-H",
        "Accept: application/vnd.github+json",
        "-H",
        "User-Agent: perseu-updater",
        &url,
    ])?;
    Ok(newer_release(&reply, repo, CURRENT))
}

/// Downloads the installer and starts it quietly. It closes this app,
/// installs over it and opens it again; the caller should quit.
pub fn install(release: &Release) -> Result<()> {
    let target: PathBuf =
        std::env::temp_dir().join(format!("{INSTALLER_PREFIX}{}.exe", release.version));
    let path = target.to_string_lossy().into_owned();
    curl(&[
        "--max-time",
        DOWNLOAD_TIMEOUT_SECS,
        "--output",
        &path,
        &release.installer,
    ])?;
    std::process::Command::new(&target)
        .args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART"])
        .spawn()
        .context("could not start the installer")?;
    Ok(())
}

fn curl(args: &[&str]) -> Result<Vec<u8>> {
    let output = std::process::Command::new("curl.exe")
        .args(["--fail", "--silent", "--show-error", "--location"])
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .context("could not start curl")?;
    anyhow::ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(output.stdout)
}

/// The release in `reply` if it is newer than `current` and carries an
/// installer from `repo`'s own release downloads.
fn newer_release(reply: &[u8], repo: &str, current: &str) -> Option<Release> {
    let release: serde_json::Value = serde_json::from_slice(reply).ok()?;
    if release["draft"].as_bool() == Some(true) || release["prerelease"].as_bool() == Some(true) {
        return None;
    }
    let version = release["tag_name"].as_str()?.trim_start_matches('v');
    if parse_version(version)? <= parse_version(current)? {
        return None;
    }
    let allowed = format!("https://github.com/{repo}/releases/download/");
    let installer = release["assets"].as_array()?.iter().find_map(|asset| {
        let name = asset["name"].as_str()?;
        let url = asset["browser_download_url"].as_str()?;
        (name.starts_with(INSTALLER_PREFIX) && name.ends_with(".exe") && url.starts_with(&allowed))
            .then(|| url.to_string())
    })?;
    Some(Release {
        version: version.to_string(),
        installer,
    })
}

fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.split(['.', '-', '+']).map(str::parse::<u64>);
    Some((
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next().unwrap_or(Ok(0)).ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(tag: &str, url: &str) -> Vec<u8> {
        format!(
            r#"{{"tag_name":"{tag}","draft":false,"prerelease":false,"assets":[
                {{"name":"notes.txt","browser_download_url":"https://github.com/me/app/releases/download/{tag}/notes.txt"}},
                {{"name":"perseu-setup-0.2.0.exe","browser_download_url":"{url}"}}]}}"#
        )
        .into_bytes()
    }

    #[test]
    fn only_a_newer_release_from_the_repo_is_offered() {
        let good = "https://github.com/me/app/releases/download/v0.2.0/perseu-setup-0.2.0.exe";
        assert_eq!(
            newer_release(&reply("v0.2.0", good), "me/app", "0.1.0"),
            Some(Release {
                version: "0.2.0".into(),
                installer: good.into()
            })
        );
        assert_eq!(
            newer_release(&reply("v0.1.0", good), "me/app", "0.1.0"),
            None
        );
        assert_eq!(
            newer_release(&reply("v0.0.9", good), "me/app", "0.1.0"),
            None
        );
        let elsewhere = "https://evil.example/perseu-setup-0.2.0.exe";
        assert_eq!(
            newer_release(&reply("v0.2.0", elsewhere), "me/app", "0.1.0"),
            None
        );
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(parse_version("0.10.0") > parse_version("0.9.9"));
        assert_eq!(parse_version("1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version("x"), None);
    }
}
