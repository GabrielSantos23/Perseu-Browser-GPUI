//! `!bang` shortcuts, in the style DuckDuckGo made popular, resolved here:
//! `!yt cats` goes straight to YouTube's search without any search engine
//! in between. A bang may come first or last; a bang alone opens the site.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Every bang: `trigger<TAB>name<TAB>domain<TAB>search address`, from
/// Kagi's MIT-licensed list (`data/bangs-LICENSE`), itself grown from
/// DuckDuckGo's. Bundled, so no service is asked at run time.
const TABLE: &str = include_str!("../data/bangs.tsv");
const QUERY: &str = "{{{s}}}";

/// The site a bang trigger stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Site {
    /// The trigger without `!`, as listed.
    pub trigger: &'static str,
    pub name: &'static str,
    /// Host of the site, for its home page and icon.
    pub domain: &'static str,
    template: &'static str,
}

impl Site {
    /// The site's search for `query`, or its home page when it is blank.
    pub fn url(&self, query: &str) -> String {
        let query = query.trim();
        if query.is_empty() {
            format!("https://{}/", self.domain)
        } else {
            self.template.replace(QUERY, &encode(query))
        }
    }
}

/// The site of `trigger` (without `!`), in any letter case.
pub fn site(trigger: &str) -> Option<Site> {
    table().get(trigger.to_ascii_lowercase().as_str()).copied()
}

fn table() -> &'static HashMap<&'static str, Site> {
    static TABLE_INDEX: OnceLock<HashMap<&'static str, Site>> = OnceLock::new();
    TABLE_INDEX.get_or_init(|| {
        TABLE
            .lines()
            .filter_map(|line| {
                let mut fields = line.split('\t');
                let trigger = fields.next()?;
                let entry = Site {
                    trigger,
                    name: fields.next()?,
                    domain: fields.next()?,
                    template: fields.next()?,
                };
                Some((trigger, entry))
            })
            .collect()
    })
}

/// Where a bang sends the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bang {
    /// Site name, for the command bar.
    pub name: &'static str,
    /// The input without the bang.
    pub query: String,
    /// The site's search for `query`, or its home page when it is empty.
    pub url: String,
}

/// The bang in `input`, when its first or last word is a known `!trigger`.
pub fn expand(input: &str) -> Option<Bang> {
    let words: Vec<&str> = input.split_whitespace().collect();
    let (at, entry) = [0, words.len().checked_sub(1)?]
        .into_iter()
        .find_map(|at| Some((at, site(words[at].strip_prefix('!')?)?)))?;
    let query = words
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != at)
        .map(|(_, word)| *word)
        .collect::<Vec<_>>()
        .join(" ");
    Some(Bang {
        name: entry.name,
        url: entry.url(&query),
        query,
    })
}

/// Percent-encodes everything but unreserved characters, as
/// `encodeURIComponent` does, so the query fits a path or a parameter.
fn encode(query: &str) -> String {
    query
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bangs_work_first_or_last_and_alone() {
        let first = expand("!yt lofi beats").unwrap();
        assert_eq!(first.name, "YouTube");
        assert_eq!(
            first.url,
            "https://www.youtube.com/results?search_query=lofi%20beats"
        );
        assert_eq!(expand("lofi beats !YT").unwrap().url, first.url);
        assert_eq!(expand("!gh").unwrap().url, "https://github.com/");
    }

    #[test]
    fn unknown_or_middle_bangs_are_plain_text() {
        assert_eq!(expand("!nope cats"), None);
        assert_eq!(expand("cats !g dogs"), None);
        assert_eq!(expand("hello world"), None);
        assert_eq!(expand(""), None);
        assert_eq!(expand("!"), None);
    }

    #[test]
    fn sites_give_searches_and_home_pages() {
        let youtube = site("YT").unwrap();
        assert_eq!(youtube.trigger, "yt");
        assert_eq!(youtube.url("  "), "https://www.youtube.com/");
        assert_eq!(
            youtube.url("a b"),
            "https://www.youtube.com/results?search_query=a%20b"
        );
        assert_eq!(site("nope-not-a-bang"), None);
    }

    #[test]
    fn the_whole_list_loads_with_query_slots() {
        assert!(table().len() > 10_000);
        assert!(table().values().all(|entry| entry.template.contains(QUERY)));
        assert_eq!(expand("!wpt Brasil").unwrap().name, "Portuguese Wikipedia");
        assert_eq!(
            expand("ação !g").unwrap().url,
            "https://www.google.com/search?q=a%C3%A7%C3%A3o"
        );
    }
}
