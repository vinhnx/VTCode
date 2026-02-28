use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
use rustc_hash::FxHashMap;
use std::time::{Duration, Instant};
use vtcode_core::llm::provider as uni;

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
    #[allow(dead_code)]
    backoff_base: Duration,
    /// Counter for consecutive mutating file operations without execution/verification
    pub consecutive_mutations: usize,
    /// Counter for consecutive read/search operations without action or synthesis
    pub consecutive_navigations: usize,
}

impl LoopTracker {
    pub(crate) fn new() -> Self {
        Self {
            attempts: FxHashMap::with_capacity_and_hasher(16, Default::default()),
            backoff_base: Duration::from_secs(5),
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

    /// Check if a warning should be emitted (with exponential backoff)
    #[allow(dead_code)]
    pub(crate) fn should_warn(&self, signature: &str, threshold: usize) -> bool {
        if let Some((count, last_time)) = self.attempts.get(signature) {
            if *count < threshold {
                return false;
            }
            let excess = count.saturating_sub(threshold);
            let backoff = self.backoff_base * 3u32.pow(excess.min(5) as u32);
            last_time.elapsed() >= backoff
        } else {
            false
        }
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

    pub(crate) fn reset_after_balancer_recovery(&mut self) {
        self.attempts.clear();
        self.consecutive_mutations = 0;
        self.consecutive_navigations = 0;
    }
}

pub(crate) fn push_tool_response(
    history: &mut Vec<uni::Message>,
    tool_call_id: String,
    content: String,
) {
    history.push(uni::Message::tool_response(tool_call_id, content));
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
    let args_str = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
    let mut key = String::with_capacity(name.len() + args_str.len() + 1);
    key.push_str(name);
    key.push(':');
    key.push_str(&args_str);
    key
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
}

fn is_plan_artifact_write(name: &str, args: &serde_json::Value) -> bool {
    use vtcode_core::config::constants::tools as tool_names;
    use vtcode_core::tools::names::canonical_tool_name;
    use vtcode_core::tools::tool_intent::unified_file_action;

    let canonical = canonical_tool_name(name);
    match canonical.as_ref() {
        tool_names::PLAN_TASK_TRACKER => true,
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

    let signature_key = signature_key_for(name, args);
    loop_tracker.record(signature_key);

    // Update NL2Repo-Bench metrics based on tool intent.
    //
    // IMPORTANT: Check execution tools FIRST. `classify_tool_intent` marks
    // `unified_exec(action=run)` as `mutating: true` because shell commands *can*
    // mutate state, but for the Edit-Test heuristic, any execution/verification
    // step (cargo check, cargo test, etc.) should RESET the mutation counter,
    // not increment it.
    use vtcode_core::config::constants::tools as tool_names;

    let is_execution_tool = matches!(
        name,
        n if n == tool_names::UNIFIED_EXEC
            || n == tool_names::RUN_PTY_CMD
            || n == tool_names::EXECUTE_CODE
            || n == tool_names::SHELL
    );

    if is_execution_tool {
        // Execution/verification step resets both counters
        loop_tracker.consecutive_mutations = 0;
        loop_tracker.consecutive_navigations = 0;
    } else if is_plan_artifact_write(name, args) {
        // Plan artifact writes in .vtcode/plans are allowed in Plan Mode and
        // should not trigger anti-blind-editing verification pressure.
        loop_tracker.consecutive_navigations = 0;
    } else {
        let intent = vtcode_core::tools::tool_intent::classify_tool_intent(name, args);
        if intent.mutating {
            loop_tracker.consecutive_mutations += 1;
            loop_tracker.consecutive_navigations = 0;
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

        tracker.reset_after_balancer_recovery();

        assert_eq!(tracker.max_count_filtered(|_| false), 0);
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
            has_more: false,
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
            has_more: false,
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
            has_more: false,
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
            vtcode_core::config::constants::tools::GREP_FILE,
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
            has_more: false,
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
            has_more: false,
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
    fn plan_file_write_does_not_increment_mutations() {
        let mut tracker = LoopTracker::new();
        let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: serde_json::json!({}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
            has_more: false,
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
            has_more: false,
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
}
