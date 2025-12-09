//! Configuration management for tool improvements system
//!
//! Defines all tunable parameters for similarity scoring, time decay,
//! pattern detection, and cache behavior.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Configuration for the improvements system
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImprovementsConfig {
    /// Similarity scoring configuration
    pub similarity: SimilarityConfig,

    /// Time decay configuration
    pub time_decay: TimeDecayConfig,

    /// Pattern detection configuration
    pub patterns: PatternConfig,

    /// Cache configuration
    pub cache: CacheConfig,

    /// Context management
    pub context: ContextConfig,

    /// Fallback chain configuration
    pub fallback: FallbackConfig,
}

/// Similarity scoring thresholds and weights
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarityConfig {
    /// Minimum similarity score (0.0-1.0) to consider tools related
    pub min_similarity_threshold: f32,

    /// Score considered "high similarity" (0.0-1.0)
    pub high_similarity_threshold: f32,

    /// Weight for argument similarity (0.0-1.0)
    pub argument_weight: f32,

    /// Weight for return type similarity (0.0-1.0)
    pub return_type_weight: f32,

    /// Weight for description similarity (0.0-1.0)
    pub description_weight: f32,

    /// Weight for recent success (0.0-1.0)
    pub success_history_weight: f32,
}

/// Time decay configuration for effectiveness scores
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeDecayConfig {
    /// Decay constant (lambda in exp(-lambda * age_hours))
    /// Higher values = faster decay
    pub decay_constant: f32,

    /// Age at which score drops to 50% (hours)
    pub half_life_hours: f32,

    /// Minimum score after decay (prevents dropping to zero)
    pub minimum_score: f32,

    /// Window for considering recent successes (hours)
    pub recent_window_hours: f32,
}

/// Pattern detection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternConfig {
    /// Minimum tools in sequence to detect pattern
    pub min_sequence_length: usize,

    /// Time window for detecting patterns (seconds)
    pub pattern_window_seconds: u64,

    /// Confidence threshold for pattern detection
    pub confidence_threshold: f32,

    /// Maximum number of patterns to track
    pub max_patterns: usize,

    /// Enable advanced pattern detection (ML-ready)
    pub enable_advanced_detection: bool,
}

/// Cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Maximum cache size (number of entries)
    pub max_entries: usize,

    /// Cache entry time-to-live
    pub ttl: Duration,

    /// Enable result caching
    pub enable_result_cache: bool,

    /// Enable metadata caching
    pub enable_metadata_cache: bool,

    /// Enable pattern cache
    pub enable_pattern_cache: bool,
}

/// Context management configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    /// Maximum context window size (tokens)
    pub max_context_tokens: usize,

    /// Threshold for context truncation (% full)
    pub truncation_threshold_percent: f32,

    /// Enable aggressive context compaction
    pub enable_compaction: bool,

    /// Maximum history to retain
    pub max_history_entries: usize,
}

/// Fallback chain configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackConfig {
    /// Maximum fallback attempts
    pub max_attempts: usize,

    /// Backoff multiplier between retries
    pub backoff_multiplier: f32,

    /// Initial backoff duration
    pub initial_backoff_ms: u64,

    /// Maximum backoff duration
    pub max_backoff_ms: u64,

    /// Enable exponential backoff
    pub enable_exponential_backoff: bool,
}

impl Default for SimilarityConfig {
    fn default() -> Self {
        Self {
            min_similarity_threshold: 0.6,
            high_similarity_threshold: 0.8,
            argument_weight: 0.4,
            return_type_weight: 0.3,
            description_weight: 0.2,
            success_history_weight: 0.1,
        }
    }
}

impl Default for TimeDecayConfig {
    fn default() -> Self {
        Self {
            decay_constant: 0.1,
            half_life_hours: 24.0,
            minimum_score: 0.1,
            recent_window_hours: 1.0,
        }
    }
}

impl Default for PatternConfig {
    fn default() -> Self {
        Self {
            min_sequence_length: 3,
            pattern_window_seconds: 300,
            confidence_threshold: 0.75,
            max_patterns: 100,
            enable_advanced_detection: true,
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            ttl: Duration::from_secs(3600),
            enable_result_cache: true,
            enable_metadata_cache: true,
            enable_pattern_cache: true,
        }
    }
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_context_tokens: 100_000,
            truncation_threshold_percent: 85.0,
            enable_compaction: true,
            max_history_entries: 100,
        }
    }
}

impl Default for FallbackConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            backoff_multiplier: 2.0,
            initial_backoff_ms: 100,
            max_backoff_ms: 5000,
            enable_exponential_backoff: true,
        }
    }
}

impl ImprovementsConfig {
    /// Load configuration from TOML file
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        toml::from_str(&content).map_err(|e| anyhow::anyhow!("failed to parse config: {}", e))
    }

    /// Save configuration to TOML file
    pub fn to_file(&self, path: &str) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Validate configuration values
    pub fn validate(&self) -> Result<(), String> {
        // Similarity config validation
        if !(0.0..=1.0).contains(&self.similarity.min_similarity_threshold) {
            return Err("min_similarity_threshold must be between 0.0 and 1.0".to_string());
        }
        if !(0.0..=1.0).contains(&self.similarity.high_similarity_threshold) {
            return Err("high_similarity_threshold must be between 0.0 and 1.0".to_string());
        }

        // Time decay validation
        if self.time_decay.decay_constant <= 0.0 {
            return Err("decay_constant must be positive".to_string());
        }
        if self.time_decay.half_life_hours <= 0.0 {
            return Err("half_life_hours must be positive".to_string());
        }

        // Pattern validation
        if self.patterns.min_sequence_length < 2 {
            return Err("min_sequence_length must be at least 2".to_string());
        }
        if !(0.0..=1.0).contains(&self.patterns.confidence_threshold) {
            return Err("confidence_threshold must be between 0.0 and 1.0".to_string());
        }

        // Context validation
        if self.context.max_context_tokens == 0 {
            return Err("max_context_tokens must be positive".to_string());
        }
        if !(0.0..=100.0).contains(&self.context.truncation_threshold_percent) {
            return Err("truncation_threshold_percent must be between 0.0 and 100.0".to_string());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_is_valid() {
        let config = ImprovementsConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_similarity() {
        let mut config = ImprovementsConfig::default();
        config.similarity.min_similarity_threshold = 1.5;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_decay() {
        let mut config = ImprovementsConfig::default();
        config.time_decay.decay_constant = -0.1;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_pattern() {
        let mut config = ImprovementsConfig::default();
        config.patterns.min_sequence_length = 1;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_serialization() {
        let config = ImprovementsConfig::default();
        let toml_str = toml::to_string_pretty(&config).expect("serialization failed");
        assert!(toml_str.contains("min_similarity_threshold"));
        assert!(toml_str.contains("decay_constant"));
    }
}
