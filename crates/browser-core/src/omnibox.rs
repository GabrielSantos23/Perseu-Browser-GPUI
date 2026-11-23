//! Turns what the user typed in the URL field into a URL to load.

/// The URL of an empty page.
pub const BLANK_PAGE: &str = "about:blank";

const SEARCH_PLACEHOLDER: &str = "{query}";
const PASSTHROUGH_SCHEMES: &[&str] = &[
    "http",
    "https",
    "file",
    "about",
    "data",
    "edge",
    "view-source",
];

/// A web search provider, defined by a URL template containing `{query}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchEngine {
    /// Display name.
    pub name: &'static str,
    /// Search URL with `{query}` where the encoded query goes.
    pub template: &'static str,
}

impl SearchEngine {
    /// Google.
    pub const GOOGLE: Self = Self {
        name: "Google",
        template: "https://www.google.com/search?q={query}",
    };
    /// DuckDuckGo.
    pub const DUCKDUCKGO: Self = Self {
        name: "DuckDuckGo",
        template: "https://duckduckgo.com/?q={query}",
    };
    /// Bing.
    pub const BING: Self = Self {
        name: "Bing",
        template: "https://www.bing.com/search?q={query}",
    };
    /// Brave Search.
    pub const BRAVE: Self = Self {
        name: "Brave",
        template: "https://search.brave.com/search?q={query}",
    };
    /// Every built-in engine.
    pub const ALL: [Self; 4] = [Self::GOOGLE, Self::DUCKDUCKGO, Self::BING, Self::BRAVE];

    /// Builds the search URL for `query`.
    pub fn search_url(&self, query: &str) -> String {
        self.template
            .replace(SEARCH_PLACEHOLDER, &encode_query(query))
    }
}

impl Default for SearchEngine {
    fn default() -> Self {
        Self::GOOGLE
    }
}

/// Resolves URL field input into a URL: known schemes pass through, things
/// that look like hosts get a scheme, and everything else becomes a search.
/// Returns `None` for blank input.
pub fn resolve(input: &str, engine: &SearchEngine) -> Option<String> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if has_passthrough_scheme(input) {
        return Some(input.to_string());
    }
    if let Some(bang) = crate::bangs::expand(input) {
        return Some(bang.url);
    }
    if input.chars().any(char::is_whitespace) {
        return Some(engine.search_url(input));
    }
    let host = host_part(input);
    if is_local_host(host) {
        return Some(format!("http://{input}"));
    }
    if looks_like_domain(host) {
        return Some(format!("https://{input}"));
    }
    Some(engine.search_url(input))
}

/// Short form of a URL for the sidebar pill: the host without `www.`,
/// the whole URL when it has no host (for example `file:` URLs), and an
/// empty string for a blank page.
pub fn display_host(url: &str) -> &str {
    if url.is_empty() || url.eq_ignore_ascii_case(BLANK_PAGE) {
        return "";
    }
    let Some((_, rest)) = url.split_once("://") else {
        return url;
    };
    let host = host_part(rest);
    let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
    let host = strip_port(host);
    if host.is_empty() {
        return url;
    }
    host.strip_prefix("www.").unwrap_or(host)
}

fn has_passthrough_scheme(input: &str) -> bool {
    input.split_once(':').is_some_and(|(scheme, _)| {
        PASSTHROUGH_SCHEMES
            .iter()
            .any(|known| known.eq_ignore_ascii_case(scheme))
    })
}

fn host_part(input: &str) -> &str {
    let end = input.find(['/', '?', '#']).unwrap_or(input.len());
    &input[..end]
}

fn strip_port(host: &str) -> &str {
    if host.starts_with('[') {
        return host
            .split_once(']')
            .map_or(host, |(ipv6, _)| &host[..=ipv6.len()]);
    }
    host.rsplit_once(':')
        .filter(|(_, port)| port.chars().all(|c| c.is_ascii_digit()))
        .map_or(host, |(name, _)| name)
}

