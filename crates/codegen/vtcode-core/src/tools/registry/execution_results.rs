//! Output preparation and read-cache invalidation after completed execution.

use serde_json::{Value, json};

use crate::config::constants::tools;
use crate::tools::tool_intent;

use super::{ToolRegistry, normalize_tool_output};

pub(super) struct ExecutionOutput {
    pub(super) normalized_value: Value,
    pub(super) structured_error: Option<String>,
}

impl ToolRegistry {
    /// Prepare handler output without changing breaker, history, or middleware state.
    pub(super) async fn prepare_execution_output(
        &self,
        tool_name: &str,
        args: &Value,
        value: Value,
        is_mcp: bool,
        max_output_tokens: usize,
    ) -> ExecutionOutput {
        if !is_mcp && crate::core::agent::completion::tracker_adoption_succeeded(tool_name, args, &value) {
            self.harness_context
                .tracker_adopted
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        // Dynamic context discovery: spool large outputs to files
        let mut value = value;
        if tool_intent::is_spool_file_read_command(tool_name, args) {
            if let Some(output) = value.as_object_mut() {
                output.insert("no_spool".to_string(), json!(true));
            } else {
                // `process_tool_output` can spool scalar/array values
                // based on their serialized size. Wrap an unusual
                // scalar result before that boundary so a safe spool
                // inspection can never create a nested spool reference.
                value = json!({"output": value, "no_spool": true});
            }
        }
        let processed_value = self.process_tool_output(tool_name, value, is_mcp, max_output_tokens).await;
        let mut normalized_value = normalize_tool_output(processed_value);
        if tool_name == tools::CODE_SEARCH
            && let Some(output) = normalized_value.as_object_mut()
        {
            output.remove("success");
        }
        let structured_error = structured_tool_output_error(&normalized_value);

        ExecutionOutput { normalized_value, structured_error }
    }

    /// Invalidate read records for a completed call already classified as mutating.
    pub(super) fn invalidate_mutated_reads(&self, tool_name: &str, args: &Value) {
        // Invalidate only the cache records whose read target could
        // overlap the mutated file(s).  Wiping the entire history
        // (previous behavior) defeated cross-turn dedup: any write
        // tool call would discard unrelated read-only cache hits,
        // forcing the model to re-read files whose contents hadn't
        // changed at all.
        let targets = mutated_target_paths(tool_name, args);
        if targets.is_empty() && is_pathless_mutating_command(tool_name) {
            // A mutating shell command with no identifiable target
            // (e.g. `sed -i`, `cargo build`) could have touched any
            // file. Conservatively drop every cached read so no
            // record serves stale content.
            self.execution_history.invalidate_all_reads();
        } else {
            for target in targets {
                self.execution_history.invalidate_for_path(&target);
            }
        }
    }
}

/// Extract the file paths a non-readonly tool call is about to mutate.
///
/// Returns an empty Vec for tool calls that aren't path-mutating (in which
/// case no cache invalidation is needed). Delegates to the canonical
/// `apply_patch::mutation_target_paths`, which covers singular `path`
/// fields, `destination`/`destination_path` (move/copy targets), and
/// `items`/`paths`/`files` arrays.
fn mutated_target_paths(tool_name: &str, args: &Value) -> Vec<String> {
    crate::tools::apply_patch::mutation_target_paths(tool_name, args)
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

/// Returns `true` for shell/command tools that can mutate files without
/// exposing a target path in their arguments (e.g. `sed -i`, `cargo build`,
/// `make`). When such a command is classified as mutating we cannot know which
/// files changed, so cached reads must be invalidated conservatively.
fn is_pathless_mutating_command(tool_name: &str) -> bool {
    matches!(tool_name, tools::UNIFIED_EXEC | tools::EXEC_COMMAND | tools::EXEC_PTY_CMD | tools::WRITE_STDIN)
}

fn structured_tool_output_error(value: &Value) -> Option<String> {
    let obj = value.as_object()?;
    if obj.get("success").and_then(Value::as_bool) == Some(false) {
        return obj
            .get("error")
            .map(tool_error_value_to_string)
            .or_else(|| Some("tool reported success=false".to_string()));
    }

    obj.get("error").map(tool_error_value_to_string)
}

fn tool_error_value_to_string(value: &Value) -> String {
    if let Some(message) = value.as_str() {
        return message.to_string();
    }
    if let Some(message) = value.get("message").and_then(Value::as_str) {
        return message.to_string();
    }
    value.to_string()
}

#[cfg(test)]
mod tests;
