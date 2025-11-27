//! Pattern detection with sequence analysis and ML feature engineering.
//!
//! Analyzes tool call sequences to detect patterns, anomalies, and trends.

use serde_json::{Value, json};
use std::collections::HashMap;

/// A single tool call event.
#[derive(Clone, Debug)]
pub struct ToolEvent {
    pub tool_name: String,
    pub success: bool,
    pub duration_ms: u64,
    pub timestamp: std::time::Instant,
}

/// A detected pattern in tool call sequences.
#[derive(Clone, Debug)]
pub struct DetectedPattern {
    pub name: String,
    pub sequence: Vec<String>,
    pub frequency: usize,
    pub success_rate: f64,
    pub avg_duration_ms: u64,
    pub confidence: f64,
}

/// Pattern detector using sequence analysis.
pub struct PatternDetector {
    events: Vec<ToolEvent>,
    patterns: HashMap<String, DetectedPattern>,
    sequence_length: usize,
}

impl PatternDetector {
    /// Create new detector with sliding window size.
    pub fn new(sequence_length: usize) -> Self {
        Self {
            events: Vec::new(),
            patterns: HashMap::new(),
            sequence_length,
        }
    }

    /// Add an event to the detector.
    pub fn record_event(&mut self, event: ToolEvent) {
        self.events.push(event);
        self.analyze();
    }

    /// Analyze events for patterns.
    fn analyze(&mut self) {
        if self.events.len() < self.sequence_length {
            return;
        }

        let mut sequence_map: HashMap<Vec<String>, Vec<&ToolEvent>> = HashMap::new();

        // Slide window and extract sequences.
        for i in 0..=(self.events.len() - self.sequence_length) {
            let window = &self.events[i..i + self.sequence_length];
            let seq: Vec<String> = window.iter().map(|e| e.tool_name.clone()).collect();

            // Reserve or get the vector once, then push window events into it.
            let entry = sequence_map.entry(seq.clone()).or_insert_with(Vec::new);
            for event in window {
                entry.push(event);
            }
        }

        // Extract patterns with metrics.
        for (sequence, events) in sequence_map {
            if events.len() >= 2 {
                // Pattern appears at least twice.
                let success_count = events.iter().filter(|e| e.success).count();
                let success_rate = success_count as f64 / events.len() as f64;
                let avg_duration =
                    events.iter().map(|e| e.duration_ms).sum::<u64>() / events.len() as u64;
                let frequency = events.len();

                // Confidence: based on frequency and consistency.
                let confidence = (success_rate * (frequency as f64 / 10.0).min(1.0)).min(1.0);

                let pattern_name = format!("pattern_{:x}", hash_sequence(&sequence));

                self.patterns.insert(
                    pattern_name.clone(),
                    DetectedPattern {
                        name: pattern_name,
                        sequence,
                        frequency,
                        success_rate,
                        avg_duration_ms: avg_duration,
                        confidence,
                    },
                );
            }
        }
    }

    /// Get detected patterns.
    pub fn patterns(&self) -> Vec<DetectedPattern> {
        let mut patterns: Vec<_> = self.patterns.values().cloned().collect();
        patterns.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
        patterns
    }

    /// Extract normalized feature vector for ML.
    pub fn feature_vector(&self) -> Vec<f64> {
        let mut features = Vec::new();

        // Feature 1: Event count.
        features.push(self.events.len() as f64);

        // Feature 2: Success rate.
        let success_rate = self.events.iter().filter(|e| e.success).count() as f64
            / self.events.len().max(1) as f64;
        features.push(success_rate);

        // Feature 3: Average duration.
        let avg_duration = self.events.iter().map(|e| e.duration_ms).sum::<u64>() as f64
            / self.events.len().max(1) as f64;
        features.push(avg_duration);

        // Feature 4: Tool diversity (unique tools).
        let unique_tools = self
            .events
            .iter()
            .map(|e| &e.tool_name)
            .collect::<std::collections::HashSet<_>>()
            .len() as f64;
        features.push(unique_tools);

        // Feature 5: Pattern density (detected patterns / possible patterns).
        let pattern_density = self.patterns.len() as f64 / self.events.len().max(1) as f64;
        features.push(pattern_density);

        // Normalize to [0, 1] range.
        normalize_features(&features)
    }

