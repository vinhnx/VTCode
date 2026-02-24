//! Advanced pattern detection with sequence analysis
//!
//! Tracks execution sequences, detects behavioral patterns,
//! predicts user intent based on action history.

use crate::tools::improvement_algorithms::jaro_winkler_similarity;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};

/// Single execution event
#[derive(Clone, Debug)]
pub struct ExecutionEvent {
    pub tool_name: String,
    pub arguments: String,
    pub success: bool,
    pub quality_score: f32, // 0.0-1.0
    pub duration_ms: u64,
    pub timestamp: u64,
}

/// Detected pattern in execution sequence
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DetectedPattern {
    /// Tool called once
    Single,
    /// Same tool, same args, 2x
    ExactRepeat,
    /// Same tool, same args, 3+ times (likely stuck)
    Loop,
    /// Same tool, similar args (fuzzy match >0.85)
    NearLoop,
    /// Increasing quality across iterations
    Refinement,
    /// Decreasing quality (user frustrated)
    Degradation,
    /// Different tools with consistent results
    Convergence,
    /// Tool switcing, exploring alternatives
    Exploration,
}

/// Pattern detection engine with state tracking
pub struct PatternEngine {
    max_history: usize,
    sequence_window: usize,
    events: Arc<RwLock<VecDeque<ExecutionEvent>>>,
    pattern_cache: Arc<RwLock<HashMap<String, DetectedPattern>>>,
}

