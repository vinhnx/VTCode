//! Reusable fuzzy filtering for TUI list/palette surfaces.
//!
//! The matcher primitives (`normalize_query`, `FuzzyQuery`, `fuzzy_*`) live in
//! [`vtcode_commons::search`]; this module layers the UI-only list abstractions
//! (`exact_terms_match`, `SearchCandidate`, `ListSearchFilter`) on top and
//! re-exports the primitives so `crate::tui::ui::search::*` keeps resolving.

pub(crate) use vtcode_commons::search::{FuzzyQuery, normalize_query};

/// Returns true when every whitespace-separated term in `query` appears as a
/// case-insensitive substring within `candidate`. Both `query` and `candidate`
/// are expected to be pre-lowered (via [`normalize_query`] and construction-time
/// lowering respectively), so this function performs zero allocations.
#[inline]
pub(crate) fn exact_terms_match(query: &str, candidate: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    query.split_whitespace().all(|term| candidate.contains(term))
}

/// One searchable list option: title, optional description, and keyword blob.
///
/// Surfaces that already embed a full haystack (modal `search_value`) can skip
/// this and call [`ListSearchFilter::matches_haystack`] directly.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SearchCandidate<'a> {
    pub title: &'a str,
    pub description: Option<&'a str>,
    pub keywords: &'a str,
}

impl<'a> SearchCandidate<'a> {
    #[must_use]
    pub(crate) fn new(title: &'a str) -> Self {
        Self { title, description: None, keywords: "" }
    }

    #[must_use]
    pub(crate) fn with_description(mut self, description: Option<&'a str>) -> Self {
        self.description = description;
        self
    }

    #[must_use]
    pub(crate) fn with_keywords(mut self, keywords: &'a str) -> Self {
        self.keywords = keywords;
        self
    }

    /// Lowercased haystack: title + description + keywords.
    #[must_use]
    pub(crate) fn haystack(&self) -> String {
        let mut parts = vec![self.title];
        if let Some(description) = self.description {
            parts.push(description);
        }
        if !self.keywords.trim().is_empty() {
            parts.push(self.keywords);
        }
        normalize_query(&parts.join(" "))
    }
}

/// Reusable list filter: one query, many candidates.
///
/// Used by the shared modal (`search_value` haystacks) and standalone TUI
/// pickers (`SearchCandidate`). Matching rules:
/// - empty query matches everything
/// - exact: every whitespace-separated term is a substring of the haystack
/// - fuzzy: nucleo subsequence score against the haystack
#[derive(Debug)]
pub(crate) struct ListSearchFilter {
    query: String,
    fuzzy: bool,
}

impl ListSearchFilter {
    #[must_use]
    pub(crate) fn new(query: &str, fuzzy: bool) -> Self {
        Self { query: normalize_query(query), fuzzy }
    }

    #[must_use]
    pub(crate) fn is_active(&self) -> bool {
        !self.query.is_empty()
    }

    /// Match a prebuilt haystack (modal `search_value`, already lowercased).
    pub(crate) fn matches_haystack(&mut self, haystack: &str) -> bool {
        if !self.is_active() {
            return true;
        }
        if self.fuzzy {
            // Reuse one compiled matcher across a whole list via
            // `filter_haystacks`; this one-shot path is for single items.
            FuzzyQuery::new(&self.query).score(haystack).is_some()
        } else {
            exact_terms_match(&self.query, haystack)
        }
    }

