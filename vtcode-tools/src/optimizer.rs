//! Workflow optimizer: Uses patterns and ML features to improve tool execution.
//!
//! Learns from detected patterns and suggests optimizations:
//! - Parallel execution of independent tools
//! - Caching strategy adjustments
//! - Tool sequence reordering
//! - Redundancy elimination

use crate::patterns::DetectedPattern;
use serde_json::json;
use std::collections::HashMap;

/// An optimization recommendation.
#[derive(Clone, Debug)]
pub struct Optimization {
    /// What to optimize (e.g., "parallelize")
    pub optimization_type: OptimizationType,
    /// Tools involved
    pub tools: Vec<String>,
    /// Expected improvement
    pub expected_improvement: f64,
    /// Reason for recommendation
    pub reason: String,
    /// Confidence (0-1)
    pub confidence: f64,
}

/// Type of optimization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptimizationType {
    /// Run these tools in parallel instead of sequence
    Parallelize,
    /// Cache this tool's results
    CacheResult,
    /// Skip this tool (result available elsewhere)
    SkipRedundant,
    /// Reorder tools for efficiency
    Reorder,
    /// Batch similar operations
    Batch,
}

/// Workflow optimization engine.
pub struct WorkflowOptimizer {
    /// Detected patterns to learn from
    patterns: Vec<DetectedPattern>,
    /// Feature vector for analysis
    features: Vec<f64>,
    /// Generated optimizations
    optimizations: Vec<Optimization>,
}

impl WorkflowOptimizer {
    /// Create optimizer from detector output.
    pub fn from_detector(patterns: Vec<DetectedPattern>, features: Vec<f64>) -> Self {
        let mut optimizer = Self {
            patterns,
            features,
            optimizations: Vec::new(),
        };

        optimizer.analyze();
        optimizer
    }

