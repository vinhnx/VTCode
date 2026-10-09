//! History replayability, duplicate detection, and conversation push invariants.

use super::*;

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
                                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
                            current_batch.insert(tc.id.clone(), (canonical_tool_name(&func.name).to_string(), tc_args));
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
                    && tool_response_is_replayable(msg)
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

fn tool_response_is_replayable(message: &uni::Message) -> bool {
    let content = message.content.as_text();
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.len() > 128 * 1024 {
        return false;
    }

    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(serde_json::Value::Object(output)) => {
            if output.contains_key("error") || output.contains_key("error_type") || output.contains_key("failure_kind")
            {
                return false;
            }
            if output.get("blocked").and_then(serde_json::Value::as_bool) == Some(true)
                || output.get("verification_required").and_then(serde_json::Value::as_bool) == Some(true)
            {
                return false;
            }
            if matches!(output.get("success"), Some(serde_json::Value::Bool(false)) | Some(serde_json::Value::Null)) {
                return false;
            }
            if output.get("success").is_some_and(|value| !value.is_boolean()) {
                return false;
            }
            !output.get("status").and_then(serde_json::Value::as_str).is_some_and(|status| {
                matches!(
                    status.to_ascii_lowercase().as_str(),
                    "failed"
                        | "failure"
                        | "error"
                        | "denied"
                        | "permission_denied"
                        | "rejected"
                        | "timeout"
                        | "timed_out"
                        | "cancelled"
                        | "canceled"
                        | "interrupted"
                        | "aborted"
                        | "blocked"
                        | "skipped"
                        | "not_started"
                        | "not_executed"
                        | "pending"
                        | "in_progress"
                        | "not_run"
                )
            })
        }
        Ok(serde_json::Value::String(value)) => text_response_is_replayable(&value),
        Ok(serde_json::Value::Array(_) | serde_json::Value::Number(_) | serde_json::Value::Bool(_)) => true,
        Ok(serde_json::Value::Null) => false,
        Err(_) => text_response_is_replayable(trimmed),
    }
}

