use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use super::FileEntry;

#[derive(Debug)]
pub(crate) struct ScoredPath {
    pub(crate) score: usize,
    pub(crate) index: usize,
    pub(crate) is_dir: bool,
    pub(crate) path_lower: String,
}

#[derive(Debug, Default)]
pub(crate) struct SearchScorer {
    matcher: Matcher,
    pattern: Option<Pattern>,
    query_lower: String,
    buffer: Vec<char>,
}

impl SearchScorer {
    pub(crate) fn new() -> Self {
        Self {
            matcher: Matcher::new(Config::DEFAULT.match_paths()),
            pattern: None,
            query_lower: String::new(),
            buffer: Vec::new(),
        }
    }

    pub(crate) fn set_query(&mut self, query: &str) {
        self.query_lower = query.to_lowercase();
        self.pattern = if self.query_lower.is_empty() {
            None
        } else {
            Some(Pattern::parse(&self.query_lower, CaseMatching::Ignore, Normalization::Smart))
        };
        self.buffer.clear();
    }

    pub(crate) fn score(&mut self, entry: &FileEntry, index: usize) -> Option<ScoredPath> {
        let path_lower = entry.relative_path.to_lowercase();

        let score = if let Some(pattern) = self.pattern.as_ref()
            && let Some(fuzzy_score) =
                Self::fuzzy_match(&path_lower, &mut self.matcher, pattern, &self.query_lower, &mut self.buffer)
        {
            let mut score = fuzzy_score;
            if !path_lower.contains('/') {
                score += 1000;
            }
            if path_lower == self.query_lower {
                score += 10000;
            } else if let Some(file_name) = path_lower.rsplit('/').next() {
                if file_name == self.query_lower {
                    score += 5000;
                } else if file_name.starts_with(&self.query_lower) {
                    score += 2000;
                }
            }
            score
        } else if !self.query_lower.is_empty() && path_lower.contains(&self.query_lower) {
            let mut score = Self::calculate_match_score(&path_lower, &self.query_lower);
            if !path_lower.contains('/') {
                score += 1000;
            }
            score
        } else {
            return None;
        };

        Some(ScoredPath { score, index, is_dir: entry.is_dir, path_lower })
    }

    pub(crate) fn sort_results(scored: &mut [ScoredPath]) {
        scored.sort_unstable_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => b.score.cmp(&a.score).then_with(|| a.path_lower.cmp(&b.path_lower)),
        });
    }

    fn fuzzy_match(
        path: &str,
        matcher: &mut Matcher,
        pattern: &Pattern,
        query_lower: &str,
        buffer: &mut Vec<char>,
    ) -> Option<usize> {
        if query_lower.is_empty() {
            return Some(1000);
        }

        let utf32_path = Utf32Str::new(path, buffer);
        let score = pattern.score(utf32_path, matcher)?;

        let mut adjusted_score = score as usize;

        if let Some(filename) = path.rsplit('/').next()
            && filename.to_lowercase().contains(query_lower)
        {
            adjusted_score += 500;
        }

        Some(adjusted_score)
    }

    fn calculate_match_score(path: &str, query: &str) -> usize {
        let mut score: usize = 0;

        if path == query {
            return 10000;
        }

        if path.starts_with(query) {
            score += 1000;
        }

        if let Some(file_name) = path.rsplit('/').next() {
            if file_name == query {
                score += 2000;
            } else if file_name.contains(query) {
                score += 500;
            }
            if file_name.starts_with(query) {
                score += 200;
            }
        }

        for segment in path.split('/') {
            if segment.contains(query) {
                score += 50;
            }
        }

        let depth = path.matches('/').count();
        score = score.saturating_sub(depth * 5);

        let matches = path.matches(query).count();
        score += matches * 10;

        score
    }
}
