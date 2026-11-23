//! Security policy decisions that do not depend on the UI or the engine.

use std::net::IpAddr;
use std::path::Path;

/// Windows one page may open with user gestures before further ones are blocked.
pub const MAX_WINDOWS_PER_PAGE: u32 = 5;

const DANGEROUS_EXTENSIONS: &[&str] = &[
    "exe",
    "msi",
    "msix",
    "msixbundle",
    "appx",
    "appxbundle",
    "appinstaller",
    "bat",
    "cmd",
    "com",
    "scr",
    "pif",
    "cpl",
    "dll",
    "sys",
    "ps1",
    "psm1",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "hta",
    "jar",
    "lnk",
    "reg",
    "inf",
    "msc",
    "url",
    "iso",
    "img",
    "vhd",
    "vhdx",
    "application",
    "gadget",
    "chm",
    "settingcontent-ms",
    "library-ms",
    "search-ms",
    "xll",
    "docm",
    "xlsm",
    "pptm",
];

const BLOCKED_EXTERNAL_SCHEMES: &[&str] = &[
    "javascript",
    "vbscript",
    "data",
    "blob",
    "file",
    "about",
    "http",
    "https",
    "ftp",
    "ws",
    "wss",
    "shell",
    "hcp",
    "ms-msdt",
    "search",
    "search-ms",
    "ms-search",
    "ms-officecmd",
    "ms-cxh",
    "ms-cxh-full",
    "ms-appinstaller",
    "mk",
    "its",
    "ms-its",
    "res",
    "view-source",
    "edge",
    "perseu",
];

/// The origin (`scheme://host[:port]`) of an http(s) URL, lowercased and
/// without credentials. Other schemes have no origin worth showing.
pub fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let authority = rest.split(['/', '?', '#', '\\']).next()?;
    let host_port = authority.rsplit('@').next()?;
    if host_port.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{}", host_port.to_ascii_lowercase()))
}

/// The part of a host that its owner registered (`example.co.uk` for
/// `a.b.example.co.uk`), using the public suffix list. `None` for IP
/// addresses, single-label hosts and bare public suffixes.
pub fn registrable_domain(host: &str) -> Option<&str> {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if bare.parse::<IpAddr>().is_ok() || !host.contains('.') {
        return None;
    }
    psl::domain_str(host)
}

/// Whether opening this downloaded file could run code.
pub fn is_dangerous_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            DANGEROUS_EXTENSIONS
                .iter()
                .any(|dangerous| extension.eq_ignore_ascii_case(dangerous))
        })
}

/// The scheme of a URI handed to another app, when it may be offered to the
/// user at all. Schemes the browser handles itself and schemes known to be
/// abused to run code are refused.
pub fn external_scheme(uri: &str) -> Option<String> {
    let (scheme, _) = uri.split_once(':')?;
    let valid = !scheme.is_empty()
        && scheme
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    let scheme = scheme.to_ascii_lowercase();
    (valid && !BLOCKED_EXTERNAL_SCHEMES.contains(&scheme.as_str())).then_some(scheme)
}

/// Whether a page may open another window: only with a user gesture, and
/// only up to [`MAX_WINDOWS_PER_PAGE`] times per page.
pub fn may_open_window(user_initiated: bool, opened_by_page: u32) -> bool {
    user_initiated && opened_by_page < MAX_WINDOWS_PER_PAGE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_drop_paths_credentials_and_case() {
        assert_eq!(
            origin_of("HTTPS://User:Pw@Example.COM:8443/a?b#c").as_deref(),
            Some("https://example.com:8443")
        );
        assert_eq!(origin_of("http://a.b\\evil").as_deref(), Some("http://a.b"));
        assert_eq!(origin_of("file:///C:/x"), None);
        assert_eq!(origin_of("about:blank"), None);
        assert_eq!(origin_of("https://"), None);
    }

    #[test]
    fn registrable_domains_follow_the_public_suffix_list() {
        assert_eq!(
            registrable_domain("a.b.example.co.uk"),
            Some("example.co.uk")
        );
        assert_eq!(registrable_domain("evil.github.io"), Some("evil.github.io"));
        assert_eq!(registrable_domain("www.example.com"), Some("example.com"));
        assert_eq!(registrable_domain("192.168.0.1"), None);
        assert_eq!(registrable_domain("[::1]"), None);
        assert_eq!(registrable_domain("localhost"), None);
    }

    #[test]
    fn executables_and_scripts_are_dangerous() {
        assert!(is_dangerous_file(Path::new(r"C:\D\setup.EXE")));
        assert!(is_dangerous_file(Path::new("run.ps1")));
        assert!(is_dangerous_file(Path::new("report.docm")));
        assert!(!is_dangerous_file(Path::new("photo.jpg")));
        assert!(!is_dangerous_file(Path::new("archive.zip")));
        assert!(!is_dangerous_file(Path::new("noextension")));
    }

    #[test]
    fn external_schemes_exclude_web_and_abused_handlers() {
        assert_eq!(external_scheme("mailto:a@b.c").as_deref(), Some("mailto"));
        assert_eq!(external_scheme("Zoommtg://x").as_deref(), Some("zoommtg"));
        assert_eq!(external_scheme("ms-msdt:/id x"), None);
        assert_eq!(external_scheme("search-ms:query=x"), None);
        assert_eq!(external_scheme("javascript:alert(1)"), None);
        assert_eq!(external_scheme("https://a"), None);
        assert_eq!(external_scheme("1bad:x"), None);
        assert_eq!(external_scheme("no-colon"), None);
    }

    #[test]
    fn windows_need_a_gesture_and_stay_under_the_limit() {
        assert!(may_open_window(true, 0));
        assert!(!may_open_window(false, 0));
        assert!(!may_open_window(true, MAX_WINDOWS_PER_PAGE));
    }
}
