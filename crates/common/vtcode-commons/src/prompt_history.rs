//! Prompt-history ranking shared by the CLI history picker and the desktop
//! history palette.
//!
//! Both surfaces must find the same prompts for the same query, so the match,
//! rank, dedup, and cap rules live here instead of being reimplemented per UI.
//! Matching is [`crate::search::FuzzyQuery`] (nucleo) so a multi-word query
//! such as `impl chg` finds `Implement current changes`; a plain substring
//! test cannot express that.

use crate::search::FuzzyQuery;

/// Maximum prompt-history rows any surface shows for one query.
///
/// The budget covers the **merged** result, not each section: callers that
/// split rows into sections must size the later ones from what the earlier
/// ones consumed, or a long live thread plus cross-session recents could
/// render twice this many rows.
pub const MAX_PROMPT_HISTORY_ROWS: usize = 200;

/// Indices of `candidates` matching `query`, best match first.
///
/// Callers supply candidates **newest first** (live thread first, then
/// archived prompts). With an empty query every candidate matches with score
/// zero, so the stable sort preserves that order; with a query, ties also keep
/// the newer candidate ahead. Blank prompts never match.
///
/// After ranking, duplicate content is collapsed to its best-scoring
/// occurrence (so on a tie the live/newer entry wins), and the result is
/// capped at `limit`.
#[must_use]
pub fn rank_prompt_history(query: &str, candidates: &[&str], limit: usize) -> Vec<usize> {
    if limit == 0 {
        return Vec::new();
    }
    let query = query.trim();
    let mut fuzzy = (!query.is_empty()).then(|| FuzzyQuery::new(query));
    let mut scored = Vec::with_capacity(candidates.len());
    for (index, candidate) in candidates.iter().enumerate() {
        if candidate.trim().is_empty() {
            continue;
        }
        let score = match fuzzy.as_mut() {
            Some(fuzzy) => match fuzzy.score(candidate) {
                Some(score) => score,
                None => continue,
            },
            None => 0,
        };
        scored.push((index, score));
    }
    // Stable: equal scores keep the caller's newest-first order.
    scored.sort_by_key(|&(_, score)| std::cmp::Reverse(score));

    let mut seen = hashbrown::HashSet::with_capacity(scored.len());
    let mut ranked = Vec::with_capacity(scored.len().min(limit));
    for (index, _) in scored {
        let Some(content) = candidates.get(index) else {
            continue;
        };
        if !seen.insert(*content) {
            continue;
        }
        ranked.push(index);
        if ranked.len() == limit {
            break;
        }
    }
    ranked
}