fn is_local_host(host: &str) -> bool {
    let name = strip_port(host);
    name.eq_ignore_ascii_case("localhost") || name.starts_with('[') || is_ipv4(name)
}

fn is_ipv4(name: &str) -> bool {
    let parts: Vec<&str> = name.split('.').collect();
    parts.len() == 4 && parts.iter().all(|part| part.parse::<u8>().is_ok())
}

fn looks_like_domain(host: &str) -> bool {
    let name = strip_port(host);
    let Some((labels, tld)) = name.rsplit_once('.') else {
        return false;
    };
    let valid_label =
        |label: &str| !label.is_empty() && label.chars().all(|c| c.is_alphanumeric() || c == '-');
    tld.chars().count() >= 2
        && tld.chars().all(char::is_alphabetic)
        && labels.split('.').all(valid_label)
}

fn encode_query(query: &str) -> String {
    let mut encoded = String::with_capacity(query.len());
    for byte in query.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn go(input: &str) -> Option<String> {
        resolve(input, &SearchEngine::GOOGLE)
    }

    #[test]
    fn blank_input_resolves_to_nothing() {
        assert_eq!(go("   "), None);
    }

    #[test]
    fn full_urls_pass_through() {
        assert_eq!(
            go("https://example.com/a?b=1").as_deref(),
            Some("https://example.com/a?b=1")
        );
        assert_eq!(go("about:blank").as_deref(), Some("about:blank"));
        assert_eq!(go("HTTP://x.org").as_deref(), Some("HTTP://x.org"));
    }

    #[test]
    fn bare_domains_get_https() {
        assert_eq!(go("example.com").as_deref(), Some("https://example.com"));
        assert_eq!(go("docs.rs/gpui").as_deref(), Some("https://docs.rs/gpui"));
        assert_eq!(
            go("example.com:8443").as_deref(),
            Some("https://example.com:8443")
        );
    }

    #[test]
    fn local_hosts_get_http() {
        assert_eq!(
            go("localhost:3000").as_deref(),
            Some("http://localhost:3000")
        );
        assert_eq!(
            go("192.168.0.1/admin").as_deref(),
            Some("http://192.168.0.1/admin")
        );
    }

    #[test]
    fn text_becomes_a_search() {
        assert_eq!(
            go("rust gpui").as_deref(),
            Some("https://www.google.com/search?q=rust+gpui")
        );
        assert_eq!(
            go("hello").as_deref(),
            Some("https://www.google.com/search?q=hello")
        );
        assert_eq!(
            go("1.5").as_deref(),
            Some("https://www.google.com/search?q=1.5")
        );
    }

    #[test]
    fn search_queries_are_encoded() {
        assert_eq!(
            SearchEngine::DUCKDUCKGO.search_url("a&b=c #d"),
            "https://duckduckgo.com/?q=a%26b%3Dc+%23d"
        );
        assert_eq!(
            SearchEngine::GOOGLE.search_url("café"),
            "https://www.google.com/search?q=caf%C3%A9"
        );
    }

    #[test]
    fn internal_scheme_is_not_navigable() {
        let resolved = resolve("perseu://settings", &SearchEngine::GOOGLE).unwrap();
        assert!(resolved.starts_with("https://www.google.com/search?q="));
    }

    #[test]
    fn javascript_urls_are_searched_not_run() {
        assert_eq!(
            go("javascript:alert(1)").as_deref(),
            Some("https://www.google.com/search?q=javascript%3Aalert%281%29")
        );
    }

    #[test]
    fn display_host_strips_scheme_www_port_and_path() {
        assert_eq!(
            display_host("https://www.example.com/path?q=1"),
            "example.com"
        );
        assert_eq!(display_host("http://localhost:3000/"), "localhost");
        assert_eq!(display_host("https://user@site.org"), "site.org");
        assert_eq!(display_host("about:blank"), "");
        assert_eq!(display_host(""), "");
        assert_eq!(display_host("file:///C:/x.html"), "file:///C:/x.html");
    }
}
