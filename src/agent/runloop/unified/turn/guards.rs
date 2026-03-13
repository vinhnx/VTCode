use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnProcessingContext};
use anyhow::Result;
use serde_json::Value;
use std::borrow::Cow;
use vtcode_core::utils::ansi::MessageStyle;

use std::sync::Arc;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::tools::validation_cache::ValidationCache;

/// Validates that a textual tool call has required arguments before execution.
/// Returns `None` if valid, or `Some(missing_params)` if validation fails.
///
/// This prevents executing tools with empty args that will just fail,
/// allowing the Model to continue naturally instead of hitting loop detection.
/// Validates that a textual tool call has required arguments and passes security checks.
/// Returns `None` if valid, or `Some(failures)` if validation fails.
///
/// Optimization: Uses static slices for required params to avoid allocations
pub(crate) fn validate_tool_args_security(
    name: &str,
    args: &serde_json::Value,
    validation_cache: Option<&Arc<ValidationCache>>,
    tool_registry: Option<&vtcode_core::tools::ToolRegistry>,
) -> Option<Vec<String>> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::io;
    use vtcode_core::tools::validation::{commands, paths};

    struct HasherWriter<'a, H: Hasher>(&'a mut H);
    impl<H: Hasher> io::Write for HasherWriter<'_, H> {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.write(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    // Calculate hash for caching
    let args_hash = if validation_cache.is_some() {
        let mut hasher = DefaultHasher::new();
        name.hash(&mut hasher);
        if serde_json::to_writer(HasherWriter(&mut hasher), args).is_err() {
            // Fallback path should be rare; keep it resilient.
            args.to_string().hash(&mut hasher);
        }
        Some(hasher.finish())
    } else {
        None
    };

    // Check cache
    if let Some(hash) = args_hash
        && let Some(cache) = validation_cache
    {
        // ValidationCache has interior mutability, use directly
        if let Some(is_valid) = cache.check(name, hash)
            && is_valid
        {
            return None; // Valid cached
        }
        // If invalid (false), we continue to re-validate to generate error messages
    }

    if let Some(registry) = tool_registry {
        let preflight = registry.preflight_validate_call(name, args);
        match preflight {
            Ok(_) => {
                if let Some(hash) = args_hash
                    && let Some(cache) = validation_cache
                {
                    cache.insert(name, hash, true);
                }
                return None;
            }
            Err(err) => {
                return Some(vec![err.to_string()]);
            }
        }
    }

    use vtcode_core::config::constants::tools as tool_names;

    fn is_missing_arg_value(args: &Value, key: &str) -> bool {
        match args.get(key) {
            Some(v) => {
                v.is_null() || (v.is_string() && v.as_str().is_none_or(|s| s.trim().is_empty()))
            }
            None => true,
        }
    }

    fn is_missing_required_arg(tool_name: &str, args: &Value, key: &str) -> bool {
        if tool_name == tool_names::EDIT_FILE {
            return match key {
                "old_str" => {
                    is_missing_arg_value(args, "old_str")
                        && is_missing_arg_value(args, "old_string")
                }
                "new_str" => {
                    is_missing_arg_value(args, "new_str")
                        && is_missing_arg_value(args, "new_string")
                }
                _ => is_missing_arg_value(args, key),
            };
        }

        is_missing_arg_value(args, key)
    }

    // Optimization: Early return for tools with no requirements
    static EMPTY_REQUIRED: &[&str] = &[];

    // 1. Check required arguments using static slices
    let required: &[&str] = match name {
        tool_names::READ_FILE => &["path"],
        tool_names::WRITE_FILE => &["path", "content"],
        tool_names::EDIT_FILE => &["path", "old_str", "new_str"],
        "list_files" => &["path"],
        "grep_file" => &["pattern", "path"],
        tool_names::RUN_PTY_CMD => &["command"],
        tool_names::APPLY_PATCH => &["patch"],
        _ => EMPTY_REQUIRED,
    };

    // Optimization: Pre-allocate failures vec only when needed
    let mut failures: Option<Vec<String>> = None;

    if !required.is_empty() {
        for key in required {
            if is_missing_required_arg(name, args, key) {
                failures
                    .get_or_insert_with(|| Vec::with_capacity(required.len()))
                    .push(format!("Missing required argument: {}", key));
            }
        }
    }
    if name == tool_names::UNIFIED_EXEC {
        let exec_failures =
            vtcode_core::tools::command_args::unified_exec_missing_required_args(args);
        if !exec_failures.is_empty() {
            failures
                .get_or_insert_with(|| Vec::with_capacity(exec_failures.len()))
                .extend(
                    exec_failures
                        .into_iter()
                        .map(|key| format!("Missing required argument: {}", key)),
                );
        }
    }

    // Early return if required args are missing
    if failures.is_some() {
        // Validation failed, no cache update (or cache as invalid if we wanted)
        return failures;
    }

    if name == tool_names::UNIFIED_EXEC
        && vtcode_core::tools::tool_intent::unified_exec_action(args).is_none()
    {
        return Some(vec![
            "Invalid arguments: missing action; provide `action` or inferable exec arguments"
                .to_string(),
        ]);
    }

    // 2. Perform security checks only if required args passed
    // Path safety checks
    if let Some(path) = args.get("path").and_then(|v| v.as_str())
        && let Err(e) = paths::validate_path_safety(path)
    {
        failures
            .get_or_insert_with(|| Vec::with_capacity(2))
            .push(format!("Path security check failed: {}", e));
    }

    // Command safety checks
    if (name == tool_names::RUN_PTY_CMD
        || (name == tool_names::UNIFIED_EXEC
            && vtcode_core::tools::command_args::unified_exec_requires_command_safety(args)))
        && let Some(cmd) = vtcode_core::tools::command_args::command_text(args)
            .ok()
            .flatten()
        && let Err(e) = commands::validate_command_safety(&cmd)
    {
        failures
            .get_or_insert_with(|| Vec::with_capacity(2))
            .push(format!("Command security check failed: {}", e));
    }

    // Update cache if valid
    if failures.is_none()
        && let Some(hash) = args_hash
        && let Some(cache) = validation_cache
    {
        // ValidationCache has interior mutability
        cache.insert(name, hash, true);
    }

    failures
}

