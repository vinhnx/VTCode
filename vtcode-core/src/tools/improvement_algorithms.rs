//! Production-grade algorithms for tool improvements.
//!
//! Provides: Jaro-Winkler string similarity, time-decay effectiveness scoring,
//! pattern detection over tool execution history, and ML-ready feature vectors.

use crate::utils::current_timestamp;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

// ── String similarity ─────────────────────────────────────────────────────────

/// Jaro-Winkler string similarity in [0.0, 1.0].
///
/// Preferred over Levenshtein for short strings (tool arguments) because it
/// rewards matching prefixes, which is common in tool argument patterns.
pub fn jaro_winkler_similarity(s1: &str, s2: &str) -> f32 {
    if s1 == s2 {
        return 1.0;
    }
    if s1.is_empty() || s2.is_empty() {
        return 0.0;
    }

    let jaro = jaro_similarity(s1, s2);

    // Common prefix length, capped at 4 (standard Winkler constant).
    let prefix_len = s1
        .chars()
        .zip(s2.chars())
        .take_while(|(a, b)| a == b)
        .take(4)
        .count();

    // Winkler boost: p = 0.1 (standard scaling factor).
    jaro + (prefix_len as f32 * 0.1 * (1.0 - jaro))
}

/// Jaro similarity in [0.0, 1.0].
fn jaro_similarity(s1: &str, s2: &str) -> f32 {
    // Use SmallVec to avoid heap allocation for common short strings.
    let s1c: SmallVec<[char; 64]> = s1.chars().collect();
    let s2c: SmallVec<[char; 64]> = s2.chars().collect();
    let len1 = s1c.len();
    let len2 = s2c.len();

    if len1 == 0 && len2 == 0 {
        return 1.0;
    }
    if len1 == 0 || len2 == 0 {
        return 0.0;
    }

    let window = (len1.max(len2) / 2).saturating_sub(1);

    let mut s1_matched = SmallVec::<[bool; 64]>::from_elem(false, len1);
    let mut s2_matched = SmallVec::<[bool; 64]>::from_elem(false, len2);
    let mut matches = 0usize;

    for (i, &c1) in s1c.iter().enumerate() {
        let lo = i.saturating_sub(window);
        let hi = (i + window + 1).min(len2);
        for j in lo..hi {
            if !s2_matched[j] && c1 == s2c[j] {
                s1_matched[i] = true;
                s2_matched[j] = true;
                matches += 1;
                break;
            }
        }
    }

    if matches == 0 {
        return 0.0;
    }

    // Count transpositions: walk both arrays in order, zip the matched positions, count mismatches.
    let transpositions = s1c
        .iter()
        .enumerate()
        .filter(|&(i, _)| s1_matched[i])
        .zip(s2c.iter().enumerate().filter(|&(i, _)| s2_matched[i]))
        .filter(|((_, &a), (_, &b))| a != b)
        .count()
        / 2;

    let m = matches as f32;
    (m / len1 as f32 + m / len2 as f32 + (m - transpositions as f32) / m) / 3.0
}

// ── Time-decay scoring ────────────────────────────────────────────────────────

/// Time-decay effectiveness score.
///
/// Recent successes are weighted higher. Decay follows:
/// `score × exp(−λ × age_hours)`, default λ = 0.1 per 24 h.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeDecayedScore {
    /// Base score (0.0–1.0).
    pub base_score: f32,
    /// Age in seconds.
    pub age_seconds: u64,
    /// Decay constant.
    pub decay_lambda: f32,
    /// Decayed score (0.0–1.0).
    pub decayed_score: f32,
}

impl TimeDecayedScore {
    /// Calculate a time-decayed score for `base_score` recorded at `timestamp`.
    pub fn calculate(base_score: f32, timestamp: u64) -> Self {
        const DEFAULT_LAMBDA: f32 = 0.1;
        let now = current_timestamp();
        let age_seconds = now.saturating_sub(timestamp);
        let age_hours = age_seconds as f32 / 3600.0;
        let decayed_score = (base_score * (-DEFAULT_LAMBDA * age_hours).exp()).clamp(0.0, 1.0);

        Self {
            base_score,
            age_seconds,
            decay_lambda: DEFAULT_LAMBDA,
            decayed_score,
        }
    }

