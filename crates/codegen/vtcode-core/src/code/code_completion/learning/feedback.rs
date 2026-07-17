/// Feedback processor for analyzing user interactions
pub struct FeedbackProcessor {
    feedback_history: Vec<FeedbackEntry>,
}

#[derive(Debug, Clone)]
struct FeedbackEntry {
    suggestion: String,
    accepted: bool,
    context: String,
    timestamp: std::time::SystemTime,
}

impl Default for FeedbackProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl FeedbackProcessor {
    pub fn new() -> Self {
        Self {
            feedback_history: Vec::new(),
        }
    }

    /// Process user feedback
    pub fn process(&mut self, suggestion: &str, accepted: bool, context: &str) {
        let entry = FeedbackEntry {
            suggestion: suggestion.into(),
            accepted,
            context: context.into(),
            timestamp: std::time::SystemTime::now(),
        };

        self.feedback_history.push(entry);

        // Keep only recent feedback (last 1000 entries)
        if self.feedback_history.len() > 1000 {
            self.feedback_history.remove(0);
        }
    }

    /// Get acceptance rate for a specific pattern
    pub fn get_acceptance_rate(&self, pattern: &str) -> f64 {
        use std::time::{Duration, SystemTime};

        let now = SystemTime::now();
        let mut weighted_sum = 0.0;
        let mut total_weight = 0.0;

        for entry in &self.feedback_history {
            if !entry.suggestion.contains(pattern) {
                continue;
            }

            let age_weight = now
                .duration_since(entry.timestamp)
                .map(|duration| {
                    // Recent feedback counts more. Anything older than 30 days is heavily down-weighted.
                    let thirty_days = Duration::from_secs(60 * 60 * 24 * 30);
                    1.0 - (duration.as_secs_f64() / thirty_days.as_secs_f64()).min(0.9)
                })
                .unwrap_or(1.0);

            let context_weight = if entry.context.is_empty() {
                0.8
            } else {
                // Prefer matches where the usage context was recorded.
                1.0
            };

            let weight = age_weight * context_weight;
            total_weight += weight;
            if entry.accepted {
                weighted_sum += weight;
            }
        }

        if total_weight == 0.0 {
            return 0.5;
        }

        weighted_sum / total_weight
    }
}
