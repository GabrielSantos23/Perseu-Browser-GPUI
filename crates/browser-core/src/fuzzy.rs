//! Small subsequence matcher for the command bar.

const MATCH: u32 = 1;
const CONSECUTIVE_BONUS: u32 = 5;
const WORD_START_BONUS: u32 = 8;
const SUBSTRING_BONUS: u32 = 20;
const PREFIX_BONUS: u32 = 15;

/// Scores how well `query` matches `text`, ignoring case and spaces in the
/// query. Every query character must appear in order. Higher is better;
/// `None` means no match. An empty query matches everything with score 0.
pub fn score(query: &str, text: &str) -> Option<u32> {
    let query: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if query.is_empty() {
        return Some(0);
    }
    let text: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();

    let mut total = 0;
    let mut previous: Option<usize> = None;
    let mut from = 0;
    for wanted in &query {
        let offset = text[from..].iter().position(|c| c == wanted)?;
        let index = from + offset;
        total += MATCH;
        if previous.is_some_and(|last| last + 1 == index) {
            total += CONSECUTIVE_BONUS;
        }
        if index == 0 || !text[index - 1].is_alphanumeric() {
            total += WORD_START_BONUS;
        }
        previous = Some(index);
        from = index + 1;
    }

    let compact: String = query.iter().collect();
    let haystack: String = text.iter().collect();
    if haystack.starts_with(&compact) {
        total += PREFIX_BONUS;
    }
    if haystack.contains(&compact) {
        total += SUBSTRING_BONUS;
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_every_character_in_order() {
        assert!(score("gh", "GitHub").is_some());
        assert!(score("hg", "GitHub").is_none());
        assert!(score("xyz", "GitHub").is_none());
    }

    #[test]
    fn empty_query_matches_everything() {
        assert_eq!(score("  ", "anything"), Some(0));
    }

    #[test]
    fn ignores_case_and_query_spaces() {
        assert!(score("New Sp", "new space").is_some());
    }

    #[test]
    fn prefers_prefix_and_word_starts_over_scattered_matches() {
        let prefix = score("wiki", "Wikipedia").unwrap();
        let inside = score("wiki", "Old wiki").unwrap();
        let scattered = score("wiki", "w-i-k-i").unwrap();
        assert!(prefix > inside, "{prefix} > {inside}");
        assert!(inside > scattered, "{inside} > {scattered}");
    }

    #[test]
    fn acronyms_score_on_word_starts() {
        assert!(score("rct", "Reopen closed tab") > score("rct", "rxcxtx"));
    }
}