    /// Analyze patterns and generate optimizations.
    fn analyze(&mut self) {
        // Rule 1: High-frequency patterns can be parallelized
        self.detect_parallelization();

        // Rule 2: Repeated sequences should be cached
        self.detect_caching_opportunities();

        // Rule 3: Long patterns have redundancy
        self.detect_redundancy();

        // Rule 4: Tool diversity suggests reordering
        self.detect_reordering();

        // Sort by expected improvement (descending) - use sort_unstable for better perf
        self.optimizations.sort_unstable_by(|a, b| {
            b.expected_improvement
                .partial_cmp(&a.expected_improvement)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    /// Detect tools that can be parallelized.
    fn detect_parallelization(&mut self) {
        for pattern in &self.patterns {
            // If pattern has 2+ tools and high frequency, consider parallelizing
            if pattern.sequence.len() >= 2 && pattern.frequency >= 3 {
                // Check if tools are independent (this is a heuristic)
                let tools_are_likely_independent = !self.tools_have_dependencies(&pattern.sequence);

                if tools_are_likely_independent && pattern.success_rate > 0.95 {
                    let improvement = (pattern.frequency as f64 / 10.0).min(0.5);
                    let confidence = pattern.confidence;

                    self.optimizations.push(Optimization {
                        optimization_type: OptimizationType::Parallelize,
                        tools: pattern.sequence.clone(),
                        expected_improvement: improvement,
                        reason: format!(
                            "Tools {:?} appear {} times together with {:.0}% success rate",
                            pattern.sequence,
                            pattern.frequency,
                            pattern.success_rate * 100.0
                        ),
                        confidence,
                    });
                }
            }
        }
    }

    /// Detect opportunities for aggressive caching.
    fn detect_caching_opportunities(&mut self) {
        for pattern in &self.patterns {
            if pattern.frequency >= 5 && pattern.avg_duration_ms > 100 {
                // High frequency + slow tool = good cache candidate
                for tool in &pattern.sequence {
                    let improvement = (pattern.avg_duration_ms as f64 / 1000.0).min(0.8);

                    self.optimizations.push(Optimization {
                        optimization_type: OptimizationType::CacheResult,
                        tools: vec![tool.clone()],
                        expected_improvement: improvement,
                        reason: format!(
                            "Tool '{}' appears {} times, takes {}ms each",
                            tool, pattern.frequency, pattern.avg_duration_ms
                        ),
                        confidence: pattern.confidence * pattern.success_rate,
                    });
                }
            }
        }
    }

    /// Detect redundant tool calls.
    fn detect_redundancy(&mut self) {
        let mut tool_call_counts: HashMap<String, usize> = HashMap::new();

        for pattern in &self.patterns {
            for tool in &pattern.sequence {
                *tool_call_counts.entry(tool.clone()).or_insert(0) += 1;
            }
        }

        for (tool, count) in tool_call_counts {
            if count >= 3 {
                // Tool appears in multiple patterns - possible redundancy
                self.optimizations.push(Optimization {
                    optimization_type: OptimizationType::SkipRedundant,
                    tools: vec![tool.clone()],
                    expected_improvement: 0.2,
                    reason: format!("Tool '{}' appears in {} different patterns", tool, count),
                    confidence: 0.5,
                });
            }
        }
    }

    /// Detect suboptimal tool ordering.
    fn detect_reordering(&mut self) {
        // If pattern density is high, might benefit from reordering
        if self.features.len() > 4 && self.features[4] > 0.7 {
            // pattern_density > 70%
            self.optimizations.push(Optimization {
                optimization_type: OptimizationType::Reorder,
                tools: vec!["(workflow)".into()],
                expected_improvement: 0.15,
                reason: "High pattern density suggests workflow has repeating structure".into(),
                confidence: 0.6,
            });
        }
    }

    /// Check if tools likely have dependencies (simple heuristic).
    fn tools_have_dependencies(&self, tools: &[String]) -> bool {
        // Heuristic: grep likely depends on find, etc.
        let dependencies = [("grep_file", "find_files"), ("edit_file", "list_files")];

        for (i, tool1) in tools.iter().enumerate() {
            for (_j, tool2) in tools.iter().enumerate().skip(i + 1) {
                for (dep1, dep2) in &dependencies {
                    if (tool1.contains(dep1) && tool2.contains(dep2))
                        || (tool1.contains(dep2) && tool2.contains(dep1))
                    {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Get all optimizations.
    pub fn optimizations(&self) -> &[Optimization] {
        &self.optimizations
    }

    /// Get top-N optimizations.
    pub fn top_optimizations(&self, n: usize) -> &[Optimization] {
        &self.optimizations[..self.optimizations.len().min(n)]
    }

    /// Estimated total improvement if all optimizations applied.
    pub fn estimated_total_improvement(&self) -> f64 {
        self.optimizations
            .iter()
            .take(5) // Realistically apply top 5
            .map(|o| o.expected_improvement * o.confidence)
            .sum::<f64>()
            .min(1.0)
    }

    /// Export optimizations as JSON for analysis.
    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "total_optimizations": self.optimizations.len(),
            "estimated_improvement": self.estimated_total_improvement(),
            "optimizations": self.optimizations
                .iter()
                .take(5)
                .map(|o| json!({
                    "type": format!("{:?}", o.optimization_type),
                    "tools": o.tools,
                    "improvement": o.expected_improvement,
                    "confidence": o.confidence,
                    "reason": o.reason,
                }))
                .collect::<Vec<_>>(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patterns::DetectedPattern;

    #[test]
    fn test_optimizer_from_patterns() {
        let patterns = vec![DetectedPattern {
            name: "test".into(),
            sequence: vec!["find".into(), "grep".into()],
            frequency: 5,
            success_rate: 0.95,
            avg_duration_ms: 150,
            confidence: 0.7,
        }];

        let features = vec![5.0, 0.95, 150.0, 2.0, 0.6];
        let optimizer = WorkflowOptimizer::from_detector(patterns, features);

        assert!(!optimizer.optimizations.is_empty());
    }

    #[test]
    fn test_optimizer_improvement_estimate() {
        let patterns = vec![DetectedPattern {
            name: "test".into(),
            sequence: vec!["find".into(), "grep".into()],
            frequency: 5,
            success_rate: 0.95,
            avg_duration_ms: 150,
            confidence: 0.7,
        }];

        let features = vec![5.0, 0.95, 150.0, 2.0, 0.6];
        let optimizer = WorkflowOptimizer::from_detector(patterns, features);

        let improvement = optimizer.estimated_total_improvement();
        assert!(improvement > 0.0);
        assert!(improvement <= 1.0);
    }

    #[test]
    fn test_optimizer_recommendations() {
        let patterns = vec![DetectedPattern {
            name: "p1".into(),
            sequence: vec!["list_files".into(), "find_files".into(), "grep_file".into()],
            frequency: 10,
            success_rate: 0.98,
            avg_duration_ms: 200,
            confidence: 0.8,
        }];

        let features = vec![10.0, 0.98, 200.0, 3.0, 0.75];
        let optimizer = WorkflowOptimizer::from_detector(patterns, features);

        let top = optimizer.top_optimizations(3);
        assert!(!top.is_empty());

        // Should have recommendations
        let has_recommendation = !optimizer.optimizations.is_empty();
        assert!(has_recommendation);
    }
}
