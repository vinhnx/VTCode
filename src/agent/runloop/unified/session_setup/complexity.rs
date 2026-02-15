use tracing::debug;
use vtcode_core::ui::user_confirmation::TaskComplexity;

/// Analyze user query and log task complexity estimation.
#[allow(dead_code)]
pub(crate) fn estimate_and_log_task_complexity(query: &str) -> TaskComplexity {
    if query.is_empty() {
        return TaskComplexity::Moderate;
    }

    let lower = query.to_lowercase();
    let complexity = if query.len() > 200
        || lower.contains("refactor")
        || lower.contains("debug")
        || lower.contains("design")
        || lower.contains("architecture")
        || lower.contains("multiple")
    {
        TaskComplexity::Complex
    } else if query.len() > 100
        || lower.contains("fix")
        || lower.contains("modify")
        || lower.contains("implement")
    {
        TaskComplexity::Moderate
    } else {
        TaskComplexity::Simple
    };

    debug!("Task complexity: {:?} (estimated)", complexity);
    if lower.contains("refactor") {
        debug!("Detected: Refactoring work");
    }
    if lower.contains("debug") || lower.contains("fix") {
        debug!("Detected: Debugging/troubleshooting");
    }
    if lower.contains("multiple") {
        debug!("Detected: Multi-file changes");
    }
    if lower.contains("explain") {
        debug!("Detected: Explanation/documentation needed");
    }
    complexity
}
