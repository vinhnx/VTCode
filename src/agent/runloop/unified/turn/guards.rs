use crate::agent::runloop::unified::run_loop_context::RecoveryMode;
use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnProcessingContext};
use crate::agent::runloop::unified::turn::turn_processing::llm_attempt_timeout_secs;
use anyhow::Result;
use serde_json::Value;
use std::borrow::Cow;
use std::time::Duration;
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
        match registry.admit_public_tool_call(name, args) {
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
        tool_names::LIST_FILES => &["path"],
        tool_names::GREP_FILE => &["pattern", "path"],
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

    maybe_activate_turn_timeout_recovery(ctx);

    // Context trim and compaction has been removed - no proactive guards needed
    // The function is kept for future extensibility but now does minimal work

    Ok(())
}

#[cfg(test)]
fn should_activate_turn_timeout_recovery(
    tool_calls: usize,
    recovery_active: bool,
    elapsed: Duration,
    timeout_budget: Duration,
    reserve: Duration,
) -> bool {
    tool_calls > 0 && !recovery_active && elapsed >= timeout_budget.saturating_sub(reserve)
}

fn maybe_activate_turn_timeout_recovery(ctx: &mut TurnProcessingContext<'_>) {
    let configured_turn_timeout_secs = ctx
        .vt_cfg
        .map(|cfg| cfg.optimization.agent_execution.max_execution_time_secs)
        .unwrap_or(300);
    let reserve = Duration::from_secs(llm_attempt_timeout_secs(
        configured_turn_timeout_secs.max(1),
        ctx.session_stats.is_plan_mode(),
        ctx.provider_client.name(),
    ));
    if !ctx
        .harness_state
        .should_force_recovery_before_turn_timeout(reserve)
    {
        return;
    }

    let remaining_turn_budget_secs = ctx.harness_state.remaining_turn_timeout().as_secs();
    let tool_calls = ctx.harness_state.tool_calls;
    let reason = format!(
        "Turn budget nearly exhausted after {tool_calls} tool call(s); synthesize a final answer now without more tools (remaining_turn_budget_secs={remaining_turn_budget_secs})."
    );
    ctx.activate_recovery_with_mode(reason.clone(), RecoveryMode::ToolFreeSynthesis);
    ctx.push_system_message(reason);
    let _ = ctx.renderer.line(
        MessageStyle::Info,
        "Turn budget nearly exhausted; forcing a final tool-free synthesis pass.",
    );
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
            | tool_names::GREP_FILE
            | tool_names::LIST_FILES
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
        let recovery_reason = format!(
            "Navigation loop detected after {} consecutive read/search steps. Tools are disabled on the next pass; summarize findings and propose the next concrete action.",
            repeated_tool_attempts.consecutive_navigations
        );
        ctx.activate_recovery(recovery_reason.clone());
        ctx.renderer
            .line(
                MessageStyle::Warning,
                "[!] Navigation Loop: scheduling a recovery synthesis pass.",
            )
            .unwrap_or(());
        ctx.working_history.push(uni::Message::system(format!(
            "{} {}",
            recovery_reason,
            navigation_loop_guidance(ctx.session_stats.is_plan_mode())
        )));
        return apply_balancer_recovery(repeated_tool_attempts);
    }

    // --- Turn balancer: cap low-signal churn ---
    // Optimization: Skip with exponential backoff to reduce iteration frequency
    let check_interval = if step_count <= 4 {
        1
    } else {
        1_usize << ((step_count / 4).ilog2())
    };

    let effective_repeat_limit = tool_repeat_limit.max(3);
    let repeated_low_signal = repeated_tool_attempts.max_low_signal_count();
    if repeated_low_signal >= effective_repeat_limit
        && repeated_tool_attempts.consecutive_navigations >= effective_repeat_limit
    {
        let recovery_reason = format!(
            "Repeated low-signal navigation calls reached the per-turn fast-path cap ({}). Tools are disabled on the next pass; summarize only from collected evidence.",
            effective_repeat_limit
        );
        ctx.activate_recovery(recovery_reason.clone());
        ctx.renderer
            .line(
                MessageStyle::Info,
                "[!] Turn balancer: repeated low-signal navigation detected; scheduling an early recovery pass.",
            )
            .unwrap_or(());
        ctx.working_history
            .push(uni::Message::system(recovery_reason));
        {
            let mut ledger = ctx.decision_ledger.write().await;
            ledger.record_decision(
                "Turn balancer: Early recovery intervention".to_string(),
                vtcode_core::core::decision_tracker::Action::Response {
                    content:
                        "Repeated low-signal navigation was detected; an early tool-free recovery pass was scheduled."
                            .to_string(),
                    response_type:
                        vtcode_core::core::decision_tracker::ResponseType::ContextSummary,
                },
                None,
            );
        }
        return apply_balancer_recovery(repeated_tool_attempts);
    }

    if !step_count.is_multiple_of(check_interval) {
        return TurnHandlerOutcome::Continue;
    }

    // Exclude read-only tools from repeated count (they're legitimate exploration)
    let max_repeated = repeated_tool_attempts
        .max_count_filtered(is_readonly_signature)
        .max(repeated_low_signal);

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
        should_activate_turn_timeout_recovery, validate_tool_args_security,
    };
    use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
    use crate::agent::runloop::unified::turn::context::TurnHandlerOutcome;
    use crate::agent::runloop::unified::turn::context::TurnLoopResult;
    use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{
        LoopTracker, NAVIGATION_LOOP_THRESHOLD, update_repetition_tracker,
    };
    use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
    use serde_json::json;
    use std::time::Duration;
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
    fn readonly_signature_treats_safe_exec_runs_as_readonly() {
        assert!(is_readonly_signature(
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

    #[tokio::test]
    async fn navigation_loop_schedules_recovery_and_progress_only_recovery_text_blocks() {
        let mut backing = TestTurnProcessingBacking::new(8).await;
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        tracker.consecutive_navigations = NAVIGATION_LOOP_THRESHOLD;

        let balancer_outcome = super::handle_turn_balancer(&mut ctx, 1, &mut tracker, 8, 3).await;
        assert!(matches!(balancer_outcome, TurnHandlerOutcome::Continue));
        assert_eq!(tracker.consecutive_navigations, 0);
        assert!(ctx.is_recovery_active());
        assert!(ctx.working_history.iter().any(|message| {
            message
                .content
                .as_text()
                .contains("Navigation loop detected")
        }));
        assert!(ctx.consume_recovery_pass());

        let recovery_outcome = ctx
            .handle_text_response(
                "I'll inspect one more file and then summarize.".to_string(),
                Vec::new(),
                None,
                None,
                false,
            )
            .await
            .expect("recovery response should be handled");

        assert!(matches!(
            recovery_outcome,
            TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })
        ));
        assert!(!ctx.is_recovery_active());
    }

    #[tokio::test]
    async fn low_signal_search_churn_schedules_recovery_and_progress_only_recovery_text_blocks() {
        let mut backing = TestTurnProcessingBacking::new(8).await;
        backing.set_loop_limit(tool_names::UNIFIED_SEARCH, 2);
        let seeded_args = json!({"action":"grep","path":"vtcode-tui","pattern":"-> Result"});
        assert!(
            backing
                .record_tool_call(tool_names::UNIFIED_SEARCH, &seeded_args)
                .is_none()
        );
        let _ = backing.record_tool_call(tool_names::UNIFIED_SEARCH, &seeded_args);
        let warning = backing.record_tool_call(tool_names::UNIFIED_SEARCH, &seeded_args);
        assert!(warning.is_some());
        assert!(backing.is_hard_limit_exceeded(tool_names::UNIFIED_SEARCH));
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"matches": []}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });

        update_repetition_tracker(
            &mut tracker,
            &miss,
            tool_names::UNIFIED_SEARCH,
            &json!({
                "action": "structural",
                "pattern": "fn $name(...) -> Result<$T, $E>",
                "lang": "rust",
                "globs": ["vtcode-tui/**/*.rs"]
            }),
        );
        update_repetition_tracker(
            &mut tracker,
            &miss,
            tool_names::UNIFIED_SEARCH,
            &json!({
                "action": "grep",
                "pattern": "-> Result",
                "path": "vtcode-tui",
                "globs": ["vtcode-tui/**/*.rs"]
            }),
        );
        update_repetition_tracker(
            &mut tracker,
            &miss,
            tool_names::UNIFIED_SEARCH,
            &json!({
                "action": "grep",
                "pattern": "Result<",
                "path": "vtcode-tui",
                "globs": ["vtcode-tui/**/*.rs"]
            }),
        );

        let balancer_outcome = super::handle_turn_balancer(&mut ctx, 4, &mut tracker, 4, 3).await;
        assert!(matches!(balancer_outcome, TurnHandlerOutcome::Continue));
        assert!(ctx.is_recovery_active());
        assert!(ctx.consume_recovery_pass());

        let recovery_outcome = ctx
            .handle_text_response(
                "Let me try a narrower search next.".to_string(),
                Vec::new(),
                None,
                None,
                false,
            )
            .await
            .expect("recovery response should be handled");

        assert!(matches!(
            recovery_outcome,
            TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })
        ));
        assert!(!ctx.is_recovery_active());
        assert!(backing.is_hard_limit_exceeded(tool_names::UNIFIED_SEARCH));
    }

    #[tokio::test]
    async fn early_low_signal_search_churn_schedules_recovery_before_turn_window() {
        let mut backing = TestTurnProcessingBacking::new(20).await;
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"matches": []}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });

        update_repetition_tracker(
            &mut tracker,
            &miss,
            tool_names::UNIFIED_SEARCH,
            &json!({
                "action": "structural",
                "pattern": "fn $name(...) -> Result<$T, $E>",
                "lang": "rust",
                "globs": ["vtcode-tui/**/*.rs"]
            }),
        );
        update_repetition_tracker(
            &mut tracker,
            &miss,
            tool_names::UNIFIED_SEARCH,
            &json!({
                "action": "grep",
                "pattern": "-> Result",
                "path": "vtcode-tui",
                "globs": ["vtcode-tui/**/*.rs"]
            }),
        );
        update_repetition_tracker(
            &mut tracker,
            &miss,
            tool_names::UNIFIED_SEARCH,
            &json!({
                "action": "grep",
                "pattern": "Result<",
                "path": "vtcode-tui",
                "globs": ["vtcode-tui/**/*.rs"]
            }),
        );

        let balancer_outcome = super::handle_turn_balancer(&mut ctx, 3, &mut tracker, 20, 3).await;
        assert!(matches!(balancer_outcome, TurnHandlerOutcome::Continue));
        assert!(ctx.is_recovery_active());
        assert_eq!(tracker.consecutive_navigations, 0);
        assert!(ctx.working_history.iter().any(|message| {
            message
                .content
                .as_text()
                .contains("Repeated low-signal navigation calls reached the per-turn fast-path cap")
        }));
    }

    #[test]
    fn timeout_recovery_requires_tool_activity() {
        assert!(!should_activate_turn_timeout_recovery(
            0,
            false,
            Duration::from_secs(55),
            Duration::from_secs(60),
            Duration::from_secs(10),
        ));
    }

    #[test]
    fn timeout_recovery_triggers_near_deadline() {
        assert!(should_activate_turn_timeout_recovery(
            3,
            false,
            Duration::from_secs(55),
            Duration::from_secs(60),
            Duration::from_secs(10),
        ));
    }

    #[test]
    fn timeout_recovery_respects_existing_recovery() {
        assert!(!should_activate_turn_timeout_recovery(
            3,
            true,
            Duration::from_secs(55),
            Duration::from_secs(60),
            Duration::from_secs(10),
        ));
    }
}
