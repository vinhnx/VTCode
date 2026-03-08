//! Advanced pattern detection with sequence analysis
//!
//! Tracks execution sequences, detects behavioral patterns,
//! predicts user intent based on action history.

use crate::tools::improvement_algorithms::jaro_winkler_similarity;
use hashbrown::HashMap;
use parking_lot::RwLock;
use smallvec::SmallVec;
use std::collections::VecDeque;
use std::sync::Arc;

/// Single execution event.
#[derive(Clone, Debug)]
pub struct ExecutionEvent {
    pub tool_name: String,
    pub arguments: String,
    pub success: bool,
    pub quality_score: f32, // 0.0-1.0
    pub duration_ms: u64,
    pub timestamp: u64,
}

/// Detected pattern in execution sequence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DetectedPattern {
    /// Tool called once.
    Single,
    /// Same tool, same args, 2x.
    ExactRepeat,
    /// Same tool, same args, 3+ times (likely stuck).
    Loop,
    /// Same tool, similar args (fuzzy match >0.85).
    NearLoop,
    /// Increasing quality across iterations.
    Refinement,
    /// Decreasing quality (user frustrated).
    Degradation,
    /// Different tools with consistent results.
    Convergence,
    /// Tool switching, exploring alternatives.
    Exploration,
}

/// Pattern detection engine with state tracking.
pub struct PatternEngine {
    max_history: usize,
    sequence_window: usize,
    events: Arc<RwLock<VecDeque<ExecutionEvent>>>,
}

impl PatternEngine {
    /// Create a new pattern engine.
    pub fn new(max_history: usize, sequence_window: usize) -> Self {
        Self {
            max_history,
            sequence_window,
            events: Arc::new(RwLock::new(VecDeque::with_capacity(max_history))),
        }
    }

    /// Record an execution event.
    pub fn record(&self, event: ExecutionEvent) {
        let mut events = self.events.write();

        if events.len() >= self.max_history {
            events.pop_front();
        }

        events.push_back(event);
    }

    /// Detect overall pattern in execution history.
    pub fn detect_pattern(&self) -> DetectedPattern {
        let events = self.events.read();

        let len = events.len();
        if len < 2 {
            return DetectedPattern::Single;
        }

        // Use SmallVec to avoid heap allocation for the recent window.
        // sequence_window is typically 20.
        let mut recent = SmallVec::<[&ExecutionEvent; 32]>::new();
        recent.extend(events.iter().rev().take(self.sequence_window));

        detect_in_sequence(&recent)
    }

    /// Predict the user's next likely tool based on predecessor frequency.
    pub fn predict_next_tool(&self) -> Option<String> {
        let events = self.events.read();

        let len = events.len();
        if len < 2 {
            return None;
        }

        // We only need the tool names for prediction.
        let mut recent_tools = SmallVec::<[&str; 32]>::new();
        recent_tools.extend(
            events
                .iter()
                .rev()
                .take(self.sequence_window)
                .map(|e| e.tool_name.as_str()),
        );

        if recent_tools.is_empty() {
            return None;
        }

        let last_tool = recent_tools[0];
        let mut predecessors = HashMap::<&str, usize>::new();

        for w in recent_tools.windows(2) {
            if w[0] == last_tool {
                *predecessors.entry(w[1]).or_default() += 1;
            }
        }

        predecessors
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(tool, _)| tool.to_owned())
    }

    /// Compute an execution summary in a single pass over the event history.
    ///
    /// Multiple separate `.iter()` passes each pay the traversal cost again, which
    /// prevents cache-friendly pipelining (turbopuffer "zero-cost" lesson). One
    /// combined loop is clearer and faster.
    pub fn summary(&self) -> ExecutionSummary {
        let events = self.events.read();
        if events.is_empty() {
            return ExecutionSummary::default();
        }

        let total = events.len();
        let mut successful = 0usize;
        let mut quality_sum = 0.0f32;
        let mut duration_sum = 0u64;
        // `HashSet` capacity = total is an upper bound; most runs use ≤ a handful of tools.
        let mut unique_tools: hashbrown::HashSet<&str> =
            hashbrown::HashSet::with_capacity(total.min(16));

        for e in events.iter() {
            if e.success {
                successful += 1;
            }
            quality_sum += e.quality_score;
            duration_sum += e.duration_ms;
            unique_tools.insert(e.tool_name.as_str());
        }

        ExecutionSummary {
            total_executions: total,
            successful_executions: successful,
            success_rate: successful as f32 / total as f32,
            average_quality: quality_sum / total as f32,
            average_duration_ms: duration_sum / total as u64,
            unique_tools: unique_tools.len(),
            current_pattern: self.detect_pattern_with_guard(&events),
        }
    }

    /// Internal helper to detect patterns given a read guard.
    fn detect_pattern_with_guard(&self, events: &VecDeque<ExecutionEvent>) -> DetectedPattern {
        let len = events.len();
        if len < 2 {
            return DetectedPattern::Single;
        }

        let mut recent = SmallVec::<[&ExecutionEvent; 32]>::new();
        recent.extend(events.iter().rev().take(self.sequence_window));

        detect_in_sequence(&recent)
    }
}

