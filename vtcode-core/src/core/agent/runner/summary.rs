use super::AgentRunner;
use crate::core::agent::task::{Task, TaskOutcome};
use crate::gemini::Content;

impl AgentRunner {
    /// Generate a meaningful summary of the task execution
    #[allow(clippy::too_many_arguments)]
    pub(super) fn generate_task_summary(
        &self,
        task: &Task,
        modified_files: &[String],
        executed_commands: &[String],
        warnings: &[String],
        conversation: &[Content],
        turns_executed: usize,
        peak_tool_loops: usize,
        max_tool_loops: usize,
        outcome: TaskOutcome,
        total_duration_ms: u128,
        average_turn_duration_ms: Option<f64>,
        max_turn_duration_ms: Option<u128>,
    ) -> String {
        let mut summary = Vec::new();

        summary.push(format!("Task: {}", task.title));
        if !task.description.trim().is_empty() {
            summary.push(format!("Description: {}", task.description.trim()));
        }
        summary.push(format!("Agent Type: {:?}", self.agent_type));
        summary.push(format!("Session: {}", self.session_id));

        let reasoning_label = self
            .reasoning_effort
            .map(|effort| effort.to_string())
            .unwrap_or_else(|| "default".to_owned());

        summary.push(format!(
            "Model: {} (provider: {}, reasoning: {})",
            self.client.model_id(),
            self.provider_client.name(),
            reasoning_label
        ));

        let tool_loops_used = peak_tool_loops;
        summary.push(format!(
            "Turns: {} used / {} max | Tool loops: {} used / {} max",
            turns_executed, self.max_turns, tool_loops_used, max_tool_loops
        ));

        let mut duration_line = format!("Duration: {} ms", total_duration_ms);
        let mut duration_metrics = Vec::new();
        if let Some(avg) = average_turn_duration_ms {
            duration_metrics.push(format!("avg {:.1} ms/turn", avg));
        }
        if let Some(max_turn) = max_turn_duration_ms {
            duration_metrics.push(format!("max {} ms", max_turn));
        }
        if !duration_metrics.is_empty() {
            duration_line.push_str(" (");
            duration_line.push_str(&duration_metrics.join(", "));
            duration_line.push(')');
        }
        summary.push(duration_line);

        let mut resolved_outcome = outcome;
        if matches!(resolved_outcome, TaskOutcome::Unknown)
            && conversation.last().is_some_and(|c| {
                c.role == "model"
                    && c.parts.iter().any(|p| {
                        p.as_text().is_some_and(|t| {
                            t.contains("completed") || t.contains("done") || t.contains("finished")
                        })
                    })
            })
        {
            resolved_outcome = TaskOutcome::Success;
        }

        let mut status_line = format!("Final Status: {}", resolved_outcome.description());
        if !warnings.is_empty() && resolved_outcome.is_success() {
            status_line.push_str(" (with warnings)");
        }
        summary.push(status_line);
        summary.push(format!("Outcome Code: {}", resolved_outcome.code()));

        if !executed_commands.is_empty() {
            summary.push("Executed Commands:".to_owned());
            for command in executed_commands {
                summary.push(format!(" - {}", command));
            }
        }

        if !modified_files.is_empty() {
            summary.push("Modified Files:".to_owned());
            for file in modified_files {
                summary.push(format!(" - {}", file));
            }
        }

        if !warnings.is_empty() {
            summary.push("Warnings:".to_owned());
            for warning in warnings {
                summary.push(format!(" - {}", warning));
            }
        }

        summary.join("\n")
    }
}
