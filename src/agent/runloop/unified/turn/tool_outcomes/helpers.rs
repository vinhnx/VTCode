use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
use rustc_hash::FxHashMap;
use std::time::Instant;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::names::canonical_tool_name;

pub(crate) const EXIT_PLAN_MODE_REASON_AUTO_TRIGGER_ON_DENIAL: &str = "auto_trigger_on_plan_denial";
pub(crate) const EXIT_PLAN_MODE_REASON_USER_REQUESTED_IMPLEMENTATION: &str =
    "user_requested_implementation";

/// Threshold: number of consecutive file mutations before the Anti-Blind-Editing
/// warning fires. NL2Repo-Bench recommends verifying after every few edits.
pub(crate) const BLIND_EDITING_THRESHOLD: usize = 4;

/// Threshold: number of consecutive read/search operations before the Navigation
/// Loop warning fires.
pub(crate) const NAVIGATION_LOOP_THRESHOLD: usize = 15;

/// Optimized loop detection with bounded signature keys and exponential backoff.
pub(crate) struct LoopTracker {
    attempts: FxHashMap<String, (usize, Instant)>,
    low_signal_attempts: FxHashMap<String, (usize, Instant)>,
    /// Counter for consecutive mutating file operations without execution/verification
    pub consecutive_mutations: usize,
    /// Counter for consecutive read/search operations without action or synthesis
    pub consecutive_navigations: usize,
}

impl LoopTracker {
    pub(crate) fn new() -> Self {
        Self {
            attempts: FxHashMap::with_capacity_and_hasher(16, Default::default()),
            low_signal_attempts: FxHashMap::with_capacity_and_hasher(8, Default::default()),
            consecutive_mutations: 0,
            consecutive_navigations: 0,
        }
    }

    /// Record an attempt and return the count
    pub(crate) fn record(&mut self, signature: String) -> usize {
        let entry = self
            .attempts
            .entry(signature)
            .or_insert((0, Instant::now()));
        entry.0 += 1;
        entry.1 = Instant::now();
        entry.0
    }

    fn record_low_signal(&mut self, signature: String) -> usize {
        let entry = self
            .low_signal_attempts
            .entry(signature)
            .or_insert((0, Instant::now()));
        entry.0 += 1;
        entry.1 = Instant::now();
        entry.0
    }

    /// Get the maximum repetition count, optionally filtering by a predicate on the signature
    pub(crate) fn max_count_filtered<F>(&self, exclude: F) -> usize
    where
        F: Fn(&str) -> bool,
    {
        self.attempts
            .iter()
            .filter_map(
                |(sig, (count, _))| {
                    if exclude(sig) { None } else { Some(*count) }
                },
            )
            .max()
            .unwrap_or(0)
    }

    pub(crate) fn max_low_signal_count(&self) -> usize {
        self.low_signal_attempts
            .values()
            .map(|(count, _)| *count)
            .max()
            .unwrap_or(0)
    }

    fn reset_low_signal_attempts(&mut self) {
        self.low_signal_attempts.clear();
    }

    pub(crate) fn reset_after_balancer_recovery(&mut self) {
        self.attempts.clear();
        self.low_signal_attempts.clear();
        self.consecutive_mutations = 0;
        self.consecutive_navigations = 0;
    }
}

fn output_has_empty_search_matches(output: &serde_json::Value) -> bool {
    output
        .get("matches")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|matches| matches.is_empty())
}

fn output_reuses_recent_result(output: &serde_json::Value) -> bool {
    [
        "loop_detected",
        "reused_recent_result",
        "spool_ref_only",
        "result_ref_only",
    ]
    .iter()
    .any(|key| output.get(*key).and_then(serde_json::Value::as_bool) == Some(true))
}

fn looks_like_grep_style_command(command: &str) -> bool {
    let lower = command.trim().to_ascii_lowercase();
    lower.starts_with("grep ")
        || lower.starts_with("rg ")
        || lower.contains("/grep ")
        || lower.contains("/rg ")
}

