use std::borrow::Cow;
use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::tools::validation_cache::ValidationCache;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext};

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
    args: &Value,
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
            Some(v) => v.is_null() || (v.is_string() && v.as_str().is_none_or(|s| s.trim().is_empty())),
            None => true,
        }
    }

    fn is_missing_required_arg(tool_name: &str, args: &Value, key: &str) -> bool {
        if tool_name == tool_names::EDIT_FILE {
            return match key {
                "old_str" => is_missing_arg_value(args, "old_str") && is_missing_arg_value(args, "old_string"),
                "new_str" => is_missing_arg_value(args, "new_str") && is_missing_arg_value(args, "new_string"),
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
                    .push(format!("Missing required argument: {key}"));
            }
        }
    }
    if name == tool_names::UNIFIED_EXEC {
        let exec_failures = vtcode_core::tools::command_args::command_session_missing_required_args(args);
        if !exec_failures.is_empty() {
            failures
                .get_or_insert_with(|| Vec::with_capacity(exec_failures.len()))
                .extend(exec_failures.into_iter().map(|key| format!("Missing required argument: {key}")));
        }
    }

    // Early return if required args are missing
    if failures.is_some() {
        // Validation failed, no cache update (or cache as invalid if we wanted)
        return failures;
    }

    if name == tool_names::UNIFIED_EXEC && vtcode_core::tools::tool_intent::command_session_action(args).is_none() {
        return Some(vec![
            "Invalid arguments: missing action; provide `action` or inferable exec arguments".to_string(),
        ]);
    }

    // 2. Perform security checks only if required args passed
    // Path safety checks
    if let Some(path) = args.get("path").and_then(|v| v.as_str())
        && let Err(e) = paths::validate_path_safety(path)
    {
        failures
            .get_or_insert_with(|| Vec::with_capacity(2))
            .push(format!("Path security check failed: {e}"));
    }

    // Command safety checks
    if (name == tool_names::RUN_PTY_CMD
        || (name == tool_names::UNIFIED_EXEC
            && vtcode_core::tools::command_args::command_session_requires_command_safety(args)))
        && let Some(cmd) = vtcode_core::tools::command_args::command_text(args).ok().flatten()
        && let Err(e) = commands::validate_command_safety(&cmd)
    {
        failures
            .get_or_insert_with(|| Vec::with_capacity(2))
            .push(format!("Command security check failed: {e}"));
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

pub(crate) async fn run_proactive_guards(ctx: &mut TurnProcessingContext<'_>, _step_count: usize) -> Result<()> {
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
        return !vtcode_core::tools::tool_intent::classify_tool_intent(tool_name_str, &args).mutating;
    }

    // Fallback for malformed signature payloads.
    if matches!(
        tool_name_str,
        tool_names::READ_FILE
            | tool_names::GREP_FILE
            | tool_names::LIST_FILES
            | tool_names::CODE_SEARCH
            | "search_tools"
            | "agent_info"
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
            || lower_json.contains(r#""action":"run""#)
            || lower_json.contains(r#""action": "poll""#)
            || lower_json.contains(r#""action": "list""#)
            || lower_json.contains(r#""action": "inspect""#)
            || lower_json.contains(r#""action": "run""#);
    }

    false
}

fn normalize_turn_balancer_tool_name(name: &str) -> Cow<'_, str> {
    let lowered = name.trim().to_ascii_lowercase();
    match lowered.as_str() {
        "read file" | "repo_browser.read_file" => Cow::Borrowed(tool_names::READ_FILE),
        "write file" | "repo_browser.write_file" => Cow::Borrowed(tool_names::WRITE_FILE),
        "edit file" => Cow::Borrowed(tool_names::EDIT_FILE),
        "code search" | "search code" => Cow::Borrowed(tool_names::CODE_SEARCH),
        "search text"
        | "list files"
        | "structural search"
        | "code intelligence"
        | "list tools"
        | "list errors"
        | "show agent info"
        | "fetch"
        | "search_dispatch"
        | "search_dispatch_internal" => Cow::Borrowed(tool_names::UNIFIED_SEARCH),
        "run command (pty)"
        | "run command"
        | "run code"
        | "exec code"
        | "bash"
        | "command_session"
        | "container.exec"
        | "command_session_internal" => Cow::Borrowed(tool_names::UNIFIED_EXEC),
        "apply patch"
        | "delete file"
        | "move file"
        | "copy file"
        | "file operation"
        | "file_operation"
        | "file_operation_internal" => Cow::Borrowed(tool_names::UNIFIED_FILE),
        _ => Cow::Owned(lowered),
    }
}

/// Shared plan-format suffix for planning synthesis recovery reasons.
///
/// Planning convergence guards (low-signal,
/// repeated-navigation) schedule the same single tool-free synthesis pass, so
/// they must instruct the same `<proposed_plan>` contract. Without the format
/// the model emits research prose that fails validation and the turn ends
/// `Blocked` even though the evidence was present.
const PLANNING_SYNTHESIS_FORMAT_HINT: &str = "Synthesize exactly one complete `<proposed_plan>` from the evidence already gathered: include Summary, numbered steps as `Action -> files: [path] -> verify: [command]`, Validation, and Assumptions. Every implementation step must name a concrete file, symbol, or behavior target and a concrete `verify:` command or observable check. Valid examples: `verify: [cargo nextest run -p vtcode]`, `verify: [cargo check --locked]`, `verify: [rg -n 'symbol' src/file.rs]`, `verify: [sed -n '1,40p' docs/file.md]`, `verify: [grep -n 'symbol' src/file.rs]`, or `verify: [git show --stat HEAD]` for a targeted review. Reuse visible evidence and keep output focused. Invalid examples: `verify: [run checks]`, `verify: [check later]`, and `verify: [git diff --check]`; vague prose and generic VCS-only checks fail validation. If a list has multiple comma-separated checks, every item must independently be concrete. Do not emit tool calls or tool-call markup.";

fn navigation_loop_guidance(planning_active: bool, repetition: usize) -> &'static str {
    if repetition >= 2 {
        "The navigation-loop guard has fired again this turn, so more read/search steps are unlikely to change the answer. Reply with only a direct synthesis and the next action, or one blocking question."
    } else if planning_active {
        "Planning has taken many read/search steps without an actionable output. Summarize key findings, then update `task_tracker` with concrete steps (files + outcome + verification), or ask one blocking question."
    } else {
        "Many read/search steps have run without edits or execution. Summarize findings and propose the next concrete edit/action, or explain the blocker."
    }
}

/// Diagnostic suffix for recovery reasons: names the family the model looped
/// on so the synthesis prompt (and session archives) record what converged.
/// Suppressed below two repeats: diverse churn (a new query each time) has no
/// dominant family to name.
fn churn_reason_note(churn: Option<&(String, usize)>) -> String {
    churn
        .filter(|(_, count)| *count >= 2)
        .map_or(String::new(), |(family, count)| format!(" Top churn: {family} ×{count}."))
}

/// Compact renderer-line annotation for the dominant churn family.
fn churn_line_label(churn: Option<&(String, usize)>) -> String {
    churn
        .filter(|(_, count)| *count >= 2)
        .map_or(String::new(), |(family, count)| format!(" ({family} ×{count})"))
}

/// Keep navigation recovery distinct from policy/budget synthesis.
fn activate_navigation_recovery(ctx: &mut TurnProcessingContext<'_>, reason: String) -> bool {
    if ctx.is_recovery_active() {
        return false;
    }
    if !ctx.is_planning_active()
        && ctx.session_stats.navigation_recovery == crate::agent::runloop::unified::state::NavigationRecovery::Consumed
    {
        ctx.session_stats.navigation_recovery.arm();
        return false;
    }
    if !ctx.activate_recovery(reason) {
        return false;
    }
    if !ctx.is_planning_active() {
        ctx.session_stats.navigation_recovery.arm();
    }
    true
}

/// Arm the early tool-free synthesis pass and, only when actually armed,
/// announce it in the UI, history, and decision ledger. Returns `false` when
/// a recovery was already armed (e.g. by the blocked-tool fuse in the same
/// batch) so the intervention is never claimed twice.
async fn arm_and_announce_early_recovery(
    ctx: &mut TurnProcessingContext<'_>,
    recovery_reason: String,
    churn: Option<&(String, usize)>,
) -> bool {
    if !activate_navigation_recovery(ctx, recovery_reason.clone()) {
        return false;
    }
    ctx.renderer
        .line(
            MessageStyle::Info,
            &format!(
                "[!] Turn balancer: repeated low-signal navigation detected{}; scheduling an early recovery pass.",
                churn_line_label(churn)
            ),
        )
        .unwrap_or(());
    ctx.working_history
        .push(vtcode_core::llm::provider::Message::system(recovery_reason));
    let mut ledger = ctx.decision_ledger.write().await;
    ledger.record_decision(
        "Turn balancer: Early recovery intervention".to_string(),
        vtcode_core::core::decision_tracker::Action::Response {
            content: "Repeated low-signal navigation was detected; an early tool-free recovery pass was scheduled."
                .to_string(),
            response_type: vtcode_core::core::decision_tracker::ResponseType::ContextSummary,
        },
        None,
    );
    true
}

pub(crate) async fn handle_turn_balancer(
    ctx: &mut TurnProcessingContext<'_>,
    step_count: usize,
    repeated_tool_attempts: &mut crate::agent::runloop::unified::turn::tool_outcomes::helpers::LoopTracker,
    max_tool_loops: usize,
    tool_repeat_limit: usize,
) -> TurnHandlerOutcome {
    use vtcode_core::llm::provider as uni;

    use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{
        ANTI_BLIND_EDITING_DIRECTIVE, ANTI_BLIND_EDITING_WARNING, EXECUTION_TOTAL_LOW_SIGNAL_THRESHOLD,
        LISTING_LOOP_TRIP_COUNT, NAVIGATION_LOOP_THRESHOLD, PLANNING_CONSECUTIVE_LOW_SIGNAL_THRESHOLD,
        PLANNING_LISTING_LOOP_TRIP_COUNT, PLANNING_TOTAL_LOW_SIGNAL_THRESHOLD,
    };

    // NL2Repo-Bench checks run on every step (no backoff) since they
    // are safety guardrails, not performance optimizations.

    // NL2Repo-Bench: Edit-Test Validation Loop (Anti-Blind-Editing)
    if repeated_tool_attempts.verification_is_pending() {
        repeated_tool_attempts.mark_verification_pending();
        if !repeated_tool_attempts.verification_warning_emitted {
            ctx.renderer
                .line(MessageStyle::Warning, ANTI_BLIND_EDITING_WARNING)
                .unwrap_or(());
            let directive = if ctx.tool_registry.completion_verification_pending() {
                ctx.tool_registry.unverified_completion_reason()
            } else {
                ANTI_BLIND_EDITING_DIRECTIVE.to_string()
            };
            ctx.working_history.push(uni::Message::system(directive));
            repeated_tool_attempts.verification_warning_emitted = true;
        }
        return TurnHandlerOutcome::Continue;
    }

    if !ctx.is_planning_active() && repeated_tool_attempts.tracker_observations_stalled() {
        if repeated_tool_attempts.tracker_observation_warning_emitted {
            return TurnHandlerOutcome::Break(TurnLoopResult::Blocked {
                reason: Some("Tracker observation stalled after one bounded correction. Repeated unchanged lists or rejected polls do not advance the task; the latest tracker and tool evidence are retained.".to_string()),
            });
        }
        repeated_tool_attempts.tracker_observation_warning_emitted = true;
        repeated_tool_attempts.reset_after_balancer_recovery();
        ctx.working_history.push(uni::Message::system(
            "Tracker observation repeated unchanged state. Use the latest result; do not list again. Perform the next incomplete task or its missing verification, then call task_tracker action=update with that item's index and actual status. If its work and checks already passed, mark it completed now. Do not repeat inspections or announce the same next action. One tool-enabled correction remains before a blocked handoff.".to_string(),
        ));
        return TurnHandlerOutcome::Continue;
    }

    // Planning keeps its generous 120-call ceiling for genuinely complex
    // research, but diverse empty searches and ref-only reads must converge
    // much earlier. Trigger a single tool-free synthesis pass at the first
    // adaptive threshold; recovery resets both turn-scoped counters.
    if ctx.is_planning_active()
        && !repeated_tool_attempts.planning_low_signal_synthesis_triggered
        && (repeated_tool_attempts.consecutive_low_signal_navigations >= PLANNING_CONSECUTIVE_LOW_SIGNAL_THRESHOLD
            || repeated_tool_attempts.total_low_signal_navigations >= PLANNING_TOTAL_LOW_SIGNAL_THRESHOLD)
    {
        let recovery_reason = format!(
            "Planning navigation produced {} consecutive and {} total low-signal results. Tools are disabled on the next pass. {PLANNING_SYNTHESIS_FORMAT_HINT}",
            repeated_tool_attempts.consecutive_low_signal_navigations,
            repeated_tool_attempts.total_low_signal_navigations
        );
        // Only claim the intervention when a pass was actually armed: a
        // recovery already pending from another guard or the blocked-tool
        // fuse would make the "[!]" line and duplicate reason a lie.
        if activate_navigation_recovery(ctx, recovery_reason.clone()) {
            repeated_tool_attempts.planning_low_signal_synthesis_triggered = true;
            ctx.renderer
                .line(
                    MessageStyle::Info,
                    "[!] Planning recovery: low-signal research reached the adaptive synthesis threshold.",
                )
                .unwrap_or(());
            ctx.working_history.push(uni::Message::system(recovery_reason));
        }
        return navigation_balancer_outcome(ctx, repeated_tool_attempts);
    }

    // NL2Repo-Bench: Navigation Loop Detection
    // Only trigger when there are actual repeated navigations (not just diverse exploration).
    // Reading 15 different files is exploration; re-reading the same 3 files 5x each is a loop.
    if repeated_tool_attempts.consecutive_navigations >= NAVIGATION_LOOP_THRESHOLD
        && repeated_tool_attempts.repeated_navigation_count() >= 3
    {
        repeated_tool_attempts.navigation_loop_recoveries =
            repeated_tool_attempts.navigation_loop_recoveries.saturating_add(1);
        let recurrence = repeated_tool_attempts.navigation_loop_recoveries;
        if ctx.is_approved_plan_execution() && recurrence >= 2 {
            let blocker = "Approved-plan execution stopped after repeated read-only/navigation actions made no implementation progress. The approved plan and task checklist were retained; retry from the pending step.";
            ctx.renderer.line(MessageStyle::Warning, blocker).unwrap_or(());
            ctx.working_history.push(uni::Message::system(blocker.to_string()));
            return TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(blocker.to_string()) });
        }
        let recovery_reason = format!(
            "Navigation loop detected after {} consecutive read/search steps (recurrence #{recurrence}). Tools are disabled on the next pass; summarize findings and propose the next concrete action.",
            repeated_tool_attempts.consecutive_navigations
        );
        if activate_navigation_recovery(ctx, recovery_reason.clone()) {
            ctx.renderer
                .line(MessageStyle::Warning, "[!] Navigation Loop: scheduling a recovery synthesis pass.")
                .unwrap_or(());
            ctx.working_history.push(uni::Message::system(format!(
                "{} {}",
                recovery_reason,
                navigation_loop_guidance(ctx.is_planning_active(), recurrence)
            )));
        }
        return navigation_balancer_outcome(ctx, repeated_tool_attempts);
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
    // Same-target directory listings (`ls`/`find`/`fd`) share one coarse
    // family per binary+root across flag variations, so three rescans of one
    // tree trip recovery even when no exact request repeats; scans of
    // distinct trees stay below the tripwire (legitimate exploration).
    // Planning gets a higher tripwire: it owns dedicated convergence guards
    // (6 consecutive / 10 total low-signal, 12-step nav synthesis) and a
    // generous research ceiling, so three successful listings are legitimate
    // exploration there rather than churn worth killing tools over.
    let planning_active = ctx.is_planning_active();
    let listing_trip_count = if planning_active {
        PLANNING_LISTING_LOOP_TRIP_COUNT
    } else {
        LISTING_LOOP_TRIP_COUNT
    };
    let repeated_listings = repeated_tool_attempts.max_coarse_listing_count();
    if (repeated_low_signal >= effective_repeat_limit || repeated_listings >= listing_trip_count)
        && repeated_tool_attempts.consecutive_navigations >= effective_repeat_limit
    {
        let churn = repeated_tool_attempts.dominant_churn();
        let recovery_reason = if planning_active {
            format!(
                "Repeated low-signal navigation calls reached the per-turn fast-path cap ({effective_repeat_limit}). Tools are disabled on the next pass. {PLANNING_SYNTHESIS_FORMAT_HINT}{}",
                churn_reason_note(churn.as_ref())
            )
        } else {
            format!(
                "Repeated low-signal navigation calls reached the per-turn fast-path cap ({effective_repeat_limit}). Tools are disabled on the next pass; summarize only from collected evidence.{}",
                churn_reason_note(churn.as_ref())
            )
        };
        let armed = arm_and_announce_early_recovery(ctx, recovery_reason, churn.as_ref()).await;
        if armed && planning_active {
            repeated_tool_attempts.planning_low_signal_synthesis_triggered = true;
        }
        return navigation_balancer_outcome(ctx, repeated_tool_attempts);
    }

    // Execution-mode total low-signal guard: diverse churn (a new query each
    // time) never trips the per-family fast-path above and previously ran
    // until the navigation-loop guard (15 steps) or the final balancer
    // window. The counter's window resets on any mutation or verification,
    // so this fires only for churn uninterrupted by productive work.
    if !planning_active
        && (!repeated_tool_attempts.execution_total_low_signal_triggered
            || ctx.session_stats.navigation_recovery
                == crate::agent::runloop::unified::state::NavigationRecovery::Consumed)
        && repeated_tool_attempts.total_low_signal_navigations >= EXECUTION_TOTAL_LOW_SIGNAL_THRESHOLD
    {
        let churn = repeated_tool_attempts.dominant_churn();
        let recovery_reason = format!(
            "Diverse low-signal navigation reached {} outcomes without a mutation or verification resetting the window. Tools are disabled on the next pass; summarize only from collected evidence.{}",
            repeated_tool_attempts.total_low_signal_navigations,
            churn_reason_note(churn.as_ref())
        );
        let armed = arm_and_announce_early_recovery(ctx, recovery_reason, churn.as_ref()).await;
        if armed {
            repeated_tool_attempts.execution_total_low_signal_triggered = true;
        }
        return navigation_balancer_outcome(ctx, repeated_tool_attempts);
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
        let churn = repeated_tool_attempts.dominant_churn();
        let recovery_reason = if planning_active {
            format!(
                "Turn balancer detected repeated low-signal tool churn. Tools are disabled on the next pass. {PLANNING_SYNTHESIS_FORMAT_HINT}{}",
                churn_reason_note(churn.as_ref())
            )
        } else {
            format!(
                "Turn balancer detected repeated low-signal tool churn. Tools are disabled on the next pass; summarize only from collected evidence.{}",
                churn_reason_note(churn.as_ref())
            )
        };
        if activate_navigation_recovery(ctx, recovery_reason.clone()) {
            if planning_active {
                repeated_tool_attempts.planning_low_signal_synthesis_triggered = true;
            }
            ctx.renderer
                .line(
                    MessageStyle::Info,
                    &format!(
                        "[!] Turn balancer: repeated low-signal calls detected{}; scheduling a final recovery pass.",
                        churn_line_label(churn.as_ref())
                    ),
                )
                .unwrap_or(());
            ctx.working_history.push(uni::Message::system(recovery_reason));
            // Record in ledger
            {
                let mut ledger = ctx.decision_ledger.write().await;
                ledger.record_decision(
                    "Turn balancer: Recovery intervention".to_string(),
                    vtcode_core::core::decision_tracker::Action::Response {
                        content: "Low-signal churn detected; a final tool-free recovery pass was scheduled."
                            .to_string(),
                        response_type: vtcode_core::core::decision_tracker::ResponseType::ContextSummary,
                    },
                    None,
                );
            }
        }
        return navigation_balancer_outcome(ctx, repeated_tool_attempts);
    }

    TurnHandlerOutcome::Continue
}

fn navigation_balancer_outcome(
    ctx: &TurnProcessingContext<'_>,
    tracker: &mut crate::agent::runloop::unified::turn::tool_outcomes::helpers::LoopTracker,
) -> TurnHandlerOutcome {
    if ctx.session_stats.navigation_recovery.blocked() {
        return TurnHandlerOutcome::Break(TurnLoopResult::Blocked {
            reason: Some(crate::agent::runloop::unified::state::NAVIGATION_RECOVERY_BLOCK_REASON.to_string()),
        });
    }
    apply_balancer_recovery(tracker)
}

fn apply_balancer_recovery(
    repeated_tool_attempts: &mut crate::agent::runloop::unified::turn::tool_outcomes::helpers::LoopTracker,
) -> TurnHandlerOutcome {
    repeated_tool_attempts.reset_after_balancer_recovery();
    TurnHandlerOutcome::Continue
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vtcode_core::config::constants::tools as tool_names;

    use super::{
        apply_balancer_recovery, is_readonly_signature, navigation_loop_guidance, validate_tool_args_security,
    };
    use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
    use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnLoopResult};
    use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{
        ANTI_BLIND_EDITING_DIRECTIVE, BLIND_EDITING_THRESHOLD, EXECUTION_TOTAL_LOW_SIGNAL_THRESHOLD, LoopTracker,
        NAVIGATION_LOOP_THRESHOLD, PLANNING_CONSECUTIVE_LOW_SIGNAL_THRESHOLD, PLANNING_TOTAL_LOW_SIGNAL_THRESHOLD,
        update_repetition_tracker,
    };
    use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;

    #[test]
    fn readonly_signature_handles_file_and_code_search_signatures() {
        assert!(is_readonly_signature(r#"read file:{"path":"README.md"}"#));
        assert!(is_readonly_signature(
            r#"code_search:{"query":"LLMStreamEvent","path":"crates/codegen/vtcode-core/src/llm/providers/anthropic/api.rs"}"#
        ));
    }

    #[test]
    fn readonly_signature_treats_safe_exec_runs_as_readonly() {
        assert!(is_readonly_signature(r#"command_session:{"action":"run","command":"cargo check"}"#));
    }

    #[test]
    fn readonly_signature_fast_path_accepts_ro_tag() {
        assert!(is_readonly_signature("code_search:ro:{\"query\":\"Widget\"}"));
    }

    #[test]
    fn readonly_signature_fast_path_rejects_rw_tag() {
        assert!(!is_readonly_signature("file_operation:rw:len42-fnv1234abcd"));
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

        let failures = validate_tool_args_security(tool_names::EDIT_FILE, &args, None, None).unwrap();
        assert!(failures.iter().any(|msg| msg.contains("old_str")));
        assert!(failures.iter().any(|msg| msg.contains("new_str")));
    }

    #[test]
    fn validate_command_session_args_without_registry_reports_single_missing_command() {
        let failures = validate_tool_args_security(tool_names::UNIFIED_EXEC, &json!({"action": "run"}), None, None)
            .expect("missing command should fail");

        assert_eq!(failures, vec!["Missing required argument: command".to_string()]);
    }

    #[test]
    fn validate_command_session_args_without_registry_rejects_missing_action() {
        let failures = validate_tool_args_security(tool_names::UNIFIED_EXEC, &json!({}), None, None)
            .expect("missing action should fail");

        assert_eq!(
            failures,
            vec!["Invalid arguments: missing action; provide `action` or inferable exec arguments".to_string()]
        );
    }

    #[test]
    fn navigation_loop_guidance_mentions_task_tracker_in_planning_workflow() {
        let guidance = navigation_loop_guidance(true, 1);
        assert!(guidance.contains("task_tracker"));
    }

    fn assert_recovery_expires_at_fresh_turn(history: &[vtcode_core::llm::provider::Message]) {
        use crate::agent::runloop::unified::turn::turn_loop_helpers::restore_fresh_turn_tool_guidance;

        let mut restored = history.to_vec();
        restore_fresh_turn_tool_guidance(&mut restored, false);
        assert_eq!(restored.len(), history.len() + 1, "the guard's emitted directive must be superseded");
        restore_fresh_turn_tool_guidance(&mut restored, false);
        assert_eq!(restored.len(), history.len() + 1, "restoration must not duplicate itself");
    }

    #[test]
    fn navigation_loop_guidance_uses_generic_text_outside_planning_workflow() {
        let guidance = navigation_loop_guidance(false, 1);
        assert!(guidance.contains("read/search"));
        assert!(!guidance.contains("task_tracker"));
    }

    #[test]
    fn navigation_loop_guidance_escalates_on_repetition() {
        let guidance = navigation_loop_guidance(false, 2);
        assert!(guidance.contains("navigation-loop guard has fired again"));
        assert!(guidance.contains("one blocking question"));
        assert_ne!(guidance, navigation_loop_guidance(false, 1));
    }

    #[test]
    fn balancer_recovery_continues_and_preserves_mutation_pressure() {
        let mut tracker = LoopTracker::new();
        let sig = r#"command_session:{"action":"run","command":"cargo test"}"#.to_string();
        tracker.record(sig.clone());
        tracker.record(sig.clone());
        tracker.record(sig);
        tracker.consecutive_mutations = 3;
        tracker.consecutive_navigations = 5;

        let outcome = apply_balancer_recovery(&mut tracker);

        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert_eq!(tracker.max_count_filtered(|_| false), 0);
        assert_eq!(tracker.consecutive_mutations, 3);
        assert_eq!(tracker.consecutive_navigations, 0);
    }

    #[tokio::test]
    async fn planning_recovery_triggers_at_exact_consecutive_low_signal_threshold() {
        let mut backing = TestTurnProcessingBacking::new(120).await;
        backing.activate_planning_for_test();
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        tracker.consecutive_low_signal_navigations = PLANNING_CONSECUTIVE_LOW_SIGNAL_THRESHOLD;
        tracker.total_low_signal_navigations = PLANNING_CONSECUTIVE_LOW_SIGNAL_THRESHOLD;

        let outcome = super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 120, 3).await;

        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(ctx.is_recovery_active());
        assert_eq!(tracker.consecutive_low_signal_navigations, 0);
        assert_eq!(tracker.total_low_signal_navigations, 0);
        assert!(ctx.working_history.iter().any(|message| {
            message.content.as_text().contains("adaptive synthesis threshold")
                || message.content.as_text().contains("synthesize the plan")
                || message.content.as_text().contains("<proposed_plan>")
        }));
        assert_recovery_expires_at_fresh_turn(ctx.working_history);
    }

    #[tokio::test]
    async fn anti_blind_warning_keeps_verification_pending_until_a_check_runs() {
        let mut backing = TestTurnProcessingBacking::new(8).await;
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        tracker.consecutive_mutations = BLIND_EDITING_THRESHOLD;

        let outcome = super::handle_turn_balancer(&mut ctx, 1, &mut tracker, 8, 3).await;

        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert_eq!(tracker.consecutive_mutations, BLIND_EDITING_THRESHOLD);
        assert!(
            ctx.working_history
                .iter()
                .any(|message| { message.content.as_text().contains(ANTI_BLIND_EDITING_DIRECTIVE) })
        );
    }

    #[tokio::test]
    async fn unchanged_tracker_polls_get_one_correction_then_terminal_handoff() {
        let mut backing = TestTurnProcessingBacking::new(20).await;
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        for round in 0..2 {
            let status = if round == 0 {
                ToolExecutionStatus::Success {
                    output: json!({"status":"ok", "checklist":{"items":[{"index":3,"status":"in_progress"}]}}),
                    stdout: None,
                    modified_files: vec![],
                    command_success: true,
                }
            } else {
                ToolExecutionStatus::Failure {
                    error: vtcode_core::tools::registry::ToolExecutionError::new(
                        tool_names::TASK_TRACKER,
                        vtcode_core::tools::registry::ToolErrorType::PolicyViolation,
                        "identical tracker polls blocked",
                    ),
                }
            };
            let outcome = ToolPipelineOutcome::from_status(status);
            for _ in 0..2 {
                update_repetition_tracker(&mut tracker, &outcome, tool_names::TASK_TRACKER, &json!({"action":"list"}));
            }
            let result = super::handle_turn_balancer(&mut ctx, 2 + round * 2, &mut tracker, 20, 3).await;
            if round == 0 {
                assert!(matches!(result, TurnHandlerOutcome::Continue));
                assert!(!ctx.is_recovery_active(), "correction must retain usable tools");
                assert!(tracker.tracker_observation_warning_emitted);
            } else {
                let TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) }) = result else {
                    panic!("unchanged polls must not schedule another correction");
                };
                assert!(!crate::agent::runloop::unified::turn::tool_outcomes::helpers::tracker_auto_continue_is_recoverable_block(Some(&reason)));
            }
        }
    }

    #[tokio::test]
    async fn planning_recovery_triggers_at_exact_total_low_signal_threshold() {
        let mut backing = TestTurnProcessingBacking::new(120).await;
        backing.activate_planning_for_test();
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        tracker.consecutive_low_signal_navigations = 1;
        tracker.total_low_signal_navigations = PLANNING_TOTAL_LOW_SIGNAL_THRESHOLD;

        let outcome = super::handle_turn_balancer(&mut ctx, 10, &mut tracker, 120, 3).await;

        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(ctx.is_recovery_active());
        assert_eq!(tracker.consecutive_low_signal_navigations, 0);
        assert_eq!(tracker.total_low_signal_navigations, 0);
    }

    #[tokio::test]
    async fn adaptive_low_signal_threshold_does_not_change_non_planning_turns() {
        let mut backing = TestTurnProcessingBacking::new(120).await;
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        tracker.consecutive_low_signal_navigations = PLANNING_CONSECUTIVE_LOW_SIGNAL_THRESHOLD;
        tracker.total_low_signal_navigations = PLANNING_TOTAL_LOW_SIGNAL_THRESHOLD;

        let outcome = super::handle_turn_balancer(&mut ctx, 1, &mut tracker, 120, 20).await;

        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(!ctx.is_recovery_active());
    }

    #[tokio::test]
    async fn adaptive_low_signal_synthesis_triggers_only_once_per_turn() {
        let mut backing = TestTurnProcessingBacking::new(120).await;
        backing.activate_planning_for_test();
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        tracker.consecutive_low_signal_navigations = PLANNING_CONSECUTIVE_LOW_SIGNAL_THRESHOLD;
        tracker.total_low_signal_navigations = PLANNING_TOTAL_LOW_SIGNAL_THRESHOLD;

        let first = super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 120, 3).await;
        assert!(matches!(first, TurnHandlerOutcome::Continue));
        assert!(tracker.planning_low_signal_synthesis_triggered);
        assert!(
            ctx.working_history.iter().any(|message| {
                let text = message.content.as_text();
                text.contains("<proposed_plan>") && text.contains("Action -> files")
            }),
            "low-signal recovery must instruct plan-format synthesis"
        );

        tracker.consecutive_low_signal_navigations = PLANNING_CONSECUTIVE_LOW_SIGNAL_THRESHOLD;
        tracker.total_low_signal_navigations = PLANNING_TOTAL_LOW_SIGNAL_THRESHOLD;
        let second = super::handle_turn_balancer(&mut ctx, 12, &mut tracker, 120, usize::MAX).await;
        assert!(matches!(second, TurnHandlerOutcome::Continue));
        assert_eq!(tracker.consecutive_low_signal_navigations, PLANNING_CONSECUTIVE_LOW_SIGNAL_THRESHOLD);
        assert_eq!(tracker.total_low_signal_navigations, PLANNING_TOTAL_LOW_SIGNAL_THRESHOLD);
    }

    #[tokio::test]
    async fn many_productive_planning_previews_do_not_force_recovery() {
        let mut backing = TestTurnProcessingBacking::new(120).await;
        backing.activate_planning_for_test();
        let mut ctx = backing.turn_processing_context();
        for index in 0..24 {
            ctx.push_tool_response(
                format!("call-{index}"),
                Some("exec_command"),
                format!("evidence-{index} {}", "x".repeat(8_000)),
            );
        }
        let mut tracker = LoopTracker::new();
        let result = super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 120, usize::MAX).await;
        assert!(matches!(result, TurnHandlerOutcome::Continue));
        assert!(!ctx.is_recovery_active());
        assert!(!tracker.planning_low_signal_synthesis_triggered);
        assert!(
            ctx.working_history
                .iter()
                .any(|message| message.content.as_text().contains("evidence-23"))
        );
        assert!(
            !ctx.working_history
                .iter()
                .any(|message| message.content.as_text().contains("preview_budget_exhausted"))
        );
    }

    #[tokio::test]
    async fn planning_one_repeated_successful_inspection_remains_productive() {
        let mut backing = TestTurnProcessingBacking::new(120).await;
        backing.activate_planning_for_test();
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"stdout":"useful source"}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });
        let calls = [
            ("ls -la src", None),
            ("cat src/main.rs", None),
            ("sed -n '80,140p' src/lib.rs", None),
            ("cat src/startup.rs", None),
            ("ls -la src/startup", None),
            ("find src -name 'startup*' -o -name 'main_helpers*'", None),
            ("rg -n 'resolve_startup_context' src/", None),
            ("sed -n '100,220p' src/main_helpers/bootstrap.rs", None),
            ("rg -n 'from_cli_args' src/startup/mod.rs", None),
            ("sed -n '278,420p' src/startup/mod.rs", None),
            ("sed -n '278,340p' src/startup/mod.rs", None),
            ("sed -n '278,420p' src/startup/mod.rs", Some(8000)),
        ];

        for (command, max_output_tokens) in calls {
            let mut args = json!({"cmd": command, "command": command, "action": "run"});
            if let Some(max_output_tokens) = max_output_tokens {
                args["max_output_tokens"] = json!(max_output_tokens);
            }
            update_repetition_tracker(&mut tracker, &success, tool_names::EXEC_COMMAND, &args);
        }

        assert_eq!(tracker.consecutive_navigations, calls.len());
        assert_eq!(tracker.repeated_navigation_count(), 1);
        assert_eq!(tracker.consecutive_low_signal_navigations, 0);

        let outcome = super::handle_turn_balancer(&mut ctx, 12, &mut tracker, 120, 3).await;

        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(!ctx.is_recovery_active());
        assert_eq!(tracker.consecutive_navigations, calls.len());
        assert!(!tracker.planning_low_signal_synthesis_triggered);
    }

    #[tokio::test]
    async fn planning_diverse_inspection_remains_productive() {
        let mut backing = TestTurnProcessingBacking::new(120).await;
        backing.activate_planning_for_test();
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"stdout":"useful source"}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });
        let commands = [
            "ls -la src",
            "cat src/main.rs",
            "sed -n '80,140p' src/lib.rs",
            "cat src/startup.rs",
            "ls -la src/startup",
            "find src -name 'startup*' -o -name 'main_helpers*'",
            "rg -n 'resolve_startup_context' src/",
            "sed -n '100,220p' src/main_helpers/bootstrap.rs",
            "rg -n 'from_cli_args' src/startup/mod.rs",
            "sed -n '278,420p' src/startup/mod.rs",
            "sed -n '278,340p' src/startup/mod.rs",
            "cat src/startup/mod.rs",
        ];

        for command in commands {
            update_repetition_tracker(
                &mut tracker,
                &success,
                tool_names::EXEC_COMMAND,
                &json!({"cmd": command, "command": command, "action": "run"}),
            );
        }

        assert_eq!(tracker.consecutive_navigations, commands.len());
        assert_eq!(tracker.repeated_navigation_count(), 0);

        let outcome = super::handle_turn_balancer(&mut ctx, 12, &mut tracker, 120, 3).await;

        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(!ctx.is_recovery_active());
        assert_eq!(tracker.consecutive_navigations, commands.len());
    }

    #[tokio::test]
    async fn planning_genuine_repeated_navigation_still_recovers() {
        let mut backing = TestTurnProcessingBacking::new(120).await;
        backing.activate_planning_for_test();
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"stdout":"useful source"}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });
        for index in 0..NAVIGATION_LOOP_THRESHOLD {
            let command = format!("cat src/{}.rs", index % 4);
            update_repetition_tracker(
                &mut tracker,
                &success,
                tool_names::EXEC_COMMAND,
                &json!({"cmd": command, "command": command, "action": "run"}),
            );
        }
        assert!(tracker.repeated_navigation_count() >= 3);
        assert_eq!(tracker.consecutive_low_signal_navigations, 0);

        let outcome = super::handle_turn_balancer(&mut ctx, NAVIGATION_LOOP_THRESHOLD, &mut tracker, 120, 3).await;
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(ctx.is_recovery_active());
        assert_eq!(tracker.consecutive_navigations, 0);
    }

    #[tokio::test]
    async fn oversized_preview_does_not_trigger_synthesis_outside_planning() {
        use vtcode_config::constants::output_limits::TURN_PREVIEW_BUDGET_BYTES;

        let mut backing = TestTurnProcessingBacking::new(120).await;
        let mut ctx = backing.turn_processing_context();
        ctx.push_tool_response("call-blind", Some("exec_command"), "x".repeat(TURN_PREVIEW_BUDGET_BYTES + 1));
        assert!(!ctx.harness_state.model_visible_preview_budget_exhausted());

        let mut tracker = LoopTracker::new();
        let outcome = super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 120, 3).await;
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(!ctx.is_recovery_active());
    }

    #[tokio::test]
    async fn build_listing_loop_converges_on_third_same_base_scan() {
        // Rehearsal of the observed build-mode listing loop (`ls src/`,
        // `find …`, `ls src`, `ls -1 src`, …): argument variations keep every
        // exact-request counter quiet, so the third same-base listing must
        // schedule recovery instead of burning the turn tool-call budget.
        let mut backing = TestTurnProcessingBacking::new(120).await;
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"stdout": "src\nsrc-tauri"}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });
        for command in ["ls src/", "find src -maxdepth 1 -type d", "ls src"] {
            update_repetition_tracker(
                &mut tracker,
                &success,
                tool_names::EXEC_COMMAND,
                &json!({"cmd": command, "command": command, "action": "run"}),
            );
        }
        assert_eq!(tracker.max_coarse_listing_count(), 2);
        assert_eq!(tracker.repeated_navigation_count(), 0);

        let outcome = super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 120, 3).await;
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(!ctx.is_recovery_active());

        update_repetition_tracker(
            &mut tracker,
            &success,
            tool_names::EXEC_COMMAND,
            &json!({"cmd": "ls -1 src", "command": "ls -1 src", "action": "run"}),
        );
        assert_eq!(tracker.max_coarse_listing_count(), 3);

        let outcome = super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 120, 3).await;
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(ctx.is_recovery_active());
        assert!(
            ctx.working_history
                .iter()
                .any(|message| { message.content.as_text().contains("summarize only from collected evidence") })
        );
    }

    #[tokio::test]
    async fn build_search_triplets_do_not_trip_listing_convergence() {
        // Distinct `rg` queries are semantically distinct questions, so three
        // of them must not schedule recovery. They carry no coarse inspection
        // family (`rg`/`grep` are excluded from coarse grouping because their
        // first positional is the search pattern, not a path root).
        let mut backing = TestTurnProcessingBacking::new(120).await;
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"stdout": "src/lib.rs:1:hit"}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });
        for query in ["rg -n 'alpha' src/", "rg -n 'beta' src/", "rg -n 'gamma' src/"] {
            update_repetition_tracker(
                &mut tracker,
                &success,
                tool_names::EXEC_COMMAND,
                &json!({"cmd": query, "command": query, "action": "run"}),
            );
        }
        assert_eq!(tracker.max_coarse_listing_count(), 0);

        let outcome = super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 120, 3).await;
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(!ctx.is_recovery_active());
    }

    #[tokio::test]
    async fn same_pattern_search_repeats_do_not_trip_early_recovery() {
        // Regression for turn_1303/turn_1304 (`exec::inspection::grep::enum ×5`)
        // and turn_1291 (`exec::inspection::rg::pub ×5`): five distinct
        // successful searches sharing one pattern across different files/flags
        // are legitimate research. With `rg`/`grep` excluded from coarse
        // grouping they must not enter the low-signal ledger and must not
        // schedule early recovery.
        let mut backing = TestTurnProcessingBacking::new(120).await;
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"stdout": "src/lib.rs:1:hit"}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });
        for query in [
            "rg -n 'pub enum Commands' src/ -A 40",
            "rg -n 'pub enum Commands' crates/codegen/vtcode-core/src/cli/args/mod.rs -A 50",
            "rg -n 'pub enum Commands' crates/codegen/vtcode-core/src/cli/args/mod.rs -A 600",
            "rg -n 'pub enum Provider|Gemini|OpenAI' crates/codegen/vtcode-llm/src",
            "rg -n 'pub enum SecretCommand|Add|List' crates/codegen/vtcode-core/src/cli/args/secret.rs",
        ] {
            update_repetition_tracker(
                &mut tracker,
                &success,
                tool_names::EXEC_COMMAND,
                &json!({"cmd": query, "command": query, "action": "run"}),
            );
        }
        assert_eq!(tracker.max_coarse_listing_count(), 0);
        assert_eq!(tracker.max_low_signal_count(), 0);
        assert_eq!(tracker.dominant_churn(), None);

        let outcome = super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 120, 3).await;
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(!ctx.is_recovery_active());
    }

    #[tokio::test]
    async fn planning_empty_search_pipelines_schedule_bounded_synthesis() {
        let mut backing = TestTurnProcessingBacking::new(120).await;
        backing.activate_planning_for_test();
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        let empty = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"exit_code": 0, "output": "", "total_output_bytes": 0}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });
        for index in 0..PLANNING_CONSECUTIVE_LOW_SIGNAL_THRESHOLD {
            let command = format!("rg --files --hidden | rg 'optional-lint-{index}' | head -10");
            update_repetition_tracker(&mut tracker, &empty, tool_names::EXEC_COMMAND, &json!({"cmd": command}));
            if index + 1 < PLANNING_CONSECUTIVE_LOW_SIGNAL_THRESHOLD {
                let outcome = super::handle_turn_balancer(&mut ctx, index as usize + 1, &mut tracker, 120, 3).await;
                assert!(matches!(outcome, TurnHandlerOutcome::Continue));
                assert!(!ctx.is_recovery_active());
            }
        }
        assert_eq!(tracker.max_low_signal_count(), 1);
        let outcome = super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 120, 3).await;
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(ctx.is_recovery_active());
        assert!(tracker.planning_low_signal_synthesis_triggered);
        assert!(ctx.working_history.iter().any(|message| {
            let text = message.content.as_text();
            text.contains("<proposed_plan>") && text.contains("Action -> files")
        }));
    }

    #[tokio::test]
    async fn planning_listing_triplet_stays_below_early_recovery() {
        // Planning owns dedicated convergence guards (6/10 low-signal, 12-step
        // nav synthesis), so three successful same-base listings are legitimate
        // exploration and must not trip the generic fast-path. The fifth does.
        let mut backing = TestTurnProcessingBacking::new(120).await;
        backing.activate_planning_for_test();
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"stdout": "src\nsrc-tauri"}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });
        for command in ["ls src/", "ls src", "ls -1 src"] {
            update_repetition_tracker(
                &mut tracker,
                &success,
                tool_names::EXEC_COMMAND,
                &json!({"cmd": command, "command": command, "action": "run"}),
            );
        }
        assert_eq!(tracker.max_coarse_listing_count(), 3);

        let outcome = super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 120, 3).await;
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(!ctx.is_recovery_active(), "planning triplet must not schedule early recovery");

        for command in ["ls -R src", "ls -lh src"] {
            update_repetition_tracker(
                &mut tracker,
                &success,
                tool_names::EXEC_COMMAND,
                &json!({"cmd": command, "command": command, "action": "run"}),
            );
        }
        assert_eq!(tracker.max_coarse_listing_count(), 5);

        let outcome = super::handle_turn_balancer(&mut ctx, 8, &mut tracker, 120, 3).await;
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(ctx.is_recovery_active());
        assert!(
            ctx.working_history.iter().any(|message| {
                let text = message.content.as_text();
                text.contains("<proposed_plan>") && text.contains("Action -> files")
            }),
            "planning listing recovery must instruct plan-format synthesis"
        );
    }

    #[tokio::test]
    async fn navigation_loop_schedules_recovery_and_progress_only_recovery_text_completes() {
        let mut backing = TestTurnProcessingBacking::new(8).await;
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        tracker.consecutive_navigations = NAVIGATION_LOOP_THRESHOLD;

        let balancer_outcome = super::handle_turn_balancer(&mut ctx, 1, &mut tracker, 8, 3).await;
        assert!(matches!(balancer_outcome, TurnHandlerOutcome::Continue));
        assert_eq!(tracker.consecutive_navigations, 0);
        assert!(ctx.is_recovery_active());
        assert!(
            ctx.working_history
                .iter()
                .any(|message| { message.content.as_text().contains("Navigation loop detected") })
        );
        assert_recovery_expires_at_fresh_turn(ctx.working_history);
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

        assert!(matches!(recovery_outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
        assert!(!ctx.is_recovery_active());
    }

    #[tokio::test]
    async fn low_signal_search_churn_schedules_recovery_and_progress_only_recovery_text_completes() {
        let mut backing = TestTurnProcessingBacking::new(8).await;
        backing.set_loop_limit(tool_names::CODE_SEARCH, 2);
        let seeded_args = json!({"query":"Result","path":"src"});
        assert!(backing.record_tool_call(tool_names::CODE_SEARCH, &seeded_args).is_none());
        let _ = backing.record_tool_call(tool_names::CODE_SEARCH, &seeded_args);
        let warning = backing.record_tool_call(tool_names::CODE_SEARCH, &seeded_args);
        assert!(warning.is_some());
        assert!(backing.is_hard_limit_exceeded(tool_names::CODE_SEARCH));
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"results": []}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });

        let same_args = json!({"query":"Result","path":"src"});
        update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);
        update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);
        update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);

        let balancer_outcome = super::handle_turn_balancer(&mut ctx, 4, &mut tracker, 4, 3).await;
        assert!(matches!(balancer_outcome, TurnHandlerOutcome::Continue));
        assert!(ctx.is_recovery_active());
        assert!(ctx.consume_recovery_pass());

        let recovery_outcome = ctx
            .handle_text_response("Let me try a narrower search next.".to_string(), Vec::new(), None, None, false)
            .await
            .expect("recovery response should be handled");

        assert!(matches!(recovery_outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
        assert!(!ctx.is_recovery_active());
        assert!(backing.is_hard_limit_exceeded(tool_names::CODE_SEARCH));
    }

    #[tokio::test]
    async fn early_low_signal_search_churn_schedules_recovery_before_turn_window() {
        let mut backing = TestTurnProcessingBacking::new(20).await;
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"results": []}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });

        let same_args = json!({"query":"Result","path":"src"});
        update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);
        update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);
        update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);

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
        assert_recovery_expires_at_fresh_turn(ctx.working_history);
    }

    #[tokio::test]
    async fn early_balancer_recovery_is_plan_aware_in_planning_workflow() {
        let mut backing = TestTurnProcessingBacking::new(20).await;
        backing.activate_planning_for_test();
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"results": []}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });

        let same_args = json!({"query":"Result","path":"src"});
        update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);
        update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);
        update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);

        let balancer_outcome = super::handle_turn_balancer(&mut ctx, 3, &mut tracker, 20, 3).await;
        assert!(matches!(balancer_outcome, TurnHandlerOutcome::Continue));
        assert!(ctx.is_recovery_active());
        assert!(tracker.planning_low_signal_synthesis_triggered);
        assert!(
            ctx.working_history
                .iter()
                .any(|message| { message.content.as_text().contains("<proposed_plan>") })
        );
    }

    #[tokio::test]
    async fn early_balancer_recovery_stays_generic_outside_planning_workflow() {
        let mut backing = TestTurnProcessingBacking::new(20).await;
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"results": []}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });

        let same_args = json!({"query":"Other","path":"src"});
        update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);
        update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);
        update_repetition_tracker(&mut tracker, &miss, tool_names::CODE_SEARCH, &same_args);

        let balancer_outcome = super::handle_turn_balancer(&mut ctx, 3, &mut tracker, 20, 3).await;
        assert!(matches!(balancer_outcome, TurnHandlerOutcome::Continue));
        assert!(ctx.is_recovery_active());
        assert!(!tracker.planning_low_signal_synthesis_triggered);
        assert!(ctx.working_history.iter().any(|message| {
            let text = message.content.as_text();
            text.contains("summarize only from collected evidence") && !text.contains("<proposed_plan>")
        }));
    }

    #[tokio::test]
    async fn early_balancer_trips_on_same_root_listings_below_low_signal_limit() {
        // With a lenient repeat limit (5), three same-binary same-root
        // listings stay below the low-signal cap but still trip recovery
        // through the coarse-listing branch once consecutive navigations
        // reach the limit. The armed reason names the looped family.
        let mut backing = TestTurnProcessingBacking::new(20).await;
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });
        let hit = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"results": [{"path": "src/main.rs"}]}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });

        for command in ["ls src", "ls -1 src", "ls src/"] {
            update_repetition_tracker(&mut tracker, &success, tool_names::EXEC_COMMAND, &json!({"cmd":command}));
        }
        for query in ["TurnLoop", "LoopTracker"] {
            update_repetition_tracker(
                &mut tracker,
                &hit,
                tool_names::CODE_SEARCH,
                &json!({"query":query,"path":"src"}),
            );
        }
        assert!(tracker.max_low_signal_count() < 5);

        let balancer_outcome = super::handle_turn_balancer(&mut ctx, 5, &mut tracker, 20, 5).await;
        assert!(matches!(balancer_outcome, TurnHandlerOutcome::Continue));
        assert!(ctx.is_recovery_active());
        let reason = ctx.recovery_reason().unwrap_or_default();
        assert!(reason.contains("Top churn: exec::inspection::ls::src ×3."));
    }

    #[tokio::test]
    async fn diverse_root_listings_stay_below_early_recovery() {
        // Scans of distinct trees are legitimate exploration: three
        // same-binary listings of different roots must not schedule the early
        // recovery pass, because the coarse family is scoped by search root.
        let mut backing = TestTurnProcessingBacking::new(20).await;
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });

        for command in ["ls src", "ls crates", "ls tests"] {
            update_repetition_tracker(&mut tracker, &success, tool_names::EXEC_COMMAND, &json!({"cmd":command}));
        }
        assert_eq!(tracker.max_coarse_listing_count(), 1);

        let outcome = super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 20, 3).await;
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(!ctx.is_recovery_active());
    }

    #[tokio::test]
    async fn balancer_does_not_reclaim_an_already_pending_recovery() {
        // When the blocked-tool fuse armed a recovery earlier in the same
        // batch, the balancer condition can still trip on the same tracker
        // state. It must not announce a second "scheduling" pass or overwrite
        // the armed reason — `activate_recovery` reports the no-op and the
        // messaging stays silent.
        let mut backing = TestTurnProcessingBacking::new(120).await;
        let mut ctx = backing.turn_processing_context();
        ctx.activate_recovery("blocked-tool recovery");
        let mut tracker = LoopTracker::new();
        let success = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });
        for command in ["ls src", "ls -1 src", "ls src/"] {
            update_repetition_tracker(&mut tracker, &success, tool_names::EXEC_COMMAND, &json!({"cmd":command}));
        }

        let outcome = super::handle_turn_balancer(&mut ctx, 6, &mut tracker, 120, 3).await;
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(ctx.is_recovery_active());
        assert_eq!(ctx.recovery_reason(), Some("blocked-tool recovery"));
        assert!(
            !ctx.working_history
                .iter()
                .any(|message| message.content.as_text().contains("summarize only from collected evidence"))
        );
        assert!(!ctx.working_history.iter().any(|message| {
            message
                .content
                .as_text()
                .contains("Repeated low-signal navigation calls reached")
        }));
    }

    #[tokio::test]
    async fn retained_search_results_converge_before_the_loop_budget() {
        let mut backing = TestTurnProcessingBacking::new(32).await;
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        let hit = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"results":[{"result_type":"text","path":"README.md","line":10,"snippet":"## Overview"}]}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });
        for step in 0..4 {
            update_repetition_tracker(
                &mut tracker,
                &hit,
                tool_names::CODE_SEARCH,
                &json!({"query":format!("query{step}"),"path":"README.md"}),
            );
            let outcome = super::handle_turn_balancer(&mut ctx, step + 1, &mut tracker, 60, 3).await;
            assert!(matches!(outcome, TurnHandlerOutcome::Continue));
            assert_eq!(ctx.is_recovery_active(), step == 3);
        }
        assert!(ctx.recovery_reason().unwrap().contains("low-signal"));
        assert!(!tracker.verification_is_pending());
        assert_eq!(tracker.fix_edits_remaining, 0);
    }

    #[tokio::test]
    async fn execution_total_low_signal_guard_trips_at_threshold() {
        // Diverse empty searches (a new query each time) never trip the
        // per-family fast-path; the execution-mode total guard converges the
        // turn at 12 low-signal outcomes without productive work.
        let mut backing = TestTurnProcessingBacking::new(40).await;
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"results":[]}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });

        for step in 0..(EXECUTION_TOTAL_LOW_SIGNAL_THRESHOLD - 1) {
            update_repetition_tracker(
                &mut tracker,
                &miss,
                tool_names::CODE_SEARCH,
                &json!({"query": format!("q{step}"), "path": "src"}),
            );
        }
        let outcome = super::handle_turn_balancer(&mut ctx, 12, &mut tracker, 40, 3).await;
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(!ctx.is_recovery_active());

        update_repetition_tracker(
            &mut tracker,
            &miss,
            tool_names::CODE_SEARCH,
            &json!({"query": "q-final", "path": "src"}),
        );
        let outcome = super::handle_turn_balancer(&mut ctx, 13, &mut tracker, 40, 3).await;
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(ctx.is_recovery_active());
        let reason = ctx.recovery_reason().unwrap_or_default();
        assert!(reason.contains("Diverse low-signal navigation reached 12"));
        assert_recovery_expires_at_fresh_turn(ctx.working_history);
    }

    #[tokio::test]
    async fn execution_total_low_signal_guard_fires_once_per_turn() {
        let mut backing = TestTurnProcessingBacking::new(40).await;
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        tracker.execution_total_low_signal_triggered = true;
        let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"results":[]}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });

        for step in 0..(EXECUTION_TOTAL_LOW_SIGNAL_THRESHOLD + 3) {
            update_repetition_tracker(
                &mut tracker,
                &miss,
                tool_names::CODE_SEARCH,
                &json!({"query": format!("r{step}"), "path": "src"}),
            );
        }

        let outcome = super::handle_turn_balancer(&mut ctx, 16, &mut tracker, 40, 3).await;
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(!ctx.is_recovery_active());
    }

    #[tokio::test]
    async fn execution_total_low_signal_window_resets_on_mutation() {
        // A productive mutation resets the low-signal window, so churn
        // interrupted by real work must not accumulate across the reset.
        let mut backing = TestTurnProcessingBacking::new(40).await;
        let mut ctx = backing.turn_processing_context();
        let mut tracker = LoopTracker::new();
        let miss = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({"results":[]}),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });

        for step in 0..(EXECUTION_TOTAL_LOW_SIGNAL_THRESHOLD - 1) {
            update_repetition_tracker(
                &mut tracker,
                &miss,
                tool_names::CODE_SEARCH,
                &json!({"query": format!("a{step}"), "path": "src"}),
            );
        }
        update_repetition_tracker(
            &mut tracker,
            &ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
                output: json!({}),
                stdout: None,
                modified_files: vec!["src/main.rs".to_string()],
                command_success: true,
            }),
            tool_names::EDIT_FILE,
            &json!({"path":"src/main.rs","old_string":"a","new_string":"b"}),
        );
        assert_eq!(tracker.total_low_signal_navigations, 0);

        for step in 0..(EXECUTION_TOTAL_LOW_SIGNAL_THRESHOLD - 1) {
            update_repetition_tracker(
                &mut tracker,
                &miss,
                tool_names::CODE_SEARCH,
                &json!({"query": format!("b{step}"), "path": "src"}),
            );
        }
        let outcome = super::handle_turn_balancer(&mut ctx, 24, &mut tracker, 40, 3).await;
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(!ctx.is_recovery_active());
    }
}
