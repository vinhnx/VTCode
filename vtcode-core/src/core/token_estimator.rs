use std::sync::Arc;

/// Estimates the number of tokens contained in a string.
pub trait TokenEstimator: Send + Sync {
    /// Estimate the number of tokens for the provided text.
    fn estimate_tokens(&self, text: &str) -> usize;
}

/// A simple estimator that divides characters by a fixed ratio.
/// Uses byte length for efficiency (avoiding UTF-8 char iteration).
#[derive(Debug, Clone, Copy)] // Changed to Copy for zero-cost clones
pub struct CharacterRatioTokenEstimator {
    chars_per_token: usize,
}

impl CharacterRatioTokenEstimator {
    /// Create a new estimator that assumes the provided number of characters per token.
    #[inline]
    pub const fn new(chars_per_token: usize) -> Self {
        // Use const fn for compile-time evaluation when possible
        let normalized = if chars_per_token == 0 {
            1
        } else {
            chars_per_token
        };
        Self {
            chars_per_token: normalized,
        }
    }

    /// Access the configured character-per-token ratio.
    #[inline]
    pub const fn chars_per_token(&self) -> usize {
        self.chars_per_token
    }
}

impl Default for CharacterRatioTokenEstimator {
    #[inline]
    fn default() -> Self {
        Self::new(4)
    }
}

impl TokenEstimator for CharacterRatioTokenEstimator {
    #[inline]
    fn estimate_tokens(&self, text: &str) -> usize {
        if text.is_empty() {
            return 0;
        }

        let byte_len = text.len();
        let tokens = byte_len.div_ceil(self.chars_per_token);
        tokens.max(1)
    }
}

/// Shared token estimator handle.
pub type SharedTokenEstimator = Arc<dyn TokenEstimator>;
