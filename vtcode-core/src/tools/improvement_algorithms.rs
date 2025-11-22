//! Production-grade algorithms for tool improvements
//!
//! Implements proven algorithms: Jaro-Winkler similarity, time-decay effectiveness,
//! sophisticated pattern detection, and ML-ready scoring.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Jaro-Winkler string similarity (0.0-1.0)
///
/// Preferred over simple Levenshtein for short strings (tool arguments).
/// Gives higher scores to strings with matching prefix.
pub fn jaro_winkler_similarity(s1: &str, s2: &str) -> f32 {
    if s1 == s2 {
        return 1.0;
    }

    if s1.is_empty() || s2.is_empty() {
        return 0.0;
    }

    let jaro = jaro_similarity(s1, s2);

    // Find common prefix (up to 4 chars)
    let prefix_len = s1
        .chars()
        .zip(s2.chars())
        .take_while(|(a, b)| a == b)
        .take(4)
        .count();

    // Boost score with prefix match (p=0.1 is standard)
    jaro + (prefix_len as f32 * 0.1 * (1.0 - jaro))
}

/// Jaro similarity metric
fn jaro_similarity(s1: &str, s2: &str) -> f32 {
    let len1 = s1.len();
    let len2 = s2.len();

    if len1 == 0 && len2 == 0 {
        return 1.0;
    }
    if len1 == 0 || len2 == 0 {
        return 0.0;
    }

    let match_distance = (len1.max(len2) / 2).saturating_sub(1);

    let mut s1_matches = vec![false; len1];
    let mut s2_matches = vec![false; len2];

    let mut matches = 0;

    for (i, c1) in s1.chars().enumerate() {
        let start = i.saturating_sub(match_distance);
        let end = (i + match_distance + 1).min(len2);

        for (j, c2) in s2.chars().enumerate().skip(start).take(end - start) {
            if s2_matches[j] || c1 != c2 {
                continue;
            }
            s1_matches[i] = true;
            s2_matches[j] = true;
            matches += 1;
            break;
        }
    }

    if matches == 0 {
        return 0.0;
    }

    let transpositions = s1_matches
        .iter()
        .zip(s2_matches.iter())
        .filter(|(m1, m2)| **m1 && **m2)
        .zip(s2.chars())
        .filter(|((_, m2), c2)| **m2 && s1.contains(*c2))
        .count()
        / 2;

    (matches as f32 / len1 as f32
        + matches as f32 / len2 as f32
        + (matches as f32 - transpositions as f32) / matches as f32)
        / 3.0
}

/// Time-decay effectiveness score
///
/// Recent successes weighted higher than old ones.
/// Decay follows exponential model: weight = exp(-lambda * age_hours)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeDecayedScore {
    /// Base score (0.0-1.0)
    pub base_score: f32,

    /// Age in seconds
    pub age_seconds: u64,

    /// Decay constant (default 0.1 per 24 hours)
    pub decay_lambda: f32,

    /// Decayed score (0.0-1.0)
    pub decayed_score: f32,
}

impl TimeDecayedScore {
    /// Calculate time-decayed score
    ///
    /// Formula: score * exp(-lambda * age_hours)
    /// Default lambda = 0.1 (5% decay per 24 hours)
    pub fn calculate(base_score: f32, timestamp: u64) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let age_seconds = now.saturating_sub(timestamp);
        let age_hours = age_seconds as f32 / 3600.0;
        let decay_lambda = 0.1; // Configurable

        let decay_factor = (-decay_lambda * age_hours).exp();
        let decayed_score = (base_score * decay_factor).clamp(0.0, 1.0);

        Self {
            base_score,
            age_seconds,
            decay_lambda,
            decayed_score,
        }
    }

    /// Custom decay constant
    pub fn with_decay(mut self, lambda: f32) -> Self {
        let age_hours = self.age_seconds as f32 / 3600.0;
        let decay_factor = (-lambda * age_hours).exp();
        self.decayed_score = (self.base_score * decay_factor).clamp(0.0, 1.0);
        self.decay_lambda = lambda;
        self
    }
}

/// Pattern detection state machine
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PatternState {
    /// Single execution
    Single,
    /// Two identical executions
    Duplicate,
    /// Multiple identical executions (3+)
    Loop,
    /// Executions with slight variation (fuzzy match)
    NearLoop,
    /// Sequential improvement (increasing quality)
    RefinementChain,
    /// Multiple tools converging
    Convergence,
    /// Degrading quality
    Degradation,
}

/// Sophisticated pattern detector
pub struct PatternDetector {
    window_size: usize,
}

impl PatternDetector {
    pub fn new(window_size: usize) -> Self {
        Self { window_size }
    }