/// Core pattern classification over a reversed-chronological event slice.
///
/// Extracted from `PatternEngine` so it can be tested without the `Arc<RwLock<…>>`
/// wrapper and called without `&self` (it uses no engine state).
///
/// `events[0]` is the **newest** event.
fn detect_in_sequence(events: &[&ExecutionEvent]) -> DetectedPattern {
    if events.is_empty() {
        return DetectedPattern::Single;
    }

    let first = events[0];

    // --- Exact repeat check (one short-circuit pass) ---
    if events
        .iter()
        .all(|e| e.tool_name == first.tool_name && e.arguments == first.arguments)
    {
        return match events.len() {
            1 | 2 => DetectedPattern::ExactRepeat,
            _ => DetectedPattern::Loop,
        };
    }

    // --- Single combined scan: qualities + same_tool flag + seen_second_tool ---
    //
    // We need:
    //   • quality_score for each event  → Vec<f32>  (required for .windows(2))
    //   • whether all events share the first tool   → bool
    //   • whether >1 distinct tool exists           → bool (early-exit once true)
    //
    // A single `for` loop gathers all three cheaply and keeps the data hot in cache.
    let mut qualities = SmallVec::<[f32; 32]>::with_capacity(events.len());
    let mut same_tool = true;
    let mut multi_tool = false;

    for e in events {
        qualities.push(e.quality_score);
        if e.tool_name != first.tool_name {
            same_tool = false;
            multi_tool = true; // keep looping to fill `qualities`
        }
    }

    // --- Quality trend (needs ≥3 points) ---
    if qualities.len() >= 3 {
        // events[0] is newest → qualities[0] is the latest score.
        if qualities.windows(2).all(|w| w[0] > w[1] + 0.05) {
            return DetectedPattern::Refinement;
        }
        if qualities.windows(2).all(|w| w[0] < w[1] - 0.05) {
            return DetectedPattern::Degradation;
        }
    }

    // --- Near-loop: same tool, fuzzy-similar args ---
    if same_tool && events.len() >= 3 {
        if events
            .windows(2)
            .all(|w| jaro_winkler_similarity(&w[0].arguments, &w[1].arguments) > 0.85)
        {
            return DetectedPattern::NearLoop;
        }
    }

    // --- Convergence / Exploration: multiple tools ---
    if multi_tool && events.len() >= 3 {
        // Single-pass mean + mean-absolute-deviation over qualities.
        let n = qualities.len() as f32;
        let avg = qualities.iter().sum::<f32>() / n;
        let mad = qualities.iter().map(|q| (q - avg).abs()).sum::<f32>() / n;

        if mad < 0.15 {
            return DetectedPattern::Convergence;
        }
    }

    if multi_tool {
        return DetectedPattern::Exploration;
    }

    DetectedPattern::Single
}

/// Execution summary statistics.
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

    fn make_event(tool: &str, args: &str, quality: f32) -> ExecutionEvent {
        ExecutionEvent {
            tool_name: tool.to_owned(),
            arguments: args.to_owned(),
            success: true,
            quality_score: quality,
            duration_ms: 100,
            timestamp: 0,
        }
    }

    #[test]
    fn test_pattern_single() {
        let engine = PatternEngine::new(100, 10);
        engine.record(make_event("grep", "pattern:test", 0.8));
        assert_eq!(engine.detect_pattern(), DetectedPattern::Single);
    }

    #[test]
    fn test_pattern_loop() {
        let engine = PatternEngine::new(100, 10);
        for _ in 0..3 {
            engine.record(make_event("grep", "pattern:test", 0.5));
        }
        assert_eq!(engine.detect_pattern(), DetectedPattern::Loop);
    }

    #[test]
    fn test_pattern_refinement() {
        let engine = PatternEngine::new(100, 10);
        for (i, &quality) in [0.3f32, 0.5, 0.8].iter().enumerate() {
            engine.record(ExecutionEvent {
                tool_name: "grep".to_owned(),
                arguments: format!("pattern:{i}"),
                success: true,
                quality_score: quality,
                duration_ms: 100,
                timestamp: i as u64,
            });
        }
        assert_eq!(engine.detect_pattern(), DetectedPattern::Refinement);
    }

    #[test]
    fn test_pattern_degradation() {
        let engine = PatternEngine::new(100, 10);
        for (i, &quality) in [0.9f32, 0.6, 0.2].iter().enumerate() {
            engine.record(ExecutionEvent {
                tool_name: "grep".to_owned(),
                arguments: format!("pattern:{i}"),
                success: true,
                quality_score: quality,
                duration_ms: 100,
                timestamp: i as u64,
            });
        }
        assert_eq!(engine.detect_pattern(), DetectedPattern::Degradation);
    }

    #[test]
    fn test_predict_next_tool() {
        let engine = PatternEngine::new(100, 10);
        for (i, &tool) in ["grep", "read", "grep", "read"].iter().enumerate() {
            engine.record(ExecutionEvent {
                tool_name: tool.to_owned(),
                arguments: "arg".to_owned(),
                success: true,
                quality_score: 0.8,
                duration_ms: 100,
                timestamp: i as u64,
            });
        }
        assert_eq!(engine.predict_next_tool(), Some("grep".to_owned()));
    }

    #[test]
    fn test_execution_summary() {
        let engine = PatternEngine::new(100, 10);
        for i in 0..5u64 {
            engine.record(ExecutionEvent {
                tool_name: "grep".to_owned(),
                arguments: "arg".to_owned(),
                success: i != 2,
                quality_score: 0.8,
                duration_ms: 100,
                timestamp: i,
            });
        }

        let summary = engine.summary();
        assert_eq!(summary.total_executions, 5);
        assert_eq!(summary.successful_executions, 4);
        assert_eq!(summary.success_rate, 0.8);
        assert_eq!(summary.unique_tools, 1);
    }
}