fn text_response_is_replayable(content: &str) -> bool {
    let trimmed = content.trim();
    const FAILURE_PREFIXES: &[&str] = &[
        "error:",
        "execution denied",
        "permission denied",
        "timeout",
        "timed out",
        "cancelled",
        "canceled",
        "failed",
        "failure",
        "denied",
        "rejected",
        "blocked",
        "aborted",
        "interrupted",
        "skipped",
        "not started",
        "not executed",
        "not run",
        "pending",
        "in progress",
    ];
    !FAILURE_PREFIXES.iter().any(|prefix| {
        trimmed
            .get(..prefix.len())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
    })
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
                    let Ok(args) = serde_json::from_str::<serde_json::Value>(&function.arguments) else {
                        continue;
                    };
                    if !vtcode_core::tools::tool_intent::classify_tool_intent(&function.name, &args).mutating {
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
                        vtcode_core::tools::code_search_scope_contains_mutated_path(search_args, path, workspace_root)
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

pub(super) fn tool_response_is_success(message: &uni::Message) -> bool {
    let Ok(output) = serde_json::from_str::<serde_json::Value>(&message.content.as_text()) else {
        return false;
    };
    let Some(output) = output.as_object() else {
        return false;
    };
    if output.contains_key("error") || output.contains_key("error_type") || output.contains_key("failure_kind") {
        return false;
    }
    if output.get("status").is_some_and(|status| status.as_str() != Some("success")) {
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
        && !output_has_error_signal(output)
}

fn output_has_actionable_recovery_guidance(output: &serde_json::Value) -> bool {
    ["hint", "next_action", "critical_note", "warning"].iter().any(|key| {
        output
            .get(*key)
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    }) || output
        .get("fallback_tool")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
        || output.get("hints").and_then(serde_json::Value::as_array).is_some_and(|hints| {
            hints
                .iter()
                .any(|hint| hint.as_str().is_some_and(|value| !value.trim().is_empty()))
        })
}

fn output_has_error_signal(output: &serde_json::Value) -> bool {
    ["error", "error_type", "stderr", "stderr_preview", "message"]
        .iter()
        .any(|key| !output_field_is_empty(output.get(*key)))
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

/// Detect the session-loss error text emitted by the exec session manager
/// ("exec session '<id>' not found. ...") and the PTY session manager
/// ("PTY session '<id>' not found"), mirroring the phrasing in `vtcode-core`
/// exec_session and pty session_ops tool errors.
pub(super) fn error_text_indicates_lost_session(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("not found") && (lower.contains("exec session") || lower.contains("pty session"))
}

/// Return whether `(canonical_name, args)` is a follow-up on an existing
/// exec/PTY session rather than a fresh command run.
///
/// `canonical_name` must already be canonicalized (see [`canonical_tool_name`]).
/// Any non-`run` `unified_exec` action counts as a follow-up
/// (poll/wait/inspect/continue/input, plus list/code/write/close), as does
/// any non-`run` PTY session tool (`read_pty_session`, `send_pty_input`,
/// `close_pty_session`, `list_pty_sessions`, `exec_pty_cmd`): session
/// follow-ups carry a `session_id`, not command text, so they never classify
/// as [`ShellActivity::Verification`]. A missing-session failure on one of
/// them therefore needs its own lost-result branch in
/// [`update_repetition_tracker`]: the verifier output it was waiting on died
/// with the session. Fresh `run` calls are excluded: a run creates its
/// session, so it cannot lose a prior verifier's result.
pub(super) fn is_session_follow_up(canonical_name: &str, args: &serde_json::Value) -> bool {
    use vtcode_core::config::constants::tools;
    if canonical_name == tools::WRITE_STDIN {
        return true;
    }
    if matches!(
        canonical_name,
        tools::UNIFIED_EXEC
            | tools::READ_PTY_SESSION
            | tools::SEND_PTY_INPUT
            | tools::CLOSE_PTY_SESSION
            | tools::LIST_PTY_SESSIONS
            | tools::EXEC_PTY_CMD
    ) && !vtcode_core::tools::tool_intent::is_command_run_tool_call(canonical_name, args)
    {
        return true;
    }
    false
}

pub(super) fn is_low_signal_outcome(
    outcome: &ToolPipelineOutcome,
    canonical_tool_name: &str,
    args: &serde_json::Value,
) -> bool {
    match &outcome.status {
        ToolExecutionStatus::Success { output, command_success, .. } => {
            output_has_empty_search_results(output)
                || output_reuses_recent_result(output)
                || (*command_success && is_empty_shell_search(canonical_tool_name, args, output))
                || (matches!(
                    canonical_tool_name,
                    vtcode_core::config::constants::tools::UNIFIED_EXEC
                        | vtcode_core::config::constants::tools::EXEC_COMMAND
                ) && !*command_success
                    && is_grep_style_no_match(canonical_tool_name, args, output))
        }
        ToolExecutionStatus::Failure { error } => error_is_missing_resource(&error.message),
        ToolExecutionStatus::Timeout { .. } | ToolExecutionStatus::Cancelled => false,
    }
}

/// Fingerprint only complete, positioned text evidence. This is a progress
/// signal, never a cache, admission decision, or verification verdict.
pub(super) struct NavigationEvidence {
    pub(super) signatures: Vec<String>,
    pub(super) focused_search_path: Option<vtcode_core::types::CompactStr>,
}

pub(super) fn navigation_evidence(
    outcome: &ToolPipelineOutcome,
    canonical_name: &str,
    args: &serde_json::Value,
) -> Option<NavigationEvidence> {
    use vtcode_core::config::constants::tools;
    let ToolExecutionStatus::Success { output, command_success: true, .. } = &outcome.status else {
        return None;
    };
    if ["truncated", "output_truncated"]
        .iter()
        .any(|key| output.get(key).and_then(serde_json::Value::as_bool) == Some(true))
        || output_has_error_signal(output)
    {
        return None;
    }
    let rows = if canonical_name == tools::CODE_SEARCH {
        output
            .get("results")?
            .as_array()?
            .iter()
            .map(|row| {
                if row.get("result_type")?.as_str()? != "text" {
                    return None;
                }
                Some(serde_json::json!({
                    "path": row.get("path")?.as_str()?,
                    "line": row.get("line")?.as_u64()?,
                    "text": row.get("snippet")?.as_str()?,
                }))
            })
            .collect::<Option<Vec<_>>>()?
    } else if matches!(canonical_name, tools::EXEC_COMMAND | tools::UNIFIED_EXEC)
        && classify_shell_activity(canonical_name, args) == ShellActivity::Inspection
    {
        // A live session returns only a chunk, whose line origin is unknown.
        // Only a completed successful range can establish positioned evidence.
        if output.get("exit_code").and_then(serde_json::Value::as_i64) != Some(0) {
            return None;
        }
        let target =
            crate::agent::runloop::unified::turn::tool_outcomes::handlers::parse_simple_exec_read_target(args)?;
        // Piped stages and awk programs transform the output (column widths,
        // counts), so those lines are not the file's own content and must
        // never be fingerprinted as positioned evidence. Loop guards still
        // count such reads via path/slice; only evidence needs verbatim.
        if !target.verbatim {
            return None;
        }
        output.get("output")?.as_str()?.lines().enumerate().map(|(offset, text)| {
            serde_json::json!({"path":target.path, "cwd":vtcode_core::tools::command_args::working_dir_text(args), "line":target.start_line.saturating_add(offset), "text":text})
        }).collect()
    } else {
        return None;
    };
    let first_path = rows.first()?.get("path")?.as_str()?;
    let focused_search_path = (canonical_name == tools::CODE_SEARCH
        && rows
            .iter()
            .all(|row| row.get("path").and_then(serde_json::Value::as_str) == Some(first_path)))
    .then(|| first_path.into());
    Some(NavigationEvidence {
        signatures: rows.iter().map(|row| signature_key_for("navigation_evidence", row)).collect(),
        focused_search_path,
    })
}

/// Coarse inspection family for duplicate-listing detection. Unlike the exact
/// `low_signal_family_key` (full normalized command), this groups overlapping
/// scans such as three `find` invocations over the same tree with different
/// flags, so successful but redundant rescans of one target still count
/// toward diagnostics. The family is scoped by binary AND search root:
/// `ls src` / `find src …` / `ls -1 src` share a target and count together,
/// while scans of distinct trees (`ls src` / `ls crates` / `ls tests`) are
/// legitimate exploration and never group.
pub(super) fn coarse_inspection_family_key(canonical_tool_name: &str, args: &serde_json::Value) -> Option<String> {
    use vtcode_core::config::constants::tools;
    // Only bare directory listings suffer from overlapping-but-distinct
    // invocations (e.g. three `find` calls over the same tree with different
    // flags) that the exact family key never groups. File reads (`cat`/`head`/
    // `tail` via shell included) and semantic search (`rg`/`grep`, `code_search`)
    // already carry precise family keys; grouping them coarsely would mislabel
    // diverse productive exploration (different files/queries) as looping.
    // In particular `rg`/`grep` must stay out: their first positional is the
    // search pattern, not the search root, so five distinct queries such as
    // `grep -n "enum Commands" ...`, `grep -rn "enum ExecSubcommand" ...`
    // (turn_1303/turn_1304: `exec::inspection::grep::enum ×5`) or five `rg`
    // searches for `pub` (turn_1291: `exec::inspection::rg::pub ×5`, e.g.
    // `rg -n 'pub enum Commands' ...`) all collapse into
    // one coarse family and get promoted to low-signal, tripping early
    // recovery on legitimate research. Distinct patterns/paths keep distinct
    // exact families and converge via the total low-signal guard instead.
    match canonical_tool_name {
        tools::UNIFIED_EXEC | tools::EXEC_COMMAND => {
            let command = vtcode_core::tools::command_args::command_text(args).ok()??;
            let first = command.split_whitespace().next().unwrap_or("");
            let base = first
                .rsplit('/')
                .next()
                .unwrap_or(first)
                .trim_matches(|ch| ch == '\'' || ch == '"')
                .to_ascii_lowercase();
            if matches!(base.as_str(), "find" | "ls" | "fd") {
                Some(format!("exec::inspection::{base}::{}", coarse_inspection_root(&command)))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Extract the search-root segment of a listing command: the first
/// non-flag argument, with surrounding quotes and trailing slashes stripped.
/// Deliberately heuristic — an option value (`ls --width 80 src` → root "80")
/// can be picked up and fragment a family, which only makes detection more
/// conservative. Commands with no positional argument (`ls -la`) scan the
/// working directory and map to ".". Only `ls`/`find`/`fd` reach this helper;
/// `rg`/`grep` are excluded above because their first positional is the
/// search pattern, not a path root.
pub(super) fn coarse_inspection_root(command: &str) -> String {
    let root = command
        .split_whitespace()
        .skip(1)
        .find(|token| !token.starts_with('-'))
        .unwrap_or(".");
    let root = root.trim_matches(|ch| ch == '\'' || ch == '"').trim_end_matches('/');
    if root.is_empty() { "." } else { root }.to_string()
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolResponseHistoryUpdate {
    Appended,
    Replaced { previous_text_len: usize },
}

pub(crate) fn push_tool_response<S>(
    history: &mut Vec<uni::Message>,
    tool_call_id: S,
    tool_name: Option<&str>,
    content: String,
) -> ToolResponseHistoryUpdate
where
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
        let previous_text_len = history[index].content.as_text().len();
        history[index].content = uni::MessageContent::Text(content);
        if let Some(tool_name) = tool_name {
            history[index].origin_tool = Some(tool_name.to_string());
        }
        return ToolResponseHistoryUpdate::Replaced { previous_text_len };
    }

    let tool_call_id = tool_call_id.into();
    history.push(match tool_name {
        Some(name) => uni::Message::tool_response_with_origin(tool_call_id, content, name.to_string()),
        None => uni::Message::tool_response(tool_call_id, content),
    });
    ToolResponseHistoryUpdate::Appended
}

/// Generate a tool signature key with predictable structure for loop tracking.
pub(crate) fn signature_key_for(name: &str, args: &serde_json::Value) -> String {
    // Keep keys compact on hot paths: hash bounded argument bytes instead of
    // allocating full JSON payloads for large tool arguments.
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut input_len = 0usize;
    let mutability_tag = if vtcode_core::tools::tool_intent::classify_tool_intent(name, args).mutating {
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