    /// Clear all data.
    pub fn reset(&mut self) {
        self.events.clear();
        self.patterns.clear();
    }

    /// Export patterns as JSON for analysis.
    pub fn to_json(&self) -> Value {
        json!({
            "event_count": self.events.len(),
            "pattern_count": self.patterns.len(),
            "patterns": self.patterns()
                .iter()
                .map(|p| json!({
                    "name": p.name,
                    "sequence": p.sequence,
                    "frequency": p.frequency,
                    "success_rate": p.success_rate,
                    "avg_duration_ms": p.avg_duration_ms,
                    "confidence": p.confidence,
                }))
                .collect::<Vec<_>>(),
            "feature_vector": self.feature_vector(),
        })
    }
}

/// Normalize features to [0, 1] range (except feature 0 which stays as-is).
fn normalize_features(features: &[f64]) -> Vec<f64> {
    features
        .iter()
        .enumerate()
        .map(|(i, &f)| {
            if i == 0 {
                f // Keep event count as-is
            } else {
                f.min(1.0).max(0.0) // Clamp to [0, 1]
            }
        })
        .collect()
}

/// Quick hash for sequence.
fn hash_sequence(seq: &[String]) -> u64 {
    let mut hash: u64 = 0;
    for s in seq {
        for b in s.bytes() {
            hash = hash.wrapping_mul(31).wrapping_add(b as u64);
        }
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn test_pattern_detection() {
        let mut detector = PatternDetector::new(2);

        let now = Instant::now();

        // Record a repeating pattern: (A, B) repeats 3 times.
        detector.record_event(ToolEvent {
            tool_name: "tool_a".into(),
            success: true,
            duration_ms: 100,
            timestamp: now,
        });
        detector.record_event(ToolEvent {
            tool_name: "tool_b".into(),
            success: true,
            duration_ms: 50,
            timestamp: now,
        });
        detector.record_event(ToolEvent {
            tool_name: "tool_a".into(),
            success: true,
            duration_ms: 100,
            timestamp: now,
        });
        detector.record_event(ToolEvent {
            tool_name: "tool_b".into(),
            success: true,
            duration_ms: 50,
            timestamp: now,
        });

        let patterns = detector.patterns();
        assert!(!patterns.is_empty());
        assert!(patterns[0].sequence.len() == 2);
    }

    #[test]
    fn test_feature_vector() {
        let mut detector = PatternDetector::new(2);
        let now = Instant::now();

        for i in 0..5 {
            detector.record_event(ToolEvent {
                tool_name: format!("tool_{}", i % 2),
                success: i % 2 == 0,
                duration_ms: 50 + i as u64 * 10,
                timestamp: now,
            });
        }

        let features = detector.feature_vector();
        assert_eq!(features.len(), 5);
        assert!(features.iter().all(|f| *f >= 0.0));
    }

    #[test]
    fn test_success_rate() {
        let mut detector = PatternDetector::new(2);
        let now = Instant::now();

        detector.record_event(ToolEvent {
            tool_name: "tool_a".into(),
            success: true,
            duration_ms: 100,
            timestamp: now,
        });
        detector.record_event(ToolEvent {
            tool_name: "tool_b".into(),
            success: false,
            duration_ms: 50,
            timestamp: now,
        });
        detector.record_event(ToolEvent {
            tool_name: "tool_a".into(),
            success: true,
            duration_ms: 100,
            timestamp: now,
        });

        let features = detector.feature_vector();
        // Feature 1 is success rate.
        assert!(features[1] > 0.0 && features[1] < 1.0);
    }
}
