use serde::Serialize;
use vtcode_core::core::agent::task::Task;
use vtcode_core::{RunnerTaskOutcome, RunnerTaskResults};

#[derive(Debug, Serialize)]
pub(super) struct BenchmarkReport {
    pub(super) model: String,
    pub(super) provider: String,
    pub(super) workspace: String,
    pub(super) task_count: usize,
    pub(super) tasks: Vec<BenchmarkTaskReport>,
}

#[derive(Debug, Serialize)]
pub(super) struct BenchmarkTaskReport {
    id: String,
    title: String,
    summary: String,
    modified_files: Vec<String>,
    executed_commands: Vec<String>,
    warnings: Vec<String>,
    outcome: RunnerTaskOutcome,
    turns_executed: usize,
    total_duration_ms: u128,
    average_turn_duration_ms: Option<f64>,
    max_turn_duration_ms: Option<u128>,
    turn_durations_ms: Vec<u128>,
    success: bool,
}

impl BenchmarkTaskReport {
    pub(super) fn from_task_result(task: &Task, result: RunnerTaskResults) -> Self {
        let success = matches!(
            result.outcome,
            RunnerTaskOutcome::Success | RunnerTaskOutcome::StoppedNoAction
        ) && result.warnings.is_empty();
        Self {
            id: task.id.clone(),
            title: task.title.clone(),
            summary: result.summary,
            modified_files: result.modified_files,
            executed_commands: result.executed_commands,
            warnings: result.warnings,
            outcome: result.outcome,
            turns_executed: result.turns_executed,
            total_duration_ms: result.total_duration_ms,
            average_turn_duration_ms: result.average_turn_duration_ms,
            max_turn_duration_ms: result.max_turn_duration_ms,
            turn_durations_ms: result.turn_durations_ms,
            success,
        }
    }
}
