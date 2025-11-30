use std::collections::HashMap;

/// Learning data for improving completion suggestions
#[derive(Debug, Clone, Default)]
pub struct CompletionLearningData {
    /// Pattern acceptance rates by context
    pub pattern_acceptance: HashMap<String, f64>,
    /// Frequently used symbols by context
    pub symbol_frequency: HashMap<String, usize>,
    /// User preferences and patterns
    pub user_preferences: HashMap<String, f64>,
    /// Context-specific insights
    pub context_insights: HashMap<String, HashMap<String, f64>>,
}

impl CompletionLearningData {
    /// Update learning data based on user feedback
    pub fn update_from_feedback(&mut self, suggestion: &str, accepted: bool) {
        let current_rate = self.pattern_acceptance.get(suggestion).unwrap_or(&0.5);
        let new_rate = if accepted {
            (current_rate + 0.1).min(1.0)
        } else {
            (current_rate - 0.1).max(0.0)
        };
        self.pattern_acceptance.insert(suggestion.into(), new_rate);
    }

    /// Get insights for a specific context
    pub fn get_context_insights(&self, context: &str) -> HashMap<String, f64> {
        self.context_insights
            .get(context)
            .cloned()
            .unwrap_or_default()
    }

    /// Record symbol usage frequency
    pub fn record_symbol_usage(&mut self, symbol: &str) {
        *self.symbol_frequency.entry(symbol.into()).or_insert(0) += 1;
    }
}