pub(crate) async fn run_proactive_guards(
    ctx: &mut TurnProcessingContext<'_>,
    _step_count: usize,
) -> Result<()> {
    // Auto-prune decision ledgers to prevent unbounded memory growth
    {
        let mut decision_ledger = ctx.decision_ledger.write().await;
        decision_ledger.auto_prune();
    }

    // Context trim and compaction has been removed - no proactive guards needed
    // The function is kept for future extensibility but now does minimal work

    Ok(())
}

/// Check if a tool signature represents a read-only operation
/// Signature format: "tool_name:args_json" where args_json is serialized Value
fn is_readonly_signature(signature: &str) -> bool {
    if let Some(first_colon) = signature.find(':')
        && let Some(second_colon_rel) = signature[first_colon + 1..].find(':')
    {
        let tag_start = first_colon + 1;
        let tag_end = tag_start + second_colon_rel;
        match &signature[tag_start..tag_end] {
            "ro" => return true,
            "rw" => return false,
            _ => {}
        }
    }

    // Prefer `:{` / `:[` separators so tool names containing `::` don't break parsing.
    let colon_pos = signature
        .find(":{")
        .or_else(|| signature.find(":["))
        .or_else(|| signature.find(':'));
    let Some(colon_pos) = colon_pos else {
        return false;
    };
    let tool_name = normalize_turn_balancer_tool_name(&signature[..colon_pos]);
    let args_json = &signature[colon_pos + 1..];

    let tool_name_str: &str = tool_name.as_ref();

    if let Ok(args) = serde_json::from_str::<Value>(args_json) {
        return !vtcode_core::tools::tool_intent::classify_tool_intent(tool_name_str, &args)
            .mutating;
    }

    // Fallback for malformed signature payloads.
    if matches!(
        tool_name_str,
        tool_names::READ_FILE
            | "grep_file"
            | "list_files"
            | "search_tools"
            | "agent_info"
            | tool_names::UNIFIED_SEARCH
    ) {
        return true;
    }

    if tool_name_str == tool_names::UNIFIED_FILE {
        let lower_json = args_json.to_ascii_lowercase();
        return lower_json.contains(r#""action":"read""#)
            || lower_json.contains(r#""action": "read""#)
            || lower_json.contains(r#"'action':'read'"#);
    }
    if tool_name_str == tool_names::UNIFIED_EXEC {
        let lower_json = args_json.to_ascii_lowercase();
        return lower_json.contains(r#""action":"poll""#)
            || lower_json.contains(r#""action":"list""#)
            || lower_json.contains(r#""action":"inspect""#)
            || lower_json.contains(r#""action": "poll""#)
            || lower_json.contains(r#""action": "list""#)
            || lower_json.contains(r#""action": "inspect""#);
    }

    false
}

fn normalize_turn_balancer_tool_name(name: &str) -> Cow<'_, str> {
    let lowered = name.trim().to_ascii_lowercase();
    match lowered.as_str() {
        "read file" | "repo_browser.read_file" => Cow::Borrowed(tool_names::READ_FILE),
        "write file" | "repo_browser.write_file" => Cow::Borrowed(tool_names::WRITE_FILE),
        "edit file" => Cow::Borrowed(tool_names::EDIT_FILE),
        "search text" | "list files" | "structural search" | "code intelligence" | "list tools"
        | "list errors" | "show agent info" | "fetch" => Cow::Borrowed(tool_names::UNIFIED_SEARCH),
        "run command (pty)" | "run command" | "run code" | "exec code" | "bash"
        | "container.exec" => Cow::Borrowed(tool_names::UNIFIED_EXEC),
        "apply patch" | "delete file" | "move file" | "copy file" | "file operation" => {
            Cow::Borrowed(tool_names::UNIFIED_FILE)
        }
        _ => Cow::Owned(lowered),
    }
}

fn navigation_loop_guidance(plan_mode: bool) -> &'static str {
    if plan_mode {
        "WARNING: Too many read/search steps in Plan Mode without an actionable output. Stop browsing, summarize key findings, then update `task_tracker` with concrete steps (files + outcome + verification), or ask one blocking question."
    } else {
        "WARNING: Too many read/search steps without edits or execution. Summarize findings and propose the next concrete edit/action, or explain the blocker."
    }
}

pub(crate) async fn handle_turn_balancer(
    ctx: &mut TurnProcessingContext<'_>,
    step_count: usize,
    repeated_tool_attempts: &mut crate::agent::runloop::unified::turn::tool_outcomes::helpers::LoopTracker,
    max_tool_loops: usize,
    tool_repeat_limit: usize,
) -> TurnHandlerOutcome {
    use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{
        BLIND_EDITING_THRESHOLD, NAVIGATION_LOOP_THRESHOLD,
    };
    use vtcode_core::llm::provider as uni;

    // NL2Repo-Bench checks run on every step (no backoff) since they
    // are safety guardrails, not performance optimizations.

    // NL2Repo-Bench: Edit-Test Validation Loop (Anti-Blind-Editing)
    if repeated_tool_attempts.consecutive_mutations >= BLIND_EDITING_THRESHOLD {
        ctx.renderer
            .line(
                MessageStyle::Warning,
                "[!] Anti-Blind-Editing: Pause to run verification/tests.",
            )
            .unwrap_or(());
        ctx.working_history.push(uni::Message::system(
            "CRITICAL: Multiple edits were made without verification. Stop editing and run `unified_exec` to compile or test before proceeding."
                .to_string(),
        ));
        repeated_tool_attempts.consecutive_mutations = 0;
        return TurnHandlerOutcome::Continue;
    }

    // NL2Repo-Bench: Navigation Loop Detection
    if repeated_tool_attempts.consecutive_navigations >= NAVIGATION_LOOP_THRESHOLD {
        ctx.renderer
            .line(
                MessageStyle::Warning,
                "[!] Navigation Loop: Pause to synthesize or act.",
            )
            .unwrap_or(());
        ctx.working_history.push(uni::Message::system(
            navigation_loop_guidance(ctx.session_stats.is_plan_mode()).to_string(),
        ));
        repeated_tool_attempts.consecutive_navigations = 0;
        return TurnHandlerOutcome::Continue;
    }

    // --- Turn balancer: cap low-signal churn ---
    // Optimization: Skip with exponential backoff to reduce iteration frequency
    let check_interval = if step_count <= 4 {
        1
    } else {
        1_usize << ((step_count / 4).ilog2())
    };

    if !step_count.is_multiple_of(check_interval) {
        return TurnHandlerOutcome::Continue;
    }

    // Exclude read-only tools from repeated count (they're legitimate exploration)
    let max_repeated = repeated_tool_attempts.max_count_filtered(is_readonly_signature);

    if crate::agent::runloop::unified::turn::utils::should_trigger_turn_balancer(
        step_count,
        max_tool_loops,
        max_repeated,
        tool_repeat_limit,
    ) {
        let recovery_reason =
            "Turn balancer detected repeated low-signal tool churn. Tools are disabled on the next pass; summarize only from collected evidence.".to_string();
        ctx.activate_recovery(recovery_reason.clone());
        ctx.renderer
            .line(
                MessageStyle::Info,
                "[!] Turn balancer: repeated low-signal calls detected; scheduling a final recovery pass.",
            )
            .unwrap_or(());
        ctx.working_history
            .push(uni::Message::system(recovery_reason));
        // Record in ledger
        {
            let mut ledger = ctx.decision_ledger.write().await;
            ledger.record_decision(
                "Turn balancer: Recovery intervention".to_string(),
                vtcode_core::core::decision_tracker::Action::Response {
                    content:
                        "Low-signal churn detected; a final tool-free recovery pass was scheduled."
                            .to_string(),
                    response_type:
                        vtcode_core::core::decision_tracker::ResponseType::ContextSummary,
                },
                None,
            );
        }
        return apply_balancer_recovery(repeated_tool_attempts);
    }

    TurnHandlerOutcome::Continue
}

fn apply_balancer_recovery(
    repeated_tool_attempts: &mut crate::agent::runloop::unified::turn::tool_outcomes::helpers::LoopTracker,
) -> TurnHandlerOutcome {
    repeated_tool_attempts.reset_after_balancer_recovery();
    TurnHandlerOutcome::Continue
}

#[cfg(test)]
mod tests {
    use super::{
        apply_balancer_recovery, is_readonly_signature, navigation_loop_guidance,
        validate_tool_args_security,
    };
    use crate::agent::runloop::unified::turn::context::TurnHandlerOutcome;
    use crate::agent::runloop::unified::turn::tool_outcomes::helpers::LoopTracker;
    use serde_json::json;
    use vtcode_core::config::constants::tools as tool_names;

    #[test]
    fn readonly_signature_handles_alias_and_search_signatures() {
        assert!(is_readonly_signature(r#"read file:{"path":"README.md"}"#));
        assert!(is_readonly_signature(
            r#"search text:{"pattern":"match provider_event","path":"vtcode-core/src/llm/providers/anthropic/api.rs"}"#
        ));
        assert!(is_readonly_signature(
            r#"unified_search:{"pattern":"LLMStreamEvent::","path":"vtcode-core/src/llm/providers/anthropic/api.rs"}"#
        ));
    }

    #[test]
    fn readonly_signature_keeps_exec_run_non_readonly() {
        assert!(!is_readonly_signature(
            r#"unified_exec:{"action":"run","command":"cargo check"}"#
        ));
    }

    #[test]
    fn readonly_signature_fast_path_accepts_ro_tag() {
        assert!(is_readonly_signature("unified_search:ro:len42-fnv1234abcd"));
    }

    #[test]
    fn readonly_signature_fast_path_rejects_rw_tag() {
        assert!(!is_readonly_signature("unified_file:rw:len42-fnv1234abcd"));
    }

    #[test]
    fn validate_edit_file_args_accepts_legacy_old_new_string_keys() {
        let args = json!({
            "path": "src/lib.rs",
            "old_string": "before",
            "new_string": "after"
        });

        assert!(validate_tool_args_security(tool_names::EDIT_FILE, &args, None, None).is_none());
    }

    #[test]
    fn validate_edit_file_args_still_rejects_when_replacements_missing() {
        let args = json!({
            "path": "src/lib.rs"
        });

        let failures =
            validate_tool_args_security(tool_names::EDIT_FILE, &args, None, None).unwrap();
        assert!(failures.iter().any(|msg| msg.contains("old_str")));
        assert!(failures.iter().any(|msg| msg.contains("new_str")));
    }

    #[test]
    fn validate_unified_exec_args_without_registry_reports_single_missing_command() {
        let failures = validate_tool_args_security(
            tool_names::UNIFIED_EXEC,
            &json!({"action": "run"}),
            None,
            None,
        )
        .expect("missing command should fail");

        assert_eq!(
            failures,
            vec!["Missing required argument: command".to_string()]
        );
    }

    #[test]
    fn validate_unified_exec_args_without_registry_rejects_missing_action() {
        let failures =
            validate_tool_args_security(tool_names::UNIFIED_EXEC, &json!({}), None, None)
                .expect("missing action should fail");

        assert_eq!(
            failures,
            vec![
                "Invalid arguments: missing action; provide `action` or inferable exec arguments"
                    .to_string()
            ]
        );
    }

    #[test]
    fn navigation_loop_guidance_mentions_task_tracker_in_plan_mode() {
        let guidance = navigation_loop_guidance(true);
        assert!(guidance.contains("task_tracker"));
    }

    #[test]
    fn navigation_loop_guidance_uses_generic_text_outside_plan_mode() {
        let guidance = navigation_loop_guidance(false);
        assert!(guidance.contains("read/search"));
        assert!(!guidance.contains("task_tracker"));
    }

    #[test]
    fn balancer_recovery_continues_and_resets_tracker() {
        let mut tracker = LoopTracker::new();
        let sig = r#"unified_exec:{"action":"run","command":"cargo test"}"#.to_string();
        tracker.record(sig.clone());
        tracker.record(sig.clone());
        tracker.record(sig);
        tracker.consecutive_mutations = 3;
        tracker.consecutive_navigations = 5;

        let outcome = apply_balancer_recovery(&mut tracker);

        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert_eq!(tracker.max_count_filtered(|_| false), 0);
        assert_eq!(tracker.consecutive_mutations, 0);
        assert_eq!(tracker.consecutive_navigations, 0);
    }
}
