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
        EXECUTION_TOTAL_LOW_SIGNAL_THRESHOLD, LISTING_LOOP_TRIP_COUNT, NAVIGATION_LOOP_THRESHOLD,
        PLANNING_CONSECUTIVE_LOW_SIGNAL_THRESHOLD, PLANNING_LISTING_LOOP_TRIP_COUNT,
        PLANNING_TOTAL_LOW_SIGNAL_THRESHOLD,
    };

    // NL2Repo-Bench checks run on every step (no backoff) since they
    // are safety guardrails, not performance optimizations.

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
mod tests;
