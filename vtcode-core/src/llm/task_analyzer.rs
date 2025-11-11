/// Task complexity analyzer for intelligent model selection
///
/// Analyzes user queries to estimate task complexity, enabling optimal model selection
/// and providing insights into the nature of the requested work.

use crate::llm::TaskComplexity;

#[derive(Debug, Clone)]
pub struct TaskAnalysis {
    /// Estimated complexity of the task
    pub complexity: TaskComplexity,
    /// Confidence score 0-100 for this estimate
    pub confidence: u8,
    /// Reasoning for the complexity estimate
    pub reasoning: String,
    /// Detected aspects of the task
    pub aspects: TaskAspects,
}

#[derive(Debug, Clone, Default)]
pub struct TaskAspects {
    /// Whether the task involves code refactoring
    pub has_refactoring: bool,
    /// Whether the task requires architectural decisions
    pub has_design_decisions: bool,
    /// Whether the task requires debugging or troubleshooting
    pub has_debugging: bool,
    /// Whether the task involves multiple files/components
    pub is_multi_file: bool,
    /// Whether the task requires research or exploration
    pub has_exploration: bool,
    /// Number of distinct tool calls likely needed
    pub estimated_tool_calls: u8,
    /// Whether reasoning/explanation is required
    pub needs_explanation: bool,
}

impl TaskAnalysis {
    pub fn new(
        complexity: TaskComplexity,
        confidence: u8,
        reasoning: String,
        aspects: TaskAspects,
    ) -> Self {
        Self {
            complexity,
            confidence: confidence.clamp(0, 100),
            reasoning,
            aspects,
        }
    }
}

pub struct TaskAnalyzer;

impl TaskAnalyzer {
    /// Analyze a user query to estimate task complexity
    pub fn analyze_query(query: &str) -> TaskAnalysis {
        if query.is_empty() {
            return TaskAnalysis::new(
                TaskComplexity::Simple,
                30,
                "Empty query, assuming simple task".to_string(),
                TaskAspects::default(),
            );
        }

        let lower = query.to_lowercase();

        // Detect refactoring tasks
        let has_refactoring = matches!(
            lower.contains("refactor") || lower.contains("rename") 
            || lower.contains("reorganize") || lower.contains("restructure"),
            true
        );

        // Detect design/architecture decisions
        let has_design_decisions = lower.contains("design") || lower.contains("architecture")
            || lower.contains("pattern") || lower.contains("structure");

        // Detect debugging/troubleshooting
        let has_debugging = lower.contains("debug") || lower.contains("fix")
            || lower.contains("error") || lower.contains("broken")
            || lower.contains("issue") || lower.contains("bug");

        // Detect multi-file work
        let is_multi_file = lower.contains("multiple") || lower.contains("across")
            || lower.contains("module") || lower.contains("component")
            || lower.contains("package");

        // Detect exploration/research tasks
        let has_exploration = lower.contains("explore") || lower.contains("research")
            || lower.contains("investigate") || lower.contains("understand")
            || lower.contains("analyze");

        // Detect explanation-heavy tasks
        let needs_explanation = lower.contains("explain") || lower.contains("how")
            || lower.contains("why") || lower.contains("document");

        // Estimate tool calls based on query length and keywords
        let estimated_tool_calls = Self::estimate_tool_calls(&lower);

        // Calculate complexity based on detected aspects
        let (complexity, confidence, reasoning) =
            Self::determine_complexity(has_refactoring, has_design_decisions, has_debugging,
                is_multi_file, has_exploration, needs_explanation, estimated_tool_calls, query.len());

        let aspects = TaskAspects {
            has_refactoring,
            has_design_decisions,
            has_debugging,
            is_multi_file,
            has_exploration,
            estimated_tool_calls,
            needs_explanation,
        };

        TaskAnalysis::new(complexity, confidence, reasoning, aspects)
    }

    /// Estimate the number of tool calls likely needed
    fn estimate_tool_calls(query: &str) -> u8 {
        let mut count = 1u8; // Minimum 1 tool call

        // Add for each major operation type
        if query.contains("find") || query.contains("search") {
            count += 1;
        }
        if query.contains("modify") || query.contains("change") || query.contains("update") {
            count += 1;
        }
        if query.contains("create") || query.contains("add") || query.contains("new") {
            count += 1;
        }
        if query.contains("test") || query.contains("verify") {
            count += 1;
        }
        if query.contains("review") || query.contains("check") {
            count += 1;
        }

        count.clamp(1, 8)
    }