fn output_is_grep_style_miss(output: &serde_json::Value, command_success: bool) -> bool {
    if command_success {
        return false;
    }

    let exit_code = output.get("exit_code").and_then(serde_json::Value::as_i64);
    let command = output
        .get("command")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let stdout_empty = output
        .get("stdout")
        .or_else(|| output.get("output"))
        .and_then(serde_json::Value::as_str)
        .is_none_or(|text| text.trim().is_empty());

    stdout_empty && matches!(exit_code, Some(1 | 2)) && looks_like_grep_style_command(command)
}

fn error_is_missing_resource(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    [
        "not found",
        "no such file",
        "resource not found",
        "spool file not found",
        "session output file not found",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn is_low_signal_outcome(outcome: &ToolPipelineOutcome, canonical_tool_name: &str) -> bool {
    match &outcome.status {
        ToolExecutionStatus::Success {
            output,
            command_success,
            ..
        } => {
            output_has_empty_search_matches(output)
                || output_reuses_recent_result(output)
                || (canonical_tool_name == vtcode_core::config::constants::tools::UNIFIED_EXEC
                    && output_is_grep_style_miss(output, *command_success))
        }
        ToolExecutionStatus::Failure { error } => error_is_missing_resource(&error.to_string()),
        ToolExecutionStatus::Timeout { .. } | ToolExecutionStatus::Cancelled => false,
    }
}

pub(crate) fn push_tool_response<S>(
    history: &mut Vec<uni::Message>,
    tool_call_id: S,
    content: String,
) where
    S: AsRef<str> + Into<String>,
{
    let tool_call_id_ref = tool_call_id.as_ref();
    if let Some(existing) = history
        .iter_mut()
        .rev()
        .find(|message| message.tool_call_id.as_deref() == Some(tool_call_id_ref))
    {
        existing.content = uni::MessageContent::Text(content);
        return;
    }
    history.push(uni::Message::tool_response(tool_call_id.into(), content));
}

pub(crate) fn push_invalid_tool_args_response<S>(
    history: &mut Vec<uni::Message>,
    tool_call_id: S,
    tool_name: &str,
    error: &str,
) where
    S: AsRef<str> + Into<String>,
{
    let payload = serde_json::json!({
        "error": format!("Invalid tool arguments for '{}': {}", tool_name, error)
    });
    push_tool_response(history, tool_call_id, payload.to_string());
}

pub(crate) fn build_exit_plan_mode_args(reason: &str) -> serde_json::Value {
    serde_json::json!({
        "reason": reason
    })
}

pub(crate) fn build_exit_plan_mode_call_id(prefix: &str, suffix: u128) -> String {
    format!("{prefix}_{suffix}")
}

pub(crate) fn build_step_exit_plan_mode_call_id(step_count: usize) -> String {
    format!("call_{step_count}_exit_plan_mode")
}

/// Generate a tool signature key with predictable structure for loop tracking.
pub(crate) fn signature_key_for(name: &str, args: &serde_json::Value) -> String {
    // Keep keys compact on hot paths: hash bounded argument bytes instead of
    // allocating full JSON payloads for large tool arguments.
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut input_len = 0usize;
    let mutability_tag =
        if vtcode_core::tools::tool_intent::classify_tool_intent(name, args).mutating {
            "rw"
        } else {
            "ro"
        };

    if serde_json::to_writer(HashingWriter::new(&mut hash, &mut input_len), args).is_err() {
        for byte in b"{}" {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
            input_len = input_len.saturating_add(1);
        }
    }

    format!("{name}:{mutability_tag}:len{input_len}-fnv{hash:016x}")
}

struct HashingWriter<'a> {
    hash: &'a mut u64,
    input_len: &'a mut usize,
}

impl<'a> HashingWriter<'a> {
    fn new(hash: &'a mut u64, input_len: &'a mut usize) -> Self {
        Self { hash, input_len }
    }
}

impl std::io::Write for HashingWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        for byte in buf {
            *self.hash ^= u64::from(*byte);
            *self.hash = self.hash.wrapping_mul(0x100000001b3);
            *self.input_len = self.input_len.saturating_add(1);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(crate) fn resolve_max_tool_retries(
    _tool_name: &str,
    vt_cfg: Option<&vtcode_core::config::loader::VTCodeConfig>,
) -> usize {
    vt_cfg
        .map(|cfg| cfg.agent.harness.max_tool_retries as usize)
        .unwrap_or(vtcode_config::constants::defaults::DEFAULT_MAX_TOOL_RETRIES as usize)
}

fn path_targets_plan_artifact(path: &str) -> bool {
    let normalized = path.trim().replace('\\', "/");
    normalized == ".vtcode/plans"
        || normalized.starts_with(".vtcode/plans/")
        || normalized.contains("/.vtcode/plans/")
        || normalized == "/tmp/vtcode-plans"
        || normalized.starts_with("/tmp/vtcode-plans/")
        || normalized.contains("/tmp/vtcode-plans/")
}

fn is_plan_artifact_write(name: &str, args: &serde_json::Value) -> bool {
    use vtcode_core::config::constants::tools as tool_names;
    use vtcode_core::tools::names::canonical_tool_name;
    use vtcode_core::tools::tool_intent::unified_file_action;

    let canonical = canonical_tool_name(name);
    match canonical.as_ref() {
        tool_names::PLAN_TASK_TRACKER | tool_names::TASK_TRACKER => true,
        tool_names::UNIFIED_FILE => {
            if !unified_file_action(args)
                .map(|action| action.eq_ignore_ascii_case("read"))
                .unwrap_or(false)
            {
                [
                    "path",
                    "file_path",
                    "filepath",
                    "filePath",
                    "target_path",
                    "destination",
                    "destination_path",
                ]
                .iter()
                .filter_map(|key| args.get(*key).and_then(|value| value.as_str()))
                .any(path_targets_plan_artifact)
            } else {
                false
            }
        }
        tool_names::WRITE_FILE
        | tool_names::EDIT_FILE
        | tool_names::CREATE_FILE
        | tool_names::SEARCH_REPLACE => ["path", "file_path", "filepath", "filePath"]
            .iter()
            .filter_map(|key| args.get(*key).and_then(|value| value.as_str()))
            .any(path_targets_plan_artifact),
        _ => false,
    }
}

/// Updates the tool repetition tracker based on the execution outcome.
///
/// Count every completed attempt except user-triggered cancellations so the turn
/// balancer can stop low-signal retry loops even when tools keep failing.
pub(crate) fn update_repetition_tracker(
    loop_tracker: &mut LoopTracker,
    outcome: &ToolPipelineOutcome,
    name: &str,
    args: &serde_json::Value,
) {
    if matches!(&outcome.status, ToolExecutionStatus::Cancelled) {
        return;
    }

    let canonical_name = canonical_tool_name(name);
    let canonical_name = canonical_name.as_ref();
    let signature_key = signature_key_for(canonical_name, args);
    loop_tracker.record(signature_key);
    let low_signal_family =
        crate::agent::runloop::unified::turn::tool_outcomes::handlers::low_signal_family_key(
            canonical_name,
            args,
        )
        .filter(|_| is_low_signal_outcome(outcome, canonical_name));
    if let Some(low_signal_family) = low_signal_family.as_ref() {
        loop_tracker.record_low_signal(low_signal_family.clone());
    }

    // Update NL2Repo-Bench metrics based on tool intent.
    //
    // IMPORTANT: Check execution tools FIRST. `classify_tool_intent` marks
    // `unified_exec(action=run)` as `mutating: true` because shell commands *can*
    // mutate state, but for the Edit-Test heuristic, any execution/verification
    // step (cargo check, cargo test, etc.) should RESET the mutation counter,
    // not increment it.
    use vtcode_core::config::constants::tools as tool_names;

    let is_execution_tool = matches!(
        canonical_name,
        n if n == tool_names::UNIFIED_EXEC
            || n == tool_names::RUN_PTY_CMD
            || n == tool_names::EXECUTE_CODE
            || n == "shell"
    );

    if is_execution_tool {
        // Execution/verification step resets both counters
        loop_tracker.consecutive_mutations = 0;
        loop_tracker.consecutive_navigations = 0;
        if low_signal_family.is_none() {
            loop_tracker.reset_low_signal_attempts();
        }
    } else if is_plan_artifact_write(canonical_name, args) {
        // Plan artifact writes in dedicated plan storage are allowed in Plan Mode and
        // should not trigger anti-blind-editing verification pressure.
        loop_tracker.consecutive_navigations = 0;
    } else {
        let intent = vtcode_core::tools::tool_intent::classify_tool_intent(canonical_name, args);
        if intent.mutating {
            loop_tracker.consecutive_mutations += 1;
            loop_tracker.consecutive_navigations = 0;
            if low_signal_family.is_none() {
                loop_tracker.reset_low_signal_attempts();
            }
        } else {
            // Read-only / navigation tool
            loop_tracker.consecutive_navigations += 1;
        }
    }
}
pub(crate) fn serialize_output(output: &serde_json::Value) -> String {
    if let Some(s) = output.as_str() {
        s.to_string()
    } else {
        serde_json::to_string(output).unwrap_or_else(|_| "{}".to_string())
    }
}

pub(crate) fn check_is_argument_error(error_str: &str) -> bool {
    error_str.contains("Missing required")
        || error_str.contains("Invalid arguments")
        || error_str.contains("Tool argument validation failed")
        || error_str.contains("required path parameter")
        || error_str.contains("is required for '")
        || error_str.contains("is required for \"")
        || error_str.contains("'index' is required")
        || error_str.contains("'index_path' is required")
        || error_str.contains("'status' is required")
        || error_str.contains("expected ")
        || error_str.contains("Expected:")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn push_tool_response_replaces_existing_tool_call_entry() {
        let mut history = vec![uni::Message::tool_response(
            "call_1".to_string(),
            "{\"output\":\"first\"}".to_string(),
        )];

        push_tool_response(
            &mut history,
            "call_1".to_string(),
            "{\"output\":\"latest\"}".to_string(),
        );

        assert_eq!(history.len(), 1);
        assert_eq!(
            history[0].content.as_text_borrowed(),
            Some("{\"output\":\"latest\"}")
        );
    }

    #[test]
    fn repetition_tracker_counts_failures() {
        let mut tracker = LoopTracker::new();
        let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
            error: anyhow::anyhow!("boom"),
        });

        update_repetition_tracker(
            &mut tracker,
            &outcome,
            "edit_file",
            &json!({"path":"src/main.rs"}),
        );

        assert_eq!(tracker.max_count_filtered(|_| false), 1);
    }

    #[test]
    fn repetition_tracker_ignores_cancellations() {
        let mut tracker = LoopTracker::new();
        let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Cancelled);

        update_repetition_tracker(
            &mut tracker,
            &outcome,
            "edit_file",
            &json!({"path":"src/main.rs"}),
        );

        assert_eq!(tracker.max_count_filtered(|_| false), 0);
    }

    #[test]
    fn reset_after_balancer_recovery_clears_attempts_and_counters() {
        let mut tracker = LoopTracker::new();
        tracker.record("unified_search:{\"action\":\"grep\"}".to_string());
        tracker.record("unified_search:{\"action\":\"grep\"}".to_string());
        tracker.consecutive_mutations = 2;
        tracker.consecutive_navigations = 4;
        tracker.record_low_signal("unified_search::grep::src".to_string());

        tracker.reset_after_balancer_recovery();

        assert_eq!(tracker.max_count_filtered(|_| false), 0);
        assert_eq!(tracker.max_low_signal_count(), 0);
        assert_eq!(tracker.consecutive_mutations, 0);
        assert_eq!(tracker.consecutive_navigations, 0);
    }

    #[test]
    fn consecutive_mutations_increments_on_edit() {
        let mut tracker = LoopTracker::new();
        let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: serde_json::json!({}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });

        // edit_file is classified as mutating
        update_repetition_tracker(
            &mut tracker,
            &success,
            "edit_file",
            &json!({"path":"src/lib.rs","old_str":"a","new_str":"b"}),
        );
        assert_eq!(tracker.consecutive_mutations, 1);
        assert_eq!(tracker.consecutive_navigations, 0);

        update_repetition_tracker(
            &mut tracker,
            &success,
            "write_to_file",
            &json!({"path":"src/lib.rs","content":"x"}),
        );
        assert_eq!(tracker.consecutive_mutations, 2);
    }

    #[test]
    fn execution_tool_resets_mutation_counter() {
        let mut tracker = LoopTracker::new();
        let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: serde_json::json!({}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });

        // Two mutations
        update_repetition_tracker(
            &mut tracker,
            &success,
            "edit_file",
            &json!({"path":"a","old_str":"x","new_str":"y"}),
        );
        update_repetition_tracker(
            &mut tracker,
            &success,
            "edit_file",
            &json!({"path":"b","old_str":"x","new_str":"y"}),
        );
        assert_eq!(tracker.consecutive_mutations, 2);

        // Execution tool resets
        update_repetition_tracker(
            &mut tracker,
            &success,
            vtcode_core::config::constants::tools::UNIFIED_EXEC,
            &json!({"action":"run","command":"cargo check"}),
        );
        assert_eq!(tracker.consecutive_mutations, 0);
        assert_eq!(tracker.consecutive_navigations, 0);
    }

    #[test]
    fn reads_increment_navigation_counter() {
        let mut tracker = LoopTracker::new();
        let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: serde_json::json!({}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });

        update_repetition_tracker(
            &mut tracker,
            &success,
            vtcode_core::config::constants::tools::READ_FILE,
            &json!({"path":"src/main.rs"}),
        );
        assert_eq!(tracker.consecutive_navigations, 1);
        assert_eq!(tracker.consecutive_mutations, 0);

        update_repetition_tracker(
            &mut tracker,
            &success,
            "grep_file",
            &json!({"pattern":"foo","path":"src/"}),
        );
        assert_eq!(tracker.consecutive_navigations, 2);
    }

    #[test]
    fn mutation_resets_navigation_counter() {
        let mut tracker = LoopTracker::new();
        let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: serde_json::json!({}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });

        // Several reads
        for _ in 0..5 {
            update_repetition_tracker(
                &mut tracker,
                &success,
                vtcode_core::config::constants::tools::READ_FILE,
                &json!({"path":"src/main.rs"}),
            );
        }
        assert_eq!(tracker.consecutive_navigations, 5);

        // A mutation resets navigation counter
        update_repetition_tracker(
            &mut tracker,
            &success,
            "edit_file",
            &json!({"path":"src/lib.rs","old_str":"a","new_str":"b"}),
        );
        assert_eq!(tracker.consecutive_navigations, 0);
        assert_eq!(tracker.consecutive_mutations, 1);
    }

    #[test]
    fn plan_task_tracker_does_not_increment_mutations() {
        let mut tracker = LoopTracker::new();
        let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: serde_json::json!({}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });

        update_repetition_tracker(
            &mut tracker,
            &success,
            vtcode_core::config::constants::tools::PLAN_TASK_TRACKER,
            &json!({"action":"create","items":["step"]}),
        );
        assert_eq!(tracker.consecutive_mutations, 0);
        assert_eq!(tracker.consecutive_navigations, 0);
    }

    #[test]
    fn task_tracker_does_not_increment_mutations() {
        let mut tracker = LoopTracker::new();
        let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: serde_json::json!({}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });

        update_repetition_tracker(
            &mut tracker,
            &success,
            vtcode_core::config::constants::tools::TASK_TRACKER,
            &json!({"action":"create","items":["step"]}),
        );
        assert_eq!(tracker.consecutive_mutations, 0);
        assert_eq!(tracker.consecutive_navigations, 0);
    }

    #[test]
    fn plan_file_write_does_not_increment_mutations() {
        let mut tracker = LoopTracker::new();
        let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: serde_json::json!({}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });

        update_repetition_tracker(
            &mut tracker,
            &success,
            vtcode_core::config::constants::tools::UNIFIED_FILE,
            &json!({"action":"write","path":".vtcode/plans/my-plan.md","content":"text"}),
        );
        assert_eq!(tracker.consecutive_mutations, 0);
        assert_eq!(tracker.consecutive_navigations, 0);
    }

    #[test]
    fn non_plan_file_write_still_increments_mutations() {
        let mut tracker = LoopTracker::new();
        let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: serde_json::json!({}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });

        update_repetition_tracker(
            &mut tracker,
            &success,
            vtcode_core::config::constants::tools::UNIFIED_FILE,
            &json!({"action":"write","path":"src/lib.rs","content":"text"}),
        );
        assert_eq!(tracker.consecutive_mutations, 1);
        assert_eq!(tracker.consecutive_navigations, 0);
    }

    #[test]
    fn argument_error_detection_includes_required_update_fields() {
        assert!(check_is_argument_error(
            "Tool execution failed: 'index' is required for 'update' (1-indexed)"
        ));
    }

    #[test]
    fn low_signal_tracker_groups_empty_search_results_by_family() {
        let mut tracker = LoopTracker::new();
        let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: serde_json::json!({"matches":[]}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });

        update_repetition_tracker(
            &mut tracker,
            &miss,
            vtcode_core::config::constants::tools::UNIFIED_SEARCH,
            &json!({"action":"structural","pattern":"fn $name(...)", "lang":"rust", "globs":["vtcode-tui/**/*.rs"]}),
        );
        update_repetition_tracker(
            &mut tracker,
            &miss,
            vtcode_core::config::constants::tools::UNIFIED_SEARCH,
            &json!({"action":"grep","pattern":"-> Result","path":"vtcode-tui","globs":["vtcode-tui/**/*.rs"]}),
        );
        update_repetition_tracker(
            &mut tracker,
            &miss,
            vtcode_core::config::constants::tools::UNIFIED_SEARCH,
            &json!({"action":"grep","pattern":"Result<","path":"vtcode-tui","globs":["vtcode-tui/**/*.rs"]}),
        );

        assert_eq!(tracker.max_low_signal_count(), 3);
    }

    #[test]
    fn low_signal_tracker_counts_missing_read_failures() {
        let mut tracker = LoopTracker::new();
        let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
            error: anyhow::anyhow!("Resource not found: vtcode-tui/src/main.rs"),
        });

        update_repetition_tracker(
            &mut tracker,
            &miss,
            vtcode_core::config::constants::tools::UNIFIED_FILE,
            &json!({"action":"read","path":"vtcode-tui/src/main.rs"}),
        );
        update_repetition_tracker(
            &mut tracker,
            &miss,
            vtcode_core::config::constants::tools::UNIFIED_FILE,
            &json!({"action":"read","path":"vtcode-tui/src/main.rs","offset":40}),
        );

        assert_eq!(tracker.max_low_signal_count(), 2);
    }

    #[test]
    fn low_signal_tracker_counts_grep_style_shell_misses() {
        let mut tracker = LoopTracker::new();
        let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: serde_json::json!({
                "command": "grep -n '-> Result' vtcode-tui/src/**/*.rs",
                "exit_code": 2,
                "output": ""
            }),
            stdout: None,
            modified_files: vec![],
            command_success: false,
        });

        update_repetition_tracker(
            &mut tracker,
            &miss,
            vtcode_core::config::constants::tools::UNIFIED_EXEC,
            &json!({"action":"run","command":"grep -n '-> Result' vtcode-tui/src/**/*.rs"}),
        );
        update_repetition_tracker(
            &mut tracker,
            &miss,
            vtcode_core::config::constants::tools::UNIFIED_EXEC,
            &json!({"action":"run","command":"grep -n \"-> Result\" vtcode-tui/src/**/*.rs"}),
        );

        assert_eq!(tracker.max_low_signal_count(), 2);
    }
}
