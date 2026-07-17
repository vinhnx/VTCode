use super::suggestions::CompletionSuggestion;

/// Suggestion ranking and filtering system
pub struct SuggestionRanker {
    confidence_threshold: f64,
    max_suggestions: usize,
}

impl SuggestionRanker {
    pub fn new() -> Self {
        Self {
            confidence_threshold: 0.3,
            max_suggestions: 10,
        }
    }

    /// Rank and filter suggestions based on confidence and relevance
    pub fn rank_suggestions(
        &self,
        mut suggestions: Vec<CompletionSuggestion>,
    ) -> Vec<CompletionSuggestion> {
        // Filter by confidence threshold
        suggestions.retain(|s| s.confidence >= self.confidence_threshold);

        // Sort by confidence and acceptance rate
        suggestions.sort_by(|a, b| {
            let score_a = a.confidence * 0.7 + a.acceptance_rate * 0.3;
            let score_b = b.confidence * 0.7 + b.acceptance_rate * 0.3;
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Limit to max suggestions
        suggestions.truncate(self.max_suggestions);
        suggestions
    }
}

impl Default for SuggestionRanker {
    fn default() -> Self {
        Self::new()
    }
}