    /// Determine complexity level based on task aspects
    fn determine_complexity(
        has_refactoring: bool,
        has_design_decisions: bool,
        has_debugging: bool,
        is_multi_file: bool,
        has_exploration: bool,
        needs_explanation: bool,
        estimated_tool_calls: u8,
        query_length: usize,
    ) -> (TaskComplexity, u8, String) {
        let mut aspect_count = 0;
        let mut reasons = Vec::new();

        if has_refactoring {
            aspect_count += 1;
            reasons.push("refactoring work");
        }
        if has_design_decisions {
            aspect_count += 2;
            reasons.push("design decisions");
        }
        if has_debugging {
            aspect_count += 2;
            reasons.push("debugging");
        }
        if is_multi_file {
            aspect_count += 1;
            reasons.push("multi-file changes");
        }
        if has_exploration {
            aspect_count += 1;
            reasons.push("exploration");
        }
        if needs_explanation {
            aspect_count += 1;
            reasons.push("explanation");
        }

        // Longer queries tend to be more complex
        let length_score = (query_length as f64 / 100.0).min(2.0) as u8;
        aspect_count += length_score;

        // Tool call estimate affects complexity
        if estimated_tool_calls >= 5 {
            aspect_count += 1;
            reasons.push("multiple tool calls");
        }

        let (complexity, confidence) = match aspect_count {
            0..=1 => (TaskComplexity::Simple, 75),
            2..=3 => (TaskComplexity::Standard, 80),
            4..=5 => (TaskComplexity::Complex, 70),
            _ => (TaskComplexity::Expert, 60),
        };

        let reasoning = if reasons.is_empty() {
            "No specific complexity indicators detected".to_string()
        } else {
            format!("Detected: {}", reasons.join(", "))
        };

        (complexity, confidence, reasoning)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_query_analysis() {
        let analysis = TaskAnalyzer::analyze_query("create a function");
        assert_eq!(analysis.complexity, TaskComplexity::Simple);
        assert!(analysis.confidence > 50);
    }

    #[test]
    fn test_refactoring_query_analysis() {
        let analysis = TaskAnalyzer::analyze_query("refactor the authentication module");
        assert!(analysis.aspects.has_refactoring);
        assert_eq!(analysis.complexity, TaskComplexity::Standard);
    }

    #[test]
    fn test_complex_query_analysis() {
        let analysis = TaskAnalyzer::analyze_query(
            "refactor and redesign the database schema across multiple modules with new patterns",
        );
        assert!(analysis.aspects.has_refactoring);
        assert!(analysis.aspects.has_design_decisions);
        assert!(analysis.aspects.is_multi_file);
        assert!(matches!(
            analysis.complexity,
            TaskComplexity::Complex | TaskComplexity::Expert
        ));
    }

    #[test]
    fn test_debugging_query_analysis() {
        let analysis = TaskAnalyzer::analyze_query("fix the memory leak bug in the cache");
        assert!(analysis.aspects.has_debugging);
    }

    #[test]
    fn test_empty_query_analysis() {
        let analysis = TaskAnalyzer::analyze_query("");
        assert_eq!(analysis.complexity, TaskComplexity::Simple);
        assert!(analysis.confidence <= 50);
    }

    #[test]
    fn test_tool_call_estimation() {
        let analysis = TaskAnalyzer::analyze_query("find and modify and test the code");
        assert!(analysis.aspects.estimated_tool_calls >= 3);
    }

    #[test]
    fn test_exploration_query() {
        let analysis = TaskAnalyzer::analyze_query("explore and understand how the system works");
        assert!(analysis.aspects.has_exploration);
        assert!(analysis.aspects.needs_explanation);
    }

    #[test]
    fn test_multi_file_detection() {
        let analysis = TaskAnalyzer::analyze_query("update this across multiple modules");
        assert!(analysis.aspects.is_multi_file);
    }

    #[test]
    fn test_explanation_detection() {
        let analysis = TaskAnalyzer::analyze_query("explain how this feature works");
        assert!(analysis.aspects.needs_explanation);
    }

    #[test]
    fn test_confidence_scoring() {
        let simple = TaskAnalyzer::analyze_query("print hello");
        let complex =
            TaskAnalyzer::analyze_query("design and implement a distributed system architecture");
        // Complex tasks have lower confidence due to more variables
        assert!(simple.confidence >= complex.confidence);
    }
}