/// Compact history row label: `first line · 3h ago · +2 lines`.
///
/// Only the first line is shown, so multi-line prompts stay one row and say
/// how much they hide. Control characters are replaced so an escape sequence
/// cannot reach the picker's rows.
#[must_use]
pub fn prompt_history_row_label(content: &str, time_label: &str) -> String {
    let mut lines = content.lines();
    let first = lines.next().unwrap_or("");
    let extra_lines = lines.count();
    let sanitized: String = first.chars().map(|ch| if ch.is_control() { ' ' } else { ch }).collect();
    let first = if sanitized.is_empty() && extra_lines > 0 {
        "…"
    } else {
        sanitized.as_str()
    };
    let mut parts = vec![first.to_owned()];
    if !time_label.is_empty() {
        parts.push(time_label.to_owned());
    }
    if extra_lines > 0 {
        if extra_lines == 1 {
            parts.push("+1 line".to_owned());
        } else {
            parts.push(format!("+{extra_lines} lines"));
        }
    }
    parts.join(" \u{b7} ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Newest-first, live thread before archived prompts.
    fn candidates<'a>() -> Vec<&'a str> {
        vec![
            "Implement current changes",
            "Review stale diffs",
            "git status",
            "git diff",
        ]
    }

    fn ranked_contents(query: &str, limit: usize) -> Vec<String> {
        let candidates = candidates();
        rank_prompt_history(query, &candidates, limit)
            .into_iter()
            .map(|index| candidates[index].to_owned())
            .collect()
    }

    #[test]
    fn multi_word_query_matches_non_contiguous_text() {
        // "impl chg" cannot match by substring; fuzzy must span the phrase.
        assert_eq!(ranked_contents("impl chg", MAX_PROMPT_HISTORY_ROWS), vec!["Implement current changes"]);
        assert_eq!(ranked_contents("stff", MAX_PROMPT_HISTORY_ROWS), vec!["Review stale diffs"]);
    }

    #[test]
    fn empty_query_preserves_newest_first_order() {
        assert_eq!(ranked_contents("   ", MAX_PROMPT_HISTORY_ROWS), candidates());
    }

    #[test]
    fn empty_query_order_is_asymmetric() {
        // Same two prompts, opposite supply order: the ranker must not sort
        // them alphabetically or by any property other than arrival order.
        let newest_first = ["newer prompt", "older prompt"];
        let oldest_first = ["older prompt", "newer prompt"];
        assert_eq!(rank_prompt_history("", &newest_first, MAX_PROMPT_HISTORY_ROWS), vec![0, 1]);
        assert_eq!(rank_prompt_history("", &oldest_first, MAX_PROMPT_HISTORY_ROWS), vec![0, 1]);
    }

    #[test]
    fn blank_and_whitespace_prompts_never_match() {
        let candidates = ["", "   ", "real prompt"];
        assert_eq!(rank_prompt_history("", &candidates, MAX_PROMPT_HISTORY_ROWS), vec![2]);
    }

    #[test]
    fn duplicate_content_collapses_to_newest_occurrence() {
        // The duplicate lives further down, i.e. it is the *older* copy.
        let candidates = ["cargo test", "unrelated", "cargo test"];
        let ranked = rank_prompt_history("", &candidates, MAX_PROMPT_HISTORY_ROWS);
        assert_eq!(ranked, vec![0, 1], "dedup keeps the first (newest) copy");
    }

    #[test]
    fn query_ranking_beats_arrival_order() {
        // The older entry matches the whole query, so it must outrank the
        // newer partial match; score trumps newest-first for non-empty queries.
        let candidates = ["git", "git status"];
        let ranked = rank_prompt_history("git status", &candidates, MAX_PROMPT_HISTORY_ROWS);
        assert_eq!(ranked.first().copied().map(|index| candidates[index]), Some("git status"));
    }

    #[test]
    fn cap_applies_to_the_merged_result() {
        let owned: Vec<String> = (0..250).map(|index| format!("prompt {index:03}")).collect();
        let candidates: Vec<&str> = owned.iter().map(String::as_str).collect();
        assert_eq!(rank_prompt_history("", &candidates, MAX_PROMPT_HISTORY_ROWS).len(), MAX_PROMPT_HISTORY_ROWS);
        assert_eq!(rank_prompt_history("", &candidates, 0).len(), 0);
    }

    #[test]
    fn no_match_yields_no_rows() {
        assert!(ranked_contents("zzzznomatch", MAX_PROMPT_HISTORY_ROWS).is_empty());
    }

    #[test]
    fn row_label_reports_hidden_lines() {
        assert_eq!(prompt_history_row_label("cargo test", "3h ago"), "cargo test \u{b7} 3h ago");
        assert_eq!(prompt_history_row_label("cargo test", ""), "cargo test");
        assert_eq!(prompt_history_row_label("one\ntwo", "just now"), "one \u{b7} just now \u{b7} +1 line");
        assert_eq!(prompt_history_row_label("one\ntwo\nthree", ""), "one \u{b7} +2 lines");
    }

    #[test]
    fn row_label_sanitizes_control_characters() {
        let label = prompt_history_row_label("safe\u{1b}[31mevil", "");
        assert!(!label.contains('\u{1b}'), "escape must not survive: {label:?}");
        assert!(label.starts_with("safe [31mevil"));
    }

    #[test]
    fn row_label_uses_ellipsis_for_continuation_only_prompts() {
        assert_eq!(prompt_history_row_label("\nsecond line", ""), "… \u{b7} +1 line");
    }
}
