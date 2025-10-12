use std::sync::Arc;

/// Estimates the number of tokens contained in a string.
pub trait TokenEstimator: Send + Sync {
    /// Estimate the number of tokens for the provided text.
    fn estimate_tokens(&self, text: &str) -> usize;
}

/// A simple estimator that divides characters by a fixed ratio.
#[derive(Debug, Clone)]
pub struct CharacterRatioTokenEstimator {
    chars_per_token: usize,
}

impl CharacterRatioTokenEstimator {
    /// Create a new estimator that assumes the provided number of characters per token.
    pub fn new(chars_per_token: usize) -> Self {
        let normalized = chars_per_token.max(1);
        Self {
            chars_per_token: normalized,
        }
    }

    /// Access the configured character-per-token ratio.
    pub fn chars_per_token(&self) -> usize {
        self.chars_per_token
    }
}

impl Default for CharacterRatioTokenEstimator {
    fn default() -> Self {
        Self::new(4)
    }
}

impl TokenEstimator for CharacterRatioTokenEstimator {
    fn estimate_tokens(&self, text: &str) -> usize {
        if text.is_empty() {
            return 0;
        }

        let byte_len = text.len();
        let tokens = (byte_len + (self.chars_per_token - 1)) / self.chars_per_token;
        tokens.max(1)
    }
}

/// Shared token estimator handle.
pub type SharedTokenEstimator = Arc<dyn TokenEstimator>;