    /// Return a copy with a custom decay constant applied.
    pub fn with_decay(mut self, lambda: f32) -> Self {
        let age_hours = self.age_seconds as f32 / 3600.0;
        self.decayed_score = (self.base_score * (-lambda * age_hours).exp()).clamp(0.0, 1.0);
        self.decay_lambda = lambda;
        self
    }
}

// ── Pattern detection ─────────────────────────────────────────────────────────

/// Detected state in a tool-execution sequence.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PatternState {
    /// Single execution.
    Single,
    /// Two identical executions.
    Duplicate,
    /// Multiple identical executions (3+).
    Loop,
    /// Executions with slight variation (fuzzy match).
    NearLoop,
    /// Sequential quality improvement.
    RefinementChain,
    /// Multiple tools converging to similar quality.
    Convergence,
    /// Quality degrading over iterations.
    Degradation,
}

/// Detect a pattern in a history of `(tool, args_hash, quality)` triples.
///
/// Only the last `window_size` entries are examined.
/// Returns [`PatternState::Single`] for an empty or single-entry history.
///
/// # Why a free function?
/// The logic is stateless — `window_size` is the only parameter. A wrapper
/// struct added no encapsulation and hurt discoverability (KISS).
pub fn detect_pattern(
    history: &[(String, String, f32)], // (tool, args_hash, quality)
    window_size: usize,
) -> PatternState {
    if history.is_empty() {
        return PatternState::Single;
    }

    // Work on the most recent `window_size` entries; borrow the slice directly
    // (no intermediate Vec allocation).
    let start = history.len().saturating_sub(window_size);
    let recent = &history[start..];

    if recent.len() < 2 {
        return PatternState::Single;
    }

    let first = &recent[0];

    // --- Exact duplicates ---
    if recent.iter().all(|r| r.0 == first.0 && r.1 == first.1) {
        return if recent.len() >= 3 {
            PatternState::Loop
        } else {
            PatternState::Duplicate
        };
    }

    // --- Single combined scan -------------------------------------------------
    // Collect qualities + same-tool flag + multi-tool flag in one pass.
    let mut qualities = SmallVec::<[f32; 32]>::with_capacity(recent.len());
    let mut same_tool = true;
    let mut multi_tool = false;

    for r in recent {
        qualities.push(r.2);
        if r.0 != first.0 {
            same_tool = false;
            multi_tool = true;
        }
    }

    // --- Quality trends (≥3 points) ---
    if qualities.len() >= 3 {
        if qualities.windows(2).all(|w| w[1] > w[0] + 0.05) {
            return PatternState::RefinementChain;
        }
        if qualities.windows(2).all(|w| w[1] < w[0] - 0.05) {
            return PatternState::Degradation;
        }
    }

    // --- Near-loop: same tool, fuzzy args ---
    // Use `.all()` directly — no intermediate Vec<f32> needed.
    if same_tool {
        if recent
            .windows(2)
            .all(|w| jaro_winkler_similarity(&w[0].1, &w[1].1) > 0.85)
        {
            return PatternState::NearLoop;
        }
    }

    // --- Convergence: different tools, similar quality ---
    if multi_tool {
        let n = qualities.len() as f32;
        let avg = qualities.iter().sum::<f32>() / n;
        if qualities.iter().all(|&q| (q - avg).abs() < 0.1) {
            return PatternState::Convergence;
        }
    }

    PatternState::Single
}

// ── ML scoring ────────────────────────────────────────────────────────────────

