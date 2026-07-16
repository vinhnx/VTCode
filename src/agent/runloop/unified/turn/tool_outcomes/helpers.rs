use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
use crate::agent::runloop::unified::turn::tool_outcomes::read_extent;
use rustc_hash::{FxHashMap, FxHashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::names::canonical_tool_name;

pub(crate) const FINISH_PLANNING_REASON_USER_REQUESTED_IMPLEMENTATION: &str =
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
    /// Number of times navigation-loop recovery has fired in this session.
    pub navigation_loop_recoveries: usize,
    /// Unique navigation signatures in the current consecutive window.
    /// Used to distinguish legitimate exploration (all unique) from actual looping (many repeats).
    nav_signatures: FxHashSet<String>,
}

impl LoopTracker {
    pub(crate) fn new() -> Self {
        Self {
            attempts: FxHashMap::with_capacity_and_hasher(16, Default::default()),
            low_signal_attempts: FxHashMap::with_capacity_and_hasher(8, Default::default()),
            consecutive_mutations: 0,
            consecutive_navigations: 0,
            navigation_loop_recoveries: 0,
            nav_signatures: FxHashSet::default(),
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

    /// Number of redundant navigations (total - unique) in the current window.
    /// At least 3 before the navigation loop guard considers firing.
    pub(crate) fn repeated_navigation_count(&self) -> usize {
        self.consecutive_navigations
            .saturating_sub(self.nav_signatures.len())
    }

    fn reset_low_signal_attempts(&mut self) {
        self.low_signal_attempts.clear();
    }

    pub(crate) fn reset_after_balancer_recovery(&mut self) {
        self.attempts.clear();
        self.low_signal_attempts.clear();
        self.nav_signatures.clear();
        self.consecutive_mutations = 0;
        self.consecutive_navigations = 0;
    }
}

/// Check if an identical tool call (same name + same args) was already executed
/// recently in the working history. Returns the output of the most recent
/// matching tool response if found.
///
/// This catches cross-turn duplicates that the per-turn `LoopTracker` misses
/// because it is reset at the start of each turn. Scans the last
/// `MAX_HISTORY_SCAN` messages to keep the check bounded.
///
/// File-read pagination is normalised so that re-reading the same file with a
/// different `offset` or `limit` is recognised as the same logical read.
/// `code_search` uses a separate replay identity that retains the effective
/// `max_results`; its loop identity is separate.
///
/// Tool-call IDs are scoped to the nearest preceding Assistant batch. A later
/// batch may reuse an ID for another tool, so both the batch and tool name must
/// match before its Tool response can satisfy this replay lookup.
pub(crate) fn find_duplicate_in_history(
    history: &[uni::Message],
    tool_name: &str,
    args: &serde_json::Value,
    workspace_root: &Path,
) -> Option<String> {
    const MAX_HISTORY_SCAN: usize = 120;
    let target_signature = read_normalized_signature_key(tool_name, args);

    let scan_start = history.len().saturating_sub(MAX_HISTORY_SCAN);
    let target_tool_name = canonical_tool_name(tool_name);
    let mut current_batch: FxHashMap<String, (String, serde_json::Value)> = FxHashMap::default();
    let mut matching_responses = Vec::new();

    for (offset, msg) in history[scan_start..].iter().enumerate() {
        let abs_idx = scan_start + offset;
        match msg.role {
            uni::MessageRole::Assistant => {
                current_batch.clear();
                if let Some(ref tool_calls) = msg.tool_calls {
                    for tc in tool_calls {
                        if let Some(ref func) = tc.function {
                            let tc_args: serde_json::Value = serde_json::from_str(&func.arguments)
                                .unwrap_or_else(|_| {
                                    serde_json::Value::Object(serde_json::Map::new())
                                });
                            current_batch.insert(
                                tc.id.clone(),
                                (canonical_tool_name(&func.name).to_string(), tc_args),
                            );
                        }
                    }
                }
            }
            uni::MessageRole::Tool => {
                let Some(call_id) = msg.tool_call_id.as_deref() else {
                    continue;
                };
                let Some((batch_tool_name, tc_args)) = current_batch.get(call_id) else {
                    continue;
                };
                if batch_tool_name == target_tool_name
                    && read_normalized_signature_key(batch_tool_name, tc_args) == target_signature
                    && read_extent::extent_covers(tc_args, args)
                {
                    matching_responses.push((abs_idx, tc_args.clone(), msg));
                }
            }
            _ => {}
        }
    }

    for (response_index, tc_args, msg) in matching_responses.into_iter().rev() {
        let invalidated = tool_name == vtcode_core::config::constants::tools::CODE_SEARCH
            && history_has_scoped_mutation_after(history, response_index, &tc_args, workspace_root);
        if !invalidated {
            return Some(msg.content.as_text().to_string());
        }
    }
    None
}

fn history_has_scoped_mutation_after(
    history: &[uni::Message],
    response_index: usize,
    search_args: &serde_json::Value,
    workspace_root: &Path,
) -> bool {
    let mut pending_mutations: FxHashMap<String, Vec<PathBuf>> = FxHashMap::default();
    for message in history.iter().skip(response_index.saturating_add(1)) {
        match message.role {
            uni::MessageRole::Assistant => {
                // Tool-call IDs are scoped to one Assistant batch and may be
                // reused later. Unanswered calls from an earlier batch were
                // never executed, so they must not survive this boundary.
                pending_mutations.clear();
                let Some(tool_calls) = message.tool_calls.as_ref() else {
                    continue;
                };
                for tool_call in tool_calls {
                    let Some(function) = tool_call.function.as_ref() else {
                        continue;
                    };
                    let Ok(args) = serde_json::from_str::<serde_json::Value>(&function.arguments)
                    else {
                        continue;
                    };
                    if !vtcode_core::tools::tool_intent::classify_tool_intent(&function.name, &args)
                        .mutating
                    {
                        continue;
                    }
                    let paths = vtcode_core::tools::mutation_target_paths(&function.name, &args);
                    if !paths.is_empty() {
                        pending_mutations.insert(tool_call.id.clone(), paths);
                    }
                }
            }
            uni::MessageRole::Tool => {
                let Some(call_id) = message.tool_call_id.as_deref() else {
                    continue;
                };
                let Some(paths) = pending_mutations.remove(call_id) else {
                    continue;
                };
                if tool_response_is_success(message)
                    && paths.iter().any(|path| {
                        vtcode_core::tools::code_search_scope_contains_mutated_path(
                            search_args,
                            path,
                            workspace_root,
                        )
                    })
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn tool_response_is_success(message: &uni::Message) -> bool {
    let Ok(output) = serde_json::from_str::<serde_json::Value>(&message.content.as_text()) else {
        return false;
    };
    let Some(output) = output.as_object() else {
        return false;
    };
    if output.contains_key("error")
        || output.contains_key("error_type")
        || output.contains_key("failure_kind")
    {
        return false;
    }
    if output
        .get("status")
        .is_some_and(|status| status.as_str() != Some("success"))
    {
        return false;
    }

    match output.get("success") {
        Some(serde_json::Value::Bool(success)) => *success,
        Some(_) => false,
        None => output
            .get("status")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|status| status == "success"),
    }
}

fn output_has_empty_search_results(output: &serde_json::Value) -> bool {
    output
        .get("results")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|results| results.is_empty())
        && !output_has_actionable_recovery_guidance(output)
}

fn output_has_actionable_recovery_guidance(output: &serde_json::Value) -> bool {
    ["hint", "next_action", "critical_note"].iter().any(|key| {
        output
            .get(*key)
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    }) || output
        .get("fallback_tool")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
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
            output_has_empty_search_results(output)
                || output_reuses_recent_result(output)
                || (canonical_tool_name == vtcode_core::config::constants::tools::UNIFIED_EXEC
                    && output_is_grep_style_miss(output, *command_success))
        }
        ToolExecutionStatus::Failure { error } => error_is_missing_resource(&error.message),
        ToolExecutionStatus::Timeout { .. } | ToolExecutionStatus::Cancelled => false,
    }
}

/// Upsert a tool result into `history`, keyed on `tool_call_id`.
///
/// This is a **bounded** upsert: the reverse scan stops as soon as it reaches
/// ANY Assistant message (regardless of its tool_calls). This is critical:
/// Assistant messages represent turn boundaries. Tool responses from before an
/// Assistant must never be overwritten by Tool responses from after it, even
/// when fabricated tool_call_ids collide across turns.
///
/// If a Tool message with a matching id is found *before* the nearest
/// Assistant boundary, it is a legitimate same-call update (e.g. an
/// auto-permission probe replaying a result) and gets overwritten in place.
/// If the boundary is hit first, the id has been reused across turns, so we
/// append instead of clobbering an unrelated, earlier Tool result.
pub(crate) fn push_tool_response<S>(
    history: &mut Vec<uni::Message>,
    tool_call_id: S,
    tool_name: Option<&str>,
    content: String,
) where
    S: AsRef<str> + Into<String>,
{
    let tool_call_id_ref = tool_call_id.as_ref();
    let mut overwrite_index = None;
    for (index, message) in history.iter().enumerate().rev() {
        match message.role {
            uni::MessageRole::Tool => {
                if message.tool_call_id.as_deref() == Some(tool_call_id_ref) {
                    overwrite_index = Some(index);
                    break;
                }
            }
            // Stop at ANY Assistant message — it marks a turn boundary.
            // Tool responses from before this Assistant must not be overwritten.
            uni::MessageRole::Assistant => {
                break;
            }
            _ => {}
        }
    }

    if let Some(index) = overwrite_index {
        history[index].content = uni::MessageContent::Text(content);
        return;
    }

    let tool_call_id = tool_call_id.into();
    history.push(match tool_name {
        Some(name) => {
            uni::Message::tool_response_with_origin(tool_call_id, content, name.to_string())
        }
        None => uni::Message::tool_response(tool_call_id, content),
    });
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
    push_tool_response(history, tool_call_id, Some(tool_name), payload.to_string());
}

pub(crate) fn build_finish_planning_args(reason: &str) -> serde_json::Value {
    serde_json::json!({
        "reason": reason
    })
}

pub(crate) fn build_step_finish_planning_call_id(step_count: usize) -> String {
    format!("call_{step_count}_finish_planning")
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

/// Generate a read-normalized signature key for cross-turn dedup.
///
/// File-read tools (`file_operation` with `read` action, `read_file`,
/// `grep_file`, `list_files`) omit pagination and read-offset fields so that
/// re-reading the same target groups under one logical read. `code_search`
/// uses its normalised result-replay identity, which preserves the effective
/// `max_results`; its separate loop identity may group searches across limits.
///
/// For mutating tools the original `signature_key_for` is returned unchanged.
pub(crate) fn read_normalized_signature_key(name: &str, args: &serde_json::Value) -> String {
    if name == vtcode_core::config::constants::tools::CODE_SEARCH
        && let Some(identity) = vtcode_core::tools::normalised_code_search_identity(args)
    {
        return format!("{name}:ro:{identity}");
    }

    if !is_read_only_tool_args(name, args) {
        return signature_key_for(name, args);
    }

    let Some(mut obj) = args.as_object().cloned() else {
        return signature_key_for(name, args);
    };

    // Strip pagination / read-offset fields that don't change *what* is read.
    for key in read_extent::normalization_strip_keys() {
        obj.remove(key);
    }

    let normalized = serde_json::Value::Object(obj);
    signature_key_for(name, &normalized)
}

/// Returns `true` when `(name, args)` describe a read-only tool invocation.
fn is_read_only_tool_args(name: &str, args: &serde_json::Value) -> bool {
    use vtcode_core::config::constants::tools;
    match name {
        tools::READ_FILE | tools::GREP_FILE | tools::LIST_FILES => true,
        tools::CODE_SEARCH => true,
        tools::UNIFIED_SEARCH | "search_dispatch" => true,
        tools::UNIFIED_FILE | "file_operation" => {
            matches!(args.get("action").and_then(|v| v.as_str()), Some("read"))
        }
        _ => false,
    }
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
    use vtcode_core::tools::tool_intent::file_operation_action;

    let canonical = canonical_tool_name(name);
    match canonical {
        tool_names::TASK_TRACKER => true,
        tool_names::UNIFIED_FILE => {
            if !file_operation_action(args)
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
    let signature_key = signature_key_for(canonical_name, args);
    loop_tracker.record(signature_key.clone());
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
    // `command_session(action=run)` as `mutating: true` because shell commands *can*
    // mutate state, but for the Edit-Test heuristic, any execution/verification
    // step (cargo check, cargo test, etc.) should RESET the mutation counter,
    // not increment it.
    use vtcode_core::config::constants::tools as tool_names;

    let is_execution_tool = matches!(
        canonical_name,
        n if n == tool_names::UNIFIED_EXEC
            || n == tool_names::RUN_PTY_CMD
            || n == tool_names::EXECUTE_CODE
            || n == tool_names::SHELL
    );

    if is_execution_tool {
        // Execution/verification step resets both counters
        loop_tracker.consecutive_mutations = 0;
        loop_tracker.consecutive_navigations = 0;
        loop_tracker.nav_signatures.clear();
        if low_signal_family.is_none() {
            loop_tracker.reset_low_signal_attempts();
        }
    } else if is_plan_artifact_write(canonical_name, args) {
        // Plan artifact writes in dedicated plan storage are allowed in Planning workflow and
        // should not trigger anti-blind-editing verification pressure.
        loop_tracker.consecutive_navigations = 0;
        loop_tracker.nav_signatures.clear();
    } else {
        let intent = vtcode_core::tools::tool_intent::classify_tool_intent(canonical_name, args);
        if intent.mutating {
            loop_tracker.consecutive_mutations += 1;
            loop_tracker.consecutive_navigations = 0;
            loop_tracker.nav_signatures.clear();
            if low_signal_family.is_none() {
                loop_tracker.reset_low_signal_attempts();
            }
        } else {
            // Read-only / navigation tool
            loop_tracker.consecutive_navigations += 1;
            loop_tracker.nav_signatures.insert(signature_key);
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

/// Extract the JSON output from a successful `ToolPipelineOutcome`.
///
/// Returns `Some(&Value)` for successful executions, `None` for failures,
/// timeouts, and cancellations.
pub(crate) fn tool_output_from_outcome(
    outcome: &ToolPipelineOutcome,
) -> Option<&serde_json::Value> {
    match &outcome.status {
        ToolExecutionStatus::Success { output, .. } => Some(output),
        _ => None,
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
    use vtcode_core::config::constants::tools;

    #[test]
    fn push_tool_response_replaces_existing_tool_call_entry() {
        let mut history = vec![uni::Message::tool_response(
            "call_1".to_string(),
            "{\"output\":\"first\"}".to_string(),
        )];

        push_tool_response(
            &mut history,
            "call_1".to_string(),
            None,
            "{\"output\":\"latest\"}".to_string(),
        );

        assert_eq!(history.len(), 1);
        assert_eq!(
            history[0].content.as_text_borrowed(),
            Some("{\"output\":\"latest\"}")
        );
    }

    #[test]
    fn push_tool_response_sets_origin_tool_when_provided() {
        let mut history = Vec::new();

        push_tool_response(
            &mut history,
            "call_1".to_string(),
            Some("read_file"),
            "{\"output\":\"first\"}".to_string(),
        );

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].origin_tool.as_deref(), Some("read_file"));
    }

    #[test]
    fn push_tool_response_appends_when_id_reused_across_assistant_boundary() {
        // Fabricated ids can collide across turns (e.g. index-based fallbacks).
        // A later assistant message re-declaring the same id must not cause a
        // new result to clobber the earlier, unrelated Tool response.
        let mut history = vec![
            uni::Message::assistant_with_tools(
                "first".into(),
                vec![uni::ToolCall::function(
                    "call_1".into(),
                    "file_operation".into(),
                    "{}".into(),
                )],
            ),
            uni::Message::tool_response("call_1".to_string(), "{\"output\":\"first\"}".into()),
            uni::Message::assistant_with_tools(
                "second".into(),
                vec![uni::ToolCall::function(
                    "call_1".into(),
                    tools::CODE_SEARCH.into(),
                    "{}".into(),
                )],
            ),
        ];

        push_tool_response(
            &mut history,
            "call_1".to_string(),
            Some(tools::CODE_SEARCH),
            "{\"output\":\"second\"}".to_string(),
        );

        let tool_messages: Vec<&uni::Message> = history
            .iter()
            .filter(|message| matches!(message.role, uni::MessageRole::Tool))
            .collect();
        assert_eq!(tool_messages.len(), 2, "must append, not overwrite");
        assert_eq!(
            tool_messages[0].content.as_text_borrowed(),
            Some("{\"output\":\"first\"}"),
            "earlier unrelated Tool result must remain intact"
        );
        assert_eq!(
            tool_messages[1].content.as_text_borrowed(),
            Some("{\"output\":\"second\"}")
        );
    }

    #[test]
    fn push_tool_response_appends_when_assistant_has_no_tool_calls() {
        // When an Assistant message has no tool_calls (e.g. commentary-only
        // message between tool calls), the boundary must STILL stop the scan.
        // Otherwise a later Tool response with a colliding fabricated id would
        // overwrite an earlier, unrelated Tool result.
        let mut history = vec![
            uni::Message::assistant_with_tools(
                String::new(),
                vec![uni::ToolCall::function(
                    "call_0".into(),
                    "file_operation".into(),
                    "{}".into(),
                )],
            ),
            uni::Message::tool_response(
                "call_0".to_string(),
                "{\"output\":\"file content\"}".into(),
            ),
            // Commentary Assistant with no tool_calls — must act as boundary
            uni::Message::assistant("I need to retry.".into()),
            uni::Message::assistant_with_tools(
                String::new(),
                vec![uni::ToolCall::function(
                    "call_0".into(),
                    "apply_patch".into(),
                    "{}".into(),
                )],
            ),
        ];

        push_tool_response(
            &mut history,
            "call_0".to_string(),
            Some("apply_patch"),
            "{\"output\":\"patch result\"}".to_string(),
        );

        let tool_messages: Vec<&uni::Message> = history
            .iter()
            .filter(|message| matches!(message.role, uni::MessageRole::Tool))
            .collect();
        assert_eq!(
            tool_messages.len(),
            2,
            "must append, not overwrite the earlier file read"
        );
        assert_eq!(
            tool_messages[0].content.as_text_borrowed(),
            Some("{\"output\":\"file content\"}"),
            "earlier file read result must remain intact"
        );
        assert_eq!(
            tool_messages[1].content.as_text_borrowed(),
            Some("{\"output\":\"patch result\"}")
        );
    }

    #[test]
    fn repetition_tracker_counts_failures() {
        let mut tracker = LoopTracker::new();
        let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
            error: vtcode_core::tools::registry::ToolExecutionError::new(
                "edit_file".to_string(),
                vtcode_core::tools::registry::ToolErrorType::ExecutionError,
                "boom".to_string(),
            ),
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
        tracker.record("code_search:{\"query\":\"Widget\"}".to_string());
        tracker.record("code_search:{\"query\":\"Widget\"}".to_string());
        tracker.consecutive_mutations = 2;
        tracker.consecutive_navigations = 4;
        tracker.record_low_signal("code_search::Widget::src".to_string());
        tracker.navigation_loop_recoveries = 3;

        tracker.reset_after_balancer_recovery();

        assert_eq!(tracker.max_count_filtered(|_| false), 0);
        assert_eq!(tracker.max_low_signal_count(), 0);
        assert_eq!(tracker.consecutive_mutations, 0);
        assert_eq!(tracker.consecutive_navigations, 0);
        assert_eq!(tracker.navigation_loop_recoveries, 3);
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
            tools::UNIFIED_EXEC,
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
            tools::READ_FILE,
            &json!({"path":"src/main.rs"}),
        );
        assert_eq!(tracker.consecutive_navigations, 1);
        assert_eq!(tracker.consecutive_mutations, 0);

        update_repetition_tracker(
            &mut tracker,
            &success,
            tools::GREP_FILE,
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
                tools::READ_FILE,
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
    fn task_tracker_does_not_increment_mutations_in_planning() {
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
            tools::TASK_TRACKER,
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
            tools::TASK_TRACKER,
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
            tools::UNIFIED_FILE,
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
            tools::UNIFIED_FILE,
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
            output: serde_json::json!({"results":[]}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });

        // Different queries produce separate family keys, so each counts as its
        // own family while the agent explores one path.
        update_repetition_tracker(
            &mut tracker,
            &miss,
            tools::CODE_SEARCH,
            &json!({"query":"Widget", "path":"src", "result_types":["definition"]}),
        );
        update_repetition_tracker(
            &mut tracker,
            &miss,
            tools::CODE_SEARCH,
            &json!({"query":"Result", "path":"src", "result_types":["usage"]}),
        );
        update_repetition_tracker(
            &mut tracker,
            &miss,
            tools::CODE_SEARCH,
            &json!({"query":"Result<", "path":"src", "result_types":["text"]}),
        );

        assert_eq!(tracker.max_low_signal_count(), 1);
    }

    #[test]
    fn low_signal_tracker_groups_identical_searches_in_same_family() {
        let mut tracker = LoopTracker::new();
        let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: serde_json::json!({"results":[]}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });

        let args = json!({"query":"TODO","path":"src","file_types":["rust"]});
        update_repetition_tracker(&mut tracker, &miss, tools::CODE_SEARCH, &args);
        update_repetition_tracker(&mut tracker, &miss, tools::CODE_SEARCH, &args);
        update_repetition_tracker(&mut tracker, &miss, tools::CODE_SEARCH, &args);

        assert_eq!(tracker.max_low_signal_count(), 3);
    }

    #[test]
    fn low_signal_tracker_ignores_empty_search_results_with_recovery_guidance() {
        let mut tracker = LoopTracker::new();
        let guided = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: serde_json::json!({
                "results": [],
                "hint": "Try narrowing the path.",
                "is_recoverable": true,
                "next_action": "Retry with narrower filters."
            }),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });

        update_repetition_tracker(
            &mut tracker,
            &guided,
            tools::CODE_SEARCH,
            &json!({"query":"run", "path":"src/agent", "result_types":["definition"]}),
        );

        assert_eq!(tracker.max_low_signal_count(), 0);
    }

    #[test]
    fn low_signal_tracker_counts_missing_read_failures() {
        let mut tracker = LoopTracker::new();
        let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
            error: vtcode_core::tools::registry::ToolExecutionError::new(
                tools::UNIFIED_FILE.to_string(),
                vtcode_core::tools::registry::ToolErrorType::ResourceNotFound,
                "Resource not found: vtcode-tui/src/main.rs".to_string(),
            ),
        });

        // Two reads of the same path with different offsets are *different*
        // slices (paginated exploration), not a retry loop. The slice-aware
        // family key keeps them as distinct families, each with count 1.
        // Regression: previously both collapsed into one family with count 2,
        // which falsely tripped the family cap when the model paginated a
        // missing file (checkpoint turn_613 pattern).
        update_repetition_tracker(
            &mut tracker,
            &miss,
            tools::UNIFIED_FILE,
            &json!({"action":"read","path":"vtcode-tui/src/main.rs"}),
        );
        update_repetition_tracker(
            &mut tracker,
            &miss,
            tools::UNIFIED_FILE,
            &json!({"action":"read","path":"vtcode-tui/src/main.rs","offset":40}),
        );

        assert_eq!(
            tracker.max_low_signal_count(),
            1,
            "paginated reads (different offset) must be distinct families, not one family with count 2"
        );
    }

    #[test]
    fn low_signal_tracker_counts_identical_missing_read_failures() {
        // True retry loop: same path + same slice, repeated. The low-signal
        // count must accumulate so the turn balancer can stop the churn.
        let mut tracker = LoopTracker::new();
        let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
            error: vtcode_core::tools::registry::ToolExecutionError::new(
                tools::UNIFIED_FILE.to_string(),
                vtcode_core::tools::registry::ToolErrorType::ResourceNotFound,
                "Resource not found: vtcode-tui/src/main.rs".to_string(),
            ),
        });

        let identical_args = json!({"action":"read","path":"vtcode-tui/src/main.rs"});
        update_repetition_tracker(&mut tracker, &miss, tools::UNIFIED_FILE, &identical_args);
        update_repetition_tracker(&mut tracker, &miss, tools::UNIFIED_FILE, &identical_args);

        assert_eq!(
            tracker.max_low_signal_count(),
            2,
            "identical retry reads must accumulate into one family with count 2"
        );
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
            tools::UNIFIED_EXEC,
            &json!({"action":"run","command":"grep -n '-> Result' vtcode-tui/src/**/*.rs"}),
        );
        update_repetition_tracker(
            &mut tracker,
            &miss,
            tools::UNIFIED_EXEC,
            &json!({"action":"run","command":"grep -n \"-> Result\" vtcode-tui/src/**/*.rs"}),
        );

