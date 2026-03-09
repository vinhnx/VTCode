use std::time::Duration;

use hashbrown::HashMap;

use crate::agent::runloop::git::{FileStat, git_working_tree_numstat_snapshot};

const CODE_CHANGE_SNAPSHOT_TIMEOUT_MS: u64 = 120;

pub(super) struct TurnExecutionMetrics {
    pub attempts_made: usize,
    pub retry_count: usize,
    pub history_snapshot_bytes: usize,
    pub timeout_secs: u64,
    pub elapsed_ms: u128,
    pub outcome: &'static str,
}

pub(super) fn estimate_history_bytes(history: &[vtcode_core::llm::provider::Message]) -> usize {
    use vtcode_core::llm::provider::{ContentPart, MessageContent};

    history
        .iter()
        .map(|message| {
            let content_bytes = match &message.content {
                MessageContent::Text(text) => text.len(),
                MessageContent::Parts(parts) => parts
                    .iter()
                    .map(|part| match part {
                        ContentPart::Text { text } => text.len(),
                        ContentPart::Image { .. } | ContentPart::File { .. } => 128,
                    })
                    .sum::<usize>(),
            };
            let reasoning_bytes = message.reasoning.as_ref().map_or(0, String::len);
            let tool_call_id_bytes = message.tool_call_id.as_ref().map_or(0, String::len);
            let origin_tool_bytes = message.origin_tool.as_ref().map_or(0, String::len);
            content_bytes + reasoning_bytes + tool_call_id_bytes + origin_tool_bytes + 32
        })
        .sum()
}

pub(super) fn emit_turn_execution_metrics(turn_metrics: TurnExecutionMetrics) {
    tracing::info!(
        target: "vtcode.turn.metrics",
        metric = "turn_execution",
        attempts_made = turn_metrics.attempts_made,
        retry_count = turn_metrics.retry_count,
        history_snapshot_bytes = turn_metrics.history_snapshot_bytes,
        timeout_secs = turn_metrics.timeout_secs,
        elapsed_ms = turn_metrics.elapsed_ms,
        outcome = turn_metrics.outcome,
        "turn metric"
    );
}

pub(super) async fn capture_code_change_snapshot(
    workspace: &std::path::Path,
    phase: &str,
) -> Option<HashMap<std::path::PathBuf, FileStat>> {
    let workspace_path = workspace.to_path_buf();
    let phase_label = phase.to_string();
    match tokio::time::timeout(
        Duration::from_millis(CODE_CHANGE_SNAPSHOT_TIMEOUT_MS),
        tokio::task::spawn_blocking(move || git_working_tree_numstat_snapshot(&workspace_path)),
    )
    .await
    {
        Ok(Ok(Ok(snapshot))) => snapshot,
        Ok(Ok(Err(err))) => {
            tracing::warn!(
                "Failed to capture {} code-change snapshot: {}",
                phase_label,
                err
            );
            None
        }
        Ok(Err(err)) => {
            tracing::warn!(
                "Failed to capture {} code-change snapshot (join error): {}",
                phase_label,
                err
            );
            None
        }
        Err(_) => {
            tracing::debug!(
                "Skipping {} code-change snapshot after {}ms timeout",
                phase_label,
                CODE_CHANGE_SNAPSHOT_TIMEOUT_MS
            );
            None
        }
    }
}
