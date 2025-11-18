pub mod data;
pub mod feedback;

pub use data::CompletionLearningData;
pub use feedback::FeedbackProcessor;

use std::collections::HashMap;

/// Learning system for improving completion suggestions
pub struct LearningSystem {
    learning_data: CompletionLearningData,
    feedback_processor: FeedbackProcessor,
}

impl Default for LearningSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl LearningSystem {
    pub fn new() -> Self {
        Self {
            learning_data: CompletionLearningData::default(),
            feedback_processor: FeedbackProcessor::new(),
        }
    }

    /// Process user feedback to improve future suggestions
    pub fn process_feedback(&mut self, suggestion_text: &str, accepted: bool, context: &str) {
        self.feedback_processor
            .process(suggestion_text, accepted, context);
        self.learning_data
            .update_from_feedback(suggestion_text, accepted);
    }

    /// Get learning insights for a given context
    pub fn get_insights(&self, context: &str) -> HashMap<String, f64> {
        self.learning_data.get_context_insights(context)
    }
}