    /// Indices of matching candidates in original order.
    pub(crate) fn filter_indices(&mut self, candidates: &[SearchCandidate<'_>]) -> Vec<usize> {
        if !self.is_active() {
            return (0..candidates.len()).collect();
        }
        let mut fuzzy = self.fuzzy.then(|| FuzzyQuery::new(&self.query));
        candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| {
                let haystack = candidate.haystack();
                match fuzzy.as_mut() {
                    Some(query) => query.score(&haystack).is_some(),
                    None => exact_terms_match(&self.query, &haystack),
                }
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// Indices of matching haystacks in original order.
    pub(crate) fn filter_haystacks(&mut self, haystacks: &[String]) -> Vec<usize> {
        if !self.is_active() {
            return (0..haystacks.len()).collect();
        }
        let mut fuzzy = self.fuzzy.then(|| FuzzyQuery::new(&self.query));
        haystacks
            .iter()
            .enumerate()
            .filter(|(_, haystack)| match fuzzy.as_mut() {
                Some(query) => query.score(haystack).is_some(),
                None => exact_terms_match(&self.query, haystack),
            })
            .map(|(index, _)| index)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_terms_match_requires_substring() {
        // Candidates are pre-lowered (as done by ModalListState construction)
        assert!(exact_terms_match("openai", "openai openai gpt-5.4 gpt-5.4"));
        assert!(exact_terms_match("gpt", "openai openai gpt-5.4 gpt-5.4"));
        assert!(!exact_terms_match("anthropic", "openai openai gpt-5.4 gpt-5.4"));
    }

    #[test]
    fn exact_terms_match_multi_term_requires_all() {
        let candidate = "anthropic anthropic claude 4 sonnet claude-4-sonnet";
        assert!(exact_terms_match("anthropic claude", candidate));
        assert!(exact_terms_match("sonnet", candidate));
        assert!(!exact_terms_match("anthropic gpt", candidate));
    }

    #[test]
    fn exact_terms_match_empty_query_matches_everything() {
        assert!(exact_terms_match("", "anything"));
    }

    #[test]
    fn exact_terms_match_rejects_fuzzy_subsequences() {
        assert!(!exact_terms_match("smr", "src/main.rs"));
    }

    #[test]
    fn exact_terms_match_provider_filtering() {
        let openai = "openai openai gpt-5.4 gpt-5.4 reasoning tools image";
        let anthropic = "anthropic anthropic claude 4 sonnet claude-4-sonnet reasoning tools";
        let gemini = "gemini gemini gemini 2.5 pro gemini-2.5-pro reasoning tools";

        // Single provider term filters correctly
        assert!(exact_terms_match("openai", openai));
        assert!(!exact_terms_match("openai", anthropic));
        assert!(!exact_terms_match("openai", gemini));

        // Provider + model narrows further
        assert!(exact_terms_match("openai gpt", openai));
        assert!(!exact_terms_match("openai claude", openai));

        // Capability filter works across providers
        assert!(exact_terms_match("reasoning", openai));
        assert!(exact_terms_match("reasoning", anthropic));
    }

    #[test]
    fn list_search_filter_empty_query_matches_all() {
        let mut filter = ListSearchFilter::new("   ", false);
        assert!(!filter.is_active());
        let candidates = [
            SearchCandidate::new("Alpha"),
            SearchCandidate::new("Beta").with_description(Some("other")).with_keywords("kw"),
        ];
        assert_eq!(filter.filter_indices(&candidates), vec![0, 1]);
    }

    #[test]
    fn list_search_filter_exact_uses_title_description_and_keywords() {
        let mut filter = ListSearchFilter::new("sonnet", false);
        let candidates = [
            SearchCandidate::new("Claude 4")
                .with_description(Some("sonnet model"))
                .with_keywords(""),
            SearchCandidate::new("GPT-5")
                .with_description(Some("reasoning"))
                .with_keywords("sol"),
            SearchCandidate::new("GLM")
                .with_description(Some("chat"))
                .with_keywords("sonnet-alias"),
        ];
        assert_eq!(filter.filter_indices(&candidates), vec![0, 2]);
    }

    #[test]
    fn list_search_filter_exact_requires_every_term() {
        let mut filter = ListSearchFilter::new("openai tools", false);
        let candidates = [
            SearchCandidate::new("GPT")
                .with_description(Some("tools"))
                .with_keywords("openai"),
            SearchCandidate::new("GPT")
                .with_description(Some("tools"))
                .with_keywords("anthropic"),
        ];
        assert_eq!(filter.filter_indices(&candidates), vec![0]);
    }

    #[test]
    fn list_search_filter_fuzzy_matches_subsequences() {
        let mut filter = ListSearchFilter::new("smr", true);
        let candidates = [
            SearchCandidate::new("src/main.rs"),
            SearchCandidate::new("docs/readme.md"),
        ];
        assert_eq!(filter.filter_indices(&candidates), vec![0]);
    }

    #[test]
    fn list_search_filter_haystack_override() {
        let mut filter = ListSearchFilter::new("tool_display_mode", false);
        let haystacks = vec![
            "ui.tool_display_mode tool display mode expanded compact".to_string(),
            "agent.default_model model".to_string(),
        ];
        assert_eq!(filter.filter_haystacks(&haystacks), vec![0]);
    }
}