impl PatternEngine {
    /// Create new pattern engine
    pub fn new(max_history: usize, sequence_window: usize) -> Self {
        Self {
            max_history,
            sequence_window,
            events: Arc::new(RwLock::new(VecDeque::with_capacity(max_history))),
            pattern_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Record execution event
    pub fn record(&self, event: ExecutionEvent) {
        let Ok(mut events) = self.events.write() else {
            return;
        };

        // Maintain max history
        if events.len() >= self.max_history {
            events.pop_front();
        }

        events.push_back(event);

        // Invalidate cache
        if let Ok(mut cache) = self.pattern_cache.write() {
            cache.clear();
        }
    }

    /// Detect overall pattern in execution history
    pub fn detect_pattern(&self) -> DetectedPattern {
        let Ok(events) = self.events.read() else {
            return DetectedPattern::Single;
        };

        if events.is_empty() {
            return DetectedPattern::Single;
        }

        if events.len() == 1 {
            return DetectedPattern::Single;
        }

        // Get recent sequence
        let recent: Vec<_> = events.iter().rev().take(self.sequence_window).collect();

        self._detect_in_sequence(recent)
    }

    /// Predict user's next likely action
    pub fn predict_next_tool(&self) -> Option<String> {
        let events = self.events.read().ok()?;

        if events.len() < 2 {
            return None;
        }

        let recent: Vec<_> = events.iter().rev().take(self.sequence_window).collect();

        // Find most common predecessor
        let last_tool = &recent[0].tool_name;

        let mut predecessors: HashMap<String, usize> = HashMap::new();

        for w in recent.windows(2) {
            if w[0].tool_name == *last_tool {
                let pred = &w[1].tool_name;
                *predecessors.entry(pred.clone()).or_insert(0) += 1;
            }
        }

        // Return most common
        predecessors
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(tool, _)| tool)
    }

    /// Get execution summary
    pub fn summary(&self) -> ExecutionSummary {
        let Ok(events) = self.events.read() else {
            return ExecutionSummary::default();
        };

        if events.is_empty() {
            return ExecutionSummary::default();
        }

        let total = events.len();
        let successful = events.iter().filter(|e| e.success).count();
        let avg_quality = events.iter().map(|e| e.quality_score).sum::<f32>() / total as f32;
        let avg_duration = events.iter().map(|e| e.duration_ms).sum::<u64>() / total as u64;

        let unique_tools: std::collections::HashSet<_> =
            events.iter().map(|e| &e.tool_name).collect();

        ExecutionSummary {
            total_executions: total,
            successful_executions: successful,
            success_rate: successful as f32 / total as f32,
            average_quality: avg_quality,
            average_duration_ms: avg_duration,
            unique_tools: unique_tools.len(),
            current_pattern: self.detect_pattern(),
        }
    }

    fn _detect_in_sequence(&self, events: Vec<&ExecutionEvent>) -> DetectedPattern {
        if events.is_empty() {
            return DetectedPattern::Single;
        }

        // Check for exact repeats
        let first = &events[0];
        let all_same_exact = events
            .iter()
            .all(|e| e.tool_name == first.tool_name && e.arguments == first.arguments);

        if all_same_exact {
            return match events.len() {
                1 | 2 => DetectedPattern::ExactRepeat,
                _ => DetectedPattern::Loop,
            };
        }

        // Check for refinement (improving quality) or degradation
        let qualities: Vec<f32> = events.iter().map(|e| e.quality_score).collect();
        if qualities.len() >= 3 {
            // events[0] is newest, so w[0] is newer than w[1]
            let is_improving = qualities.windows(2).all(|w| w[0] > w[1] + 0.05);

            if is_improving {
                return DetectedPattern::Refinement;
            }

            // Check for degradation
            let is_degrading = qualities.windows(2).all(|w| w[0] < w[1] - 0.05);

            if is_degrading {
                return DetectedPattern::Degradation;
            }
        }

        // Check for near loops (fuzzy matching)
        let same_tool = events.iter().all(|e| e.tool_name == first.tool_name);
        if same_tool && events.len() >= 3 {
            let similarities: Vec<f32> = events
                .windows(2)
                .map(|w| jaro_winkler_similarity(&w[0].arguments, &w[1].arguments))
                .collect();

            if similarities.iter().all(|&s| s > 0.85) {
                return DetectedPattern::NearLoop;
            }
        }

        // Check for convergence (different tools, similar quality)
        let different_tools: std::collections::HashSet<_> =
            events.iter().map(|e| &e.tool_name).collect();

        if different_tools.len() > 1 && events.len() >= 3 {
            let avg_quality = qualities.iter().sum::<f32>() / qualities.len() as f32;
            let quality_variance = qualities
                .iter()
                .map(|q| (q - avg_quality).abs())
                .sum::<f32>()
                / qualities.len() as f32;

            if quality_variance < 0.15 {
                return DetectedPattern::Convergence;
            }
        }

        // Check for exploration (switching between tools)
        if different_tools.len() > 1 {
            return DetectedPattern::Exploration;
        }

        DetectedPattern::Single
    }
}

/// Execution summary statistics
#[derive(Clone, Debug)]
pub struct ExecutionSummary {
    pub total_executions: usize,
    pub successful_executions: usize,
    pub success_rate: f32,
    pub average_quality: f32,
    pub average_duration_ms: u64,
    pub unique_tools: usize,
    pub current_pattern: DetectedPattern,
}

impl Default for ExecutionSummary {
    fn default() -> Self {
        Self {
            total_executions: 0,
            successful_executions: 0,
            success_rate: 0.0,
            average_quality: 0.0,
            average_duration_ms: 0,
            unique_tools: 0,
            current_pattern: DetectedPattern::Single,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_single() {
        let engine = PatternEngine::new(100, 10);

        engine.record(ExecutionEvent {
            tool_name: "grep".to_owned(),
            arguments: "pattern:test".to_owned(),
            success: true,
            quality_score: 0.8,
            duration_ms: 100,
            timestamp: 0,
        });

        assert_eq!(engine.detect_pattern(), DetectedPattern::Single);
    }

    #[test]
    fn test_pattern_loop() {
        let engine = PatternEngine::new(100, 10);

        for _ in 0..3 {
            engine.record(ExecutionEvent {
                tool_name: "grep".to_owned(),
                arguments: "pattern:test".to_owned(),
                success: true,
                quality_score: 0.5,
                duration_ms: 100,
                timestamp: 0,
            });
        }

        assert_eq!(engine.detect_pattern(), DetectedPattern::Loop);
    }

    #[test]
    fn test_pattern_refinement() {
        let engine = PatternEngine::new(100, 10);

        for (i, quality) in [0.3, 0.5, 0.8].iter().enumerate() {
            engine.record(ExecutionEvent {
                tool_name: "grep".to_owned(),
                arguments: format!("pattern:{}", i),
                success: true,
                quality_score: *quality,
                duration_ms: 100,
                timestamp: i as u64,
            });
        }

        assert_eq!(engine.detect_pattern(), DetectedPattern::Refinement);
    }

    #[test]
    fn test_pattern_degradation() {
        let engine = PatternEngine::new(100, 10);

        for (i, quality) in [0.9, 0.6, 0.2].iter().enumerate() {
            engine.record(ExecutionEvent {
                tool_name: "grep".to_owned(),
                arguments: format!("pattern:{}", i),
                success: true,
                quality_score: *quality,
                duration_ms: 100,
                timestamp: i as u64,
            });
        }

        assert_eq!(engine.detect_pattern(), DetectedPattern::Degradation);
    }

    #[test]
    fn test_predict_next_tool() {
        let engine = PatternEngine::new(100, 10);

        // Record pattern: grep -> read -> grep -> read
        let tools = vec!["grep", "read", "grep", "read"];
        for (i, tool) in tools.iter().enumerate() {
            engine.record(ExecutionEvent {
                tool_name: (*tool).to_owned(),
                arguments: "arg".to_owned(),
                success: true,
                quality_score: 0.8,
                duration_ms: 100,
                timestamp: i as u64,
            });
        }

        // Last tool is "read", so should predict "grep"
        let predicted = engine.predict_next_tool();
        assert_eq!(predicted, Some("grep".to_owned()));
    }

    #[test]
    fn test_execution_summary() {
        let engine = PatternEngine::new(100, 10);

        for i in 0..5 {
            engine.record(ExecutionEvent {
                tool_name: "grep".to_owned(),
                arguments: "arg".to_owned(),
                success: i != 2, // One failure
                quality_score: 0.8,
                duration_ms: 100,
                timestamp: i as u64,
            });
        }

        let summary = engine.summary();
        assert_eq!(summary.total_executions, 5);
        assert_eq!(summary.successful_executions, 4);
        assert_eq!(summary.success_rate, 0.8);
        assert_eq!(summary.unique_tools, 1);
    }
}