        assert_eq!(tracker.max_low_signal_count(), 2);
    }

    // --- read_normalized_signature_key tests ---

    #[test]
    fn read_normalized_signature_key_normalizes_file_operation_read_offset() {
        let args_a = json!({"action": "read", "path": "src/lib.rs", "offset": 0, "limit": 100});
        let args_b = json!({"action": "read", "path": "src/lib.rs", "offset": 50, "limit": 200});
        let key_a = read_normalized_signature_key("file_operation", &args_a);
        let key_b = read_normalized_signature_key("file_operation", &args_b);
        assert_eq!(
            key_a, key_b,
            "same file read with different offset/limit should produce the same normalized key"
        );
    }

    #[test]
    fn read_normalized_signature_key_differentiates_different_paths() {
        let args_a = json!({"action": "read", "path": "src/lib.rs"});
        let args_b = json!({"action": "read", "path": "src/main.rs"});
        let key_a = read_normalized_signature_key("file_operation", &args_a);
        let key_b = read_normalized_signature_key("file_operation", &args_b);
        assert_ne!(key_a, key_b, "different paths must produce different keys");
    }

    #[test]
    fn read_normalized_signature_key_includes_code_search_limit_and_normalises_filter_order() {
        let args_a = json!({
            "query": "Widget",
            "path": "src",
            "file_types": ["rust", "typescript"],
            "result_types": ["text", "definition"],
            "max_results": 10
        });
        let args_b = json!({
            "query": "Widget",
            "path": "src",
            "file_types": ["typescript", "rs"],
            "result_types": ["definition", "text"],
            "max_results": 100
        });
        let key_a = read_normalized_signature_key(tools::CODE_SEARCH, &args_a);
        let key_b = read_normalized_signature_key(tools::CODE_SEARCH, &args_b);
        assert_ne!(
            key_a, key_b,
            "different effective limits must not share one code-search replay identity"
        );

        let args_default = json!({
            "query": " Widget ",
            "path": "src",
            "file_types": ["rs", "typescript"],
            "result_types": ["definition", "text"]
        });
        let args_explicit_default = json!({
            "query": "Widget",
            "path": "src",
            "file_types": ["typescript", "rust"],
            "result_types": ["text", "definition"],
            "max_results": 20
        });
        assert_eq!(
            read_normalized_signature_key(tools::CODE_SEARCH, &args_default),
            read_normalized_signature_key(tools::CODE_SEARCH, &args_explicit_default),
            "omitted and explicit default limits must share replay identity"
        );
    }

    #[test]
    fn read_normalized_signature_key_preserves_mutation_for_write() {
        let args_a = json!({"path": "src/lib.rs", "content": "old"});
        let args_b = json!({"path": "src/lib.rs", "content": "new"});
        let key_a = read_normalized_signature_key("file_operation", &args_a);
        let key_b = read_normalized_signature_key("file_operation", &args_b);
        assert_ne!(key_a, key_b, "mutating writes must NOT be normalized away");
    }

    #[test]
    fn find_duplicate_in_history_matches_normalized_read() {
        use vtcode_core::llm::provider as uni;

        // find_duplicate_in_history uses read_normalized_signature_key, which
        // strips offset/limit for file reads. A later unrelated Assistant batch
        // must not obscure the earlier matching call and result pair.

        // Verify normalization: same file + different offset/limit → same key
        let key_a = read_normalized_signature_key(
            tools::UNIFIED_FILE,
            &json!({"action":"read","path":"src/lib.rs","offset":0,"limit":100}),
        );
        let key_b = read_normalized_signature_key(
            tools::UNIFIED_FILE,
            &json!({"action":"read","path":"src/lib.rs","offset":50,"limit":500}),
        );
        assert_eq!(
            key_a, key_b,
            "same file read with different offset/limit should normalize to the same key"
        );

        // Verify: different file → different key
        let key_c = read_normalized_signature_key(
            tools::UNIFIED_FILE,
            &json!({"action":"read","path":"src/main.rs","offset":0,"limit":100}),
        );
        assert_ne!(
            key_a, key_c,
            "different files must produce different normalized keys"
        );

        // Verify: code-search result limits remain distinct while filter ordering normalises away.
        let s_key_a = read_normalized_signature_key(
            tools::CODE_SEARCH,
            &json!({"query":"Widget","path":"src","file_types":["rust","typescript"],"result_types":["text","definition"],"max_results":10}),
        );
        let s_key_b = read_normalized_signature_key(
            tools::CODE_SEARCH,
            &json!({"query":"Widget","path":"src","file_types":["typescript","rs"],"result_types":["definition","text"],"max_results":100}),
        );
        assert_ne!(
            s_key_a, s_key_b,
            "different effective limits must not share one code-search replay identity"
        );

        // Verify: write NOT normalized
        let w_key_a = read_normalized_signature_key(
            tools::UNIFIED_FILE,
            &json!({"action":"write","path":"src/lib.rs","content":"old"}),
        );
        let w_key_b = read_normalized_signature_key(
            tools::UNIFIED_FILE,
            &json!({"action":"write","path":"src/lib.rs","content":"new"}),
        );
        assert_ne!(w_key_a, w_key_b, "writes must not be normalized away");

        // Verify: find_duplicate_in_history still works for EXACT match
        let mut history: Vec<uni::Message> = Vec::new();
        history.push(uni::Message::assistant_with_tools(
            "read".into(),
            vec![uni::ToolCall::function(
                "tc_exact".into(),
                tools::UNIFIED_FILE.into(),
                serde_json::to_string(
                    &json!({"action":"read","path":"src/lib.rs","offset":0,"limit":100}),
                )
                .unwrap(),
            )],
        ));
        history.push(uni::Message {
            role: uni::MessageRole::Tool,
            content: uni::MessageContent::text("exact content".into()),
            tool_call_id: Some("tc_exact".into()),
            ..Default::default()
        });
        // Second pair (different file) so the scan finds A₀'s Tool after A₁:
        history.push(uni::Message::assistant_with_tools(
            "read other".into(),
            vec![uni::ToolCall::function(
                "tc_other".into(),
                tools::UNIFIED_FILE.into(),
                serde_json::to_string(&json!({"action":"read","path":"src/main.rs"})).unwrap(),
            )],
        ));
        history.push(uni::Message {
            role: uni::MessageRole::Tool,
            content: uni::MessageContent::text("other content".into()),
            tool_call_id: Some("tc_other".into()),
            ..Default::default()
        });

        let result = find_duplicate_in_history(
            &history,
            tools::UNIFIED_FILE,
            &json!({"action":"read","path":"src/lib.rs","offset":0,"limit":50}),
            Path::new("."),
        );
        assert_eq!(result.as_deref(), Some("exact content"));
    }

    #[test]
    fn find_duplicate_in_history_respects_normalised_code_search_limit() {
        let original_args = json!({
            "query": "Widget",
            "path": "src",
            "file_types": ["rust", "typescript"],
            "result_types": ["text", "definition"],
            "max_results": 10
        });
        let history = vec![
            uni::Message::assistant_with_tools(
                "search".into(),
                vec![uni::ToolCall::function(
                    "tc_search".into(),
                    tools::CODE_SEARCH.into(),
                    serde_json::to_string(&original_args).unwrap(),
                )],
            ),
            uni::Message {
                role: uni::MessageRole::Tool,
                content: uni::MessageContent::text("{\"results\":[]}".into()),
                tool_call_id: Some("tc_search".into()),
                ..Default::default()
            },
        ];

        let different_limit = find_duplicate_in_history(
            &history,
            tools::CODE_SEARCH,
            &json!({
                "query": "Widget",
                "path": "src",
                "file_types": ["typescript", "rs"],
                "result_types": ["definition", "text"],
                "max_results": 100
            }),
            Path::new("."),
        );

        assert_eq!(different_limit, None);

        let equivalent_default_history = vec![
            uni::Message::assistant_with_tools(
                "search".into(),
                vec![uni::ToolCall::function(
                    "tc_default".into(),
                    tools::CODE_SEARCH.into(),
                    serde_json::to_string(&json!({
                        "query": "Widget",
                        "path": "src",
                        "max_results": 20
                    }))
                    .unwrap(),
                )],
            ),
            uni::Message {
                role: uni::MessageRole::Tool,
                content: uni::MessageContent::text("{\"results\":[1]}".into()),
                tool_call_id: Some("tc_default".into()),
                ..Default::default()
            },
        ];
        let reused = find_duplicate_in_history(
            &equivalent_default_history,
            tools::CODE_SEARCH,
            &json!({"query": " Widget ", "path": "src"}),
            Path::new("."),
        );
        assert_eq!(reused.as_deref(), Some("{\"results\":[1]}"));
    }

    #[test]
    fn working_history_code_search_replay_stops_at_in_scope_mutation() {
        let search_args = json!({"query": "Widget", "path": "src"});
        let search_call = uni::Message::assistant_with_tools(
            "search".into(),
            vec![uni::ToolCall::function(
                "search_call".into(),
                tools::CODE_SEARCH.into(),
                serde_json::to_string(&search_args).unwrap(),
            )],
        );
        let search_result = uni::Message {
            role: uni::MessageRole::Tool,
            content: uni::MessageContent::text("{\"results\":[\"cached\"]}".into()),
            tool_call_id: Some("search_call".into()),
            ..Default::default()
        };
        let mutation = |path: &str, result: serde_json::Value| {
            let patch = format!(
                "*** Begin Patch\n*** Update File: {path}\n@@\n-Widget\n+Gadget\n*** End Patch\n"
            );
            vec![
                uni::Message::assistant_with_tools(
                    "edit".into(),
                    vec![uni::ToolCall::function(
                        "edit_call".into(),
                        tools::APPLY_PATCH.into(),
                        serde_json::to_string(&json!({"patch": patch})).unwrap(),
                    )],
                ),
                uni::Message::tool_response("edit_call".into(), result.to_string()),
            ]
        };

        let mut in_scope_history = vec![search_call.clone(), search_result.clone()];
        in_scope_history.extend(mutation("src/widget.rs", json!({"success": true})));
        assert!(
            find_duplicate_in_history(
                &in_scope_history,
                tools::CODE_SEARCH,
                &search_args,
                Path::new("."),
            )
            .is_none(),
            "editing src/widget.rs after searching src must force a fresh search"
        );

        let mut status_success_history = vec![search_call.clone(), search_result.clone()];
        status_success_history.extend(mutation(
            "src/widget.rs",
            json!({"status": "success", "output": "patch applied"}),
        ));
        assert!(
            find_duplicate_in_history(
                &status_success_history,
                tools::CODE_SEARCH,
                &search_args,
                Path::new("."),
            )
            .is_none(),
            "the established successful status shape must invalidate replay"
        );

        let mut unrelated_history = vec![search_call.clone(), search_result.clone()];
        unrelated_history.extend(mutation("tests/widget.rs", json!({"success": true})));
        assert_eq!(
            find_duplicate_in_history(
                &unrelated_history,
                tools::CODE_SEARCH,
                &search_args,
                Path::new("."),
            )
            .as_deref(),
            Some("{\"results\":[\"cached\"]}"),
            "an unrelated edit may reuse the prior scoped search"
        );

        for failure in [
            json!({"success": false, "error": "patch rejected"}),
            json!({"error": {"message": "execution denied by policy"}}),
            json!({"failure_kind": "timeout"}),
            json!({"status": "failed"}),
            json!({"status": "denied"}),
            json!({"success": null}),
            json!({"output": "patch output without an outcome"}),
            json!(["non-object mutation output"]),
        ] {
            let mut failed_history = vec![search_call.clone(), search_result.clone()];
            failed_history.extend(mutation("src/widget.rs", failure));
            assert_eq!(
                find_duplicate_in_history(
                    &failed_history,
                    tools::CODE_SEARCH,
                    &search_args,
                    Path::new("."),
                )
                .as_deref(),
                Some("{\"results\":[\"cached\"]}"),
                "a mutation without explicit positive success evidence must preserve reuse"
            );
        }

        let mut unexecuted_history = vec![search_call, search_result];
        let unexecuted_mutation = mutation("src/widget.rs", json!({"success": true}));
        unexecuted_history.push(unexecuted_mutation[0].clone());
        assert_eq!(
            find_duplicate_in_history(
                &unexecuted_history,
                tools::CODE_SEARCH,
                &search_args,
                Path::new("."),
            )
            .as_deref(),
            Some("{\"results\":[\"cached\"]}"),
            "an unexecuted mutation call must preserve reuse"
        );
    }

    #[test]
    fn mutation_tool_response_success_rejects_malformed_and_conflicting_shapes() {
        let response =
            |content: &str| uni::Message::tool_response("edit_call".into(), content.into());

        assert!(tool_response_is_success(&response(r#"{"success":true}"#)));
        assert!(tool_response_is_success(&response(
            r#"{"status":"success","output":"patch applied"}"#,
        )));

        for content in [
            "not json",
            "null",
            r#"{"success":null,"status":"success"}"#,
            r#"{"success":true,"status":"failed"}"#,
            r#"{"success":true,"failure_kind":"timeout"}"#,
            r#"{"success":true,"error":"execution denied"}"#,
        ] {
            assert!(
                !tool_response_is_success(&response(content)),
                "mutation outcome must not count as successful: {content}"
            );
        }
    }

    #[test]
    fn working_history_code_search_replay_rejects_reused_patch_call_id() {
        let search_args = json!({"query": "Widget", "path": "src"});
        let shared_call_id = "call_0";
        let search_call = uni::Message::assistant_with_tools(
            "search".into(),
            vec![uni::ToolCall::function(
                shared_call_id.into(),
                tools::CODE_SEARCH.into(),
                serde_json::to_string(&search_args).unwrap(),
            )],
        );
        let search_result = uni::Message::tool_response(
            shared_call_id.into(),
            "{\"results\":[\"genuine search output\"]}".into(),
        );
        let patch = "*** Begin Patch\n*** Update File: src/widget.rs\n@@\n-Widget\n+Gadget\n*** End Patch\n";
        let patch_call = uni::Message::assistant_with_tools(
            "edit".into(),
            vec![uni::ToolCall::function(
                shared_call_id.into(),
                tools::APPLY_PATCH.into(),
                serde_json::to_string(&json!({"patch": patch})).unwrap(),
            )],
        );

        let mut successful_history = vec![
            search_call.clone(),
            search_result.clone(),
            patch_call.clone(),
            uni::Message::tool_response(
                shared_call_id.into(),
                json!({"success": true, "output": "patch output"}).to_string(),
            ),
        ];
        assert!(
            find_duplicate_in_history(
                &successful_history,
                tools::CODE_SEARCH,
                &search_args,
                Path::new("."),
            )
            .is_none(),
            "a successful in-scope patch must invalidate the genuine earlier search result"
        );

        successful_history.pop();
        successful_history.push(uni::Message::tool_response(
            shared_call_id.into(),
            json!({"success": false, "error": "patch rejected", "output": "patch output"})
                .to_string(),
        ));
        assert_eq!(
            find_duplicate_in_history(
                &successful_history,
                tools::CODE_SEARCH,
                &search_args,
                Path::new("."),
            )
            .as_deref(),
            Some("{\"results\":[\"genuine search output\"]}"),
            "a failed patch must preserve the earlier search without returning patch output"
        );
    }

    #[test]
    fn read_extent_covers_query_rejects_larger_limit() {
        // Cached limit=200 must NOT cover query limit=220
        assert!(!read_extent::extent_covers(
            &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
            &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":220}),
        ));

        // Cached limit=200 covers query limit=200 (same)
        assert!(read_extent::extent_covers(
            &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
            &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
        ));

        // Cached limit=200 covers query limit=100 (subset)
        assert!(read_extent::extent_covers(
            &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
            &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":100}),
        ));

        // Different offset must not match
        assert!(!read_extent::extent_covers(
            &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
            &json!({"action":"read","path":"AGENTS.md","offset":50,"limit":200}),
        ));
    }

    #[test]
    fn read_extent_covers_query_rejects_different_raw_mode() {
        // Non-raw cached must NOT cover raw=true query
        assert!(!read_extent::extent_covers(
            &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
            &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200,"raw":true}),
        ));

        // Raw cached covers raw query
        assert!(read_extent::extent_covers(
            &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200,"raw":true}),
            &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200,"raw":true}),
        ));

        // Raw cached must NOT cover non-raw query
        assert!(!read_extent::extent_covers(
            &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200,"raw":true}),
            &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
        ));
    }

    #[test]
    fn read_extent_covers_query_handles_missing_limit() {
        // Both missing limit → matches (same default read)
        assert!(read_extent::extent_covers(
            &json!({"action":"read","path":"AGENTS.md"}),
            &json!({"action":"read","path":"AGENTS.md"}),
        ));

        // Cached has limit, query doesn't → mismatch
        assert!(!read_extent::extent_covers(
            &json!({"action":"read","path":"AGENTS.md","limit":200}),
            &json!({"action":"read","path":"AGENTS.md"}),
        ));

        // Cached has no limit, query does → mismatch
        assert!(!read_extent::extent_covers(
            &json!({"action":"read","path":"AGENTS.md"}),
            &json!({"action":"read","path":"AGENTS.md","limit":200}),
        ));
    }
}
