//! Grouping and ranking of command bar results.

use crate::fuzzy;

/// Result groups, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    /// An address typed by the user; shown first when present.
    Address,
    /// Open tabs.
    Tabs,
    /// Pinned tabs and essentials.
    Pinned,
    /// Built-in commands.
    Commands,
    /// Visited pages.
    History,
    /// Web search fallback; always last.
    Search,
}

impl Group {
    fn limit(self) -> usize {
        match self {
            Self::Address | Self::Search => 1,
            Self::Tabs | Self::Pinned | Self::History => 5,
            Self::Commands => 4,
        }
    }

    fn shown_when_empty(self) -> bool {
        matches!(self, Self::Tabs | Self::Pinned | Self::Commands)
    }
}

/// Something the command bar can offer.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate<T> {
    /// Group it belongs to.
    pub group: Group,
    /// Main line, matched against the query.
    pub title: String,
    /// Second line (usually a URL), also matched.
    pub subtitle: String,
    /// What choosing it does.
    pub target: T,
}

/// Filters and orders candidates for `query`: grouped in [`Group`] order,
/// best matches first within a group (ties keep input order), each group
/// capped. With an empty query only tabs, pinned and commands are shown, in
/// input order.
pub fn rank<T: Clone>(query: &str, candidates: &[Candidate<T>]) -> Vec<Candidate<T>> {
    let empty = query.trim().is_empty();
    let mut scored: Vec<(Group, u32, usize)> = candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| !empty || candidate.group.shown_when_empty())
        .filter_map(|(index, candidate)| {
            let title = fuzzy::score(query, &candidate.title);
            let subtitle = fuzzy::score(query, &candidate.subtitle);
            let best = title.max(subtitle)?;
            Some((candidate.group, best, index))
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));

    let mut shown_in_group = 0;
    let mut current_group = None;
    scored
        .into_iter()
        .filter(|(group, _, _)| {
            if current_group != Some(*group) {
                current_group = Some(*group);
                shown_in_group = 0;
            }
            shown_in_group += 1;
            shown_in_group <= group.limit()
        })
        .map(|(_, _, index)| candidates[index].clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(group: Group, title: &str) -> Candidate<&'static str> {
        Candidate {
            group,
            title: title.to_string(),
            subtitle: String::new(),
            target: "",
        }
    }

    fn titles(results: &[Candidate<&str>]) -> Vec<String> {
        results.iter().map(|c| c.title.clone()).collect()
    }

    #[test]
    fn groups_come_out_in_fixed_order() {
        let candidates = [
            candidate(Group::Search, "Search for git"),
            candidate(Group::History, "git history"),
            candidate(Group::Commands, "git command"),
            candidate(Group::Tabs, "git tab"),
            candidate(Group::Address, "git.com"),
        ];
        let results = rank("git", &candidates);
        let groups: Vec<Group> = results.iter().map(|c| c.group).collect();
        assert_eq!(
            groups,
            [
                Group::Address,
                Group::Tabs,
                Group::Commands,
                Group::History,
                Group::Search
            ]
        );
    }

    #[test]
    fn better_matches_rank_first_within_a_group() {
        let candidates = [
            candidate(Group::Tabs, "Old wiki notes"),
            candidate(Group::Tabs, "Wikipedia"),
            candidate(Group::Tabs, "Mail"),
        ];
        assert_eq!(
            titles(&rank("wiki", &candidates)),
            ["Wikipedia", "Old wiki notes"]
        );
    }

    #[test]
    fn subtitle_matches_count() {
        let mut tab = candidate(Group::Tabs, "Home");
        tab.subtitle = "https://github.com/".into();
        assert_eq!(rank("github", &[tab]).len(), 1);
    }

    #[test]
    fn empty_query_hides_history_and_search_and_keeps_order() {
        let candidates = [
            candidate(Group::History, "h"),
            candidate(Group::Tabs, "second"),
            candidate(Group::Tabs, "first"),
            candidate(Group::Search, "s"),
            candidate(Group::Commands, "c"),
        ];
        assert_eq!(titles(&rank("", &candidates)), ["second", "first", "c"]);
    }

    #[test]
    fn groups_are_capped() {
        let candidates: Vec<_> = (0..10)
            .map(|i| candidate(Group::Tabs, &format!("tab {i}")))
            .collect();
        assert_eq!(rank("tab", &candidates).len(), Group::Tabs.limit());
    }
}
