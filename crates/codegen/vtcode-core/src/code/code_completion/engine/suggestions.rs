use super::CompletionKind;
use crate::code::code_completion::context::CompletionContext;
use crate::code::code_completion::learning::CompletionLearningData;
use hashbrown::HashMap;

/// Code completion suggestion with metadata
#[derive(Debug, Clone)]
pub struct CompletionSuggestion {
    /// Completion acceptance rate (target: 70%)
    pub acceptance_rate: f64,
    /// Learning data for improving suggestions
    pub learning_data: CompletionLearningData,
    pub text: String,
    pub kind: CompletionKind,
    pub confidence: f64,
    pub context: CompletionContext,
    pub metadata: HashMap<String, String>,
    pub accepted_count: usize,
    pub rejected_count: usize,
}

impl CompletionSuggestion {
    pub fn new(text: String, kind: CompletionKind, context: CompletionContext) -> Self {
        Self {
            acceptance_rate: 0.0,
            learning_data: CompletionLearningData::default(),
            text,
            kind,
            confidence: 0.5,
            context,
            metadata: HashMap::new(),
            accepted_count: 0,
            rejected_count: 0,
        }
    }

    /// Update acceptance rate based on feedback
    pub fn update_acceptance_rate(&mut self, accepted: bool) {
        if accepted {
            self.accepted_count += 1;
        } else {
            self.rejected_count += 1;
        }

        let total = self.accepted_count + self.rejected_count;
        if total > 0 {
            self.acceptance_rate = self.accepted_count as f64 / total as f64;
        }
    }
}