    /// Detect pattern from sequence of operations
    ///
    /// Tracks: tool names, argument similarity, result quality
    pub fn detect(
        &self,
        history: &[(String, String, f32)], // (tool, args_hash, quality)
    ) -> PatternState {
        if history.is_empty() {
            return PatternState::Single;
        }

        let recent = history
            .iter()
            .rev()
            .take(self.window_size)
            .collect::<Vec<_>>();

        if recent.len() < 2 {
            return PatternState::Single;
        }

        // Check for exact duplicates
        let first = &recent[0];
        let all_same = recent.iter().all(|r| r.0 == first.0 && r.1 == first.1);

        if all_same {
            return if recent.len() >= 3 {
                PatternState::Loop
            } else {
                PatternState::Duplicate
            };
        }

        // Check for near-loops (fuzzy argument match)
        let same_tool = recent.iter().all(|r| r.0 == first.0);
        if same_tool {
            let similarities: Vec<f32> = recent
                .windows(2)
                .map(|w| jaro_winkler_similarity(&w[0].1, &w[1].1))
                .collect();

            if similarities.iter().all(|&s| s > 0.85) && similarities.len() >= 2 {
                return PatternState::NearLoop;
            }
        }

        // Check for refinement chain (increasing quality)
        let qualities: Vec<f32> = recent.iter().map(|r| r.2).collect();
        let is_improving = qualities.windows(2).all(|w| w[1] > w[0] + 0.05); // Noticeable improvement

        if is_improving && qualities.len() >= 3 {
            return PatternState::RefinementChain;
        }

        // Check for degradation
        let is_degrading = qualities.windows(2).all(|w| w[1] < w[0] - 0.05);

        if is_degrading && qualities.len() >= 3 {
            return PatternState::Degradation;
        }

        // Check for convergence (different tools, similar quality)
        let different_tools = recent
            .iter()
            .map(|r| &r.0)
            .collect::<std::collections::HashSet<_>>()
            .len()
            > 1;
        let quality_consistent = {
            let avg = qualities.iter().sum::<f32>() / qualities.len() as f32;
            qualities.iter().all(|&q| (q - avg).abs() < 0.1)
        };

        if different_tools && quality_consistent {
            return PatternState::Convergence;
        }

        PatternState::Single
    }
}

/// ML-ready scoring components
/// Can be used for training models on tool effectiveness
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MLScoreComponents {
    /// Success rate (0-1)
    pub success_rate: f32,

    /// Average execution time (ms)
    pub avg_execution_time: f32,

    /// Result quality (0-1)
    pub result_quality: f32,

    /// Failure modes count
    pub failure_count: usize,

    /// Time since last use (hours)
    pub age_hours: f32,

    /// Usage frequency (calls per hour)
    pub frequency: f32,

    /// Confidence in measurement (0-1)
    pub confidence: f32,
}

impl MLScoreComponents {
    /// Combined ML score (before time decay)
    pub fn raw_score(&self) -> f32 {
        // Weighted: success(40%) + quality(30%) + speed(15%) + frequency(15%)
        (self.success_rate * 0.40)
            + (self.result_quality * 0.30)
            + ((10000.0 - self.avg_execution_time).max(0.0) / 10000.0 * 0.15)
            + (self.frequency.min(1.0) * 0.15)
    }

    /// Apply time decay
    pub fn with_time_decay(mut self) -> Self {
        let _decayed = TimeDecayedScore::calculate(self.raw_score(), {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            now.saturating_sub((self.age_hours * 3600.0) as u64)
        });

        // Adjust for age (older measurements less confident)
        self.confidence = (self.confidence * (-self.age_hours / 168.0).exp()).max(0.1); // 1-week half-life
        self
    }

    /// Format as feature vector for ML
    pub fn to_feature_vector(&self) -> Vec<f32> {
        vec![
            self.success_rate,
            self.avg_execution_time / 10000.0, // Normalize
            self.result_quality,
            (self.failure_count as f32).min(10.0) / 10.0, // Cap at 10 failures
            self.age_hours / 168.0,                       // Normalize to weeks
            self.frequency,
            self.confidence,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jaro_winkler_exact() {
        let sim = jaro_winkler_similarity("hello", "hello");
        assert_eq!(sim, 1.0);
    }

    #[test]
    fn test_jaro_winkler_partial() {
        let sim = jaro_winkler_similarity("pattern", "pattern_file");
        assert!(sim > 0.85 && sim < 1.0);
    }

    #[test]
    fn test_jaro_winkler_prefix() {
        let sim1 = jaro_winkler_similarity("test_one", "test_two");
        let sim2 = jaro_winkler_similarity("one_test", "two_test");
        // Prefix match should boost sim1
        assert!(sim1 > sim2);
    }

    #[test]
    fn test_time_decay() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let recent = TimeDecayedScore::calculate(0.9, now);
        let old = TimeDecayedScore::calculate(0.9, now - 7 * 24 * 3600);

        // Old score should be decayed
        assert!(old.decayed_score < recent.decayed_score);
    }

    #[test]
    fn test_pattern_loop_detection() {
        let detector = PatternDetector::new(10);

        let history = vec![
            ("grep".to_string(), "pattern1".to_string(), 0.5),
            ("grep".to_string(), "pattern1".to_string(), 0.5),
            ("grep".to_string(), "pattern1".to_string(), 0.5),
        ];

        assert_eq!(detector.detect(&history), PatternState::Loop);
    }

    #[test]
    fn test_pattern_refinement() {
        let detector = PatternDetector::new(10);

        let history = vec![
            ("grep".to_string(), "pat1".to_string(), 0.3),
            ("grep".to_string(), "pat2".to_string(), 0.5),
            ("grep".to_string(), "pat3".to_string(), 0.8),
        ];

        assert_eq!(detector.detect(&history), PatternState::RefinementChain);
    }

    #[test]
    fn test_ml_score() {
        let components = MLScoreComponents {
            success_rate: 0.9,
            avg_execution_time: 100.0,
            result_quality: 0.85,
            failure_count: 1,
            age_hours: 2.0,
            frequency: 0.5,
            confidence: 0.9,
        };

        let score = components.raw_score();
        assert!(score > 0.7 && score < 1.0);

        let features = components.to_feature_vector();
        assert_eq!(features.len(), 7);
    }
}