/// ML-ready scoring components for tool effectiveness.
///
/// Can be used as a feature vector for training models.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MLScoreComponents {
    /// Success rate (0–1).
    pub success_rate: f32,
    /// Average execution time (ms).
    pub avg_execution_time: f32,
    /// Result quality (0–1).
    pub result_quality: f32,
    /// Number of failure modes observed.
    pub failure_count: usize,
    /// Time since last use (hours).
    pub age_hours: f32,
    /// Usage frequency (calls per hour).
    pub frequency: f32,
    /// Confidence in measurement (0–1).
    pub confidence: f32,
}

impl MLScoreComponents {
    /// Combined ML score before time decay.
    ///
    /// Weights: success 40% + quality 30% + speed 15% + frequency 15%.
    pub fn raw_score(&self) -> f32 {
        (self.success_rate * 0.40)
            + (self.result_quality * 0.30)
            + ((10_000.0 - self.avg_execution_time).max(0.0) / 10_000.0 * 0.15)
            + (self.frequency.min(1.0) * 0.15)
    }

    /// Apply confidence decay for older measurements (1-week half-life).
    pub fn with_age_decay(mut self) -> Self {
        // Older measurements are less reliable; decay confidence over time.
        self.confidence = (self.confidence * (-self.age_hours / 168.0).exp()).max(0.1);
        self
    }

    /// Return a 7-element feature vector, normalised for ML consumption.
    pub fn to_feature_vector(&self) -> [f32; 7] {
        [
            self.success_rate,
            self.avg_execution_time / 10_000.0,
            self.result_quality,
            (self.failure_count as f32).min(10.0) / 10.0,
            self.age_hours / 168.0,
            self.frequency,
            self.confidence,
        ]
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jaro_winkler_exact() {
        assert_eq!(jaro_winkler_similarity("hello", "hello"), 1.0);
    }

    #[test]
    fn test_jaro_winkler_partial() {
        let sim = jaro_winkler_similarity("pattern", "pattern_file");
        assert!(sim > 0.85 && sim < 1.0, "sim={sim}");
    }

    #[test]
    fn test_jaro_winkler_prefix_boost() {
        let with_prefix = jaro_winkler_similarity("test_one", "test_two");
        let without = jaro_winkler_similarity("one_test", "two_test");
        assert!(with_prefix > without, "prefix boost should be applied");
    }

    #[test]
    fn test_time_decay_ordering() {
        let now = current_timestamp();
        let recent = TimeDecayedScore::calculate(0.9, now);
        let old = TimeDecayedScore::calculate(0.9, now.saturating_sub(7 * 24 * 3600));
        assert!(old.decayed_score < recent.decayed_score);
    }

    #[test]
    fn test_detect_pattern_loop() {
        let history = vec![
            ("grep".to_string(), "pattern1".to_string(), 0.5),
            ("grep".to_string(), "pattern1".to_string(), 0.5),
            ("grep".to_string(), "pattern1".to_string(), 0.5),
        ];
        assert_eq!(detect_pattern(&history, 10), PatternState::Loop);
    }

    #[test]
    fn test_detect_pattern_refinement() {
        let history = vec![
            ("grep".to_string(), "pat1".to_string(), 0.3),
            ("grep".to_string(), "pat2".to_string(), 0.5),
            ("grep".to_string(), "pat3".to_string(), 0.8),
        ];
        assert_eq!(detect_pattern(&history, 10), PatternState::RefinementChain);
    }

    #[test]
    fn test_ml_raw_score() {
        let c = MLScoreComponents {
            success_rate: 0.9,
            avg_execution_time: 100.0,
            result_quality: 0.85,
            failure_count: 1,
            age_hours: 2.0,
            frequency: 0.5,
            confidence: 0.9,
        };
        let score = c.raw_score();
        assert!(score > 0.7 && score < 1.0, "score={score}");
    }

    #[test]
    fn test_ml_feature_vector_length() {
        let c = MLScoreComponents {
            success_rate: 0.9,
            avg_execution_time: 100.0,
            result_quality: 0.85,
            failure_count: 1,
            age_hours: 2.0,
            frequency: 0.5,
            confidence: 0.9,
        };
        assert_eq!(c.to_feature_vector().len(), 7);
    }
}
