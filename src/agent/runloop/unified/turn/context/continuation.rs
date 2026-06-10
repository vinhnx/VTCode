use vtcode_core::llm::provider as uni;

pub(super) const AUTONOMOUS_CONTINUE_DIRECTIVE: &str = "Do not stop with intent-only updates. Execute the next concrete action now, then report completion or blocker.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct InterimTextContinuationDecision {
    pub(super) should_continue: bool,
    pub(super) reason: &'static str,
    pub(super) is_interim_progress: bool,
    pub(super) last_user_follow_up: bool,
    pub(super) recent_tool_activity: bool,
    pub(super) last_user_requested_progressive_work: bool,
}

impl InterimTextContinuationDecision {
    fn with(
        should_continue: bool,
        reason: &'static str,
        is_interim_progress: bool,
        last_user_follow_up: bool,
        recent_tool_activity: bool,
        last_user_requested_progressive_work: bool,
    ) -> Self {
        Self {
            should_continue,
            reason,
            is_interim_progress,
            last_user_follow_up,
            recent_tool_activity,
            last_user_requested_progressive_work,
        }
    }
}

pub(super) fn evaluate_interim_text_continuation(
    full_auto: bool,
    planning_active: bool,
    history: &[uni::Message],
    text: &str,
) -> InterimTextContinuationDecision {
    let is_interim_progress = is_interim_progress_update(text);
    let lower = text.to_ascii_lowercase();
    let last_user_follow_up = last_user_message_is_follow_up(history);
    let recent_tool_activity = has_recent_tool_activity(history);
    let last_user_requested_progressive_work = last_user_requested_progressive_work(history);

    let d = |should_continue: bool, reason: &'static str| {
        InterimTextContinuationDecision::with(
            should_continue,
            reason,
            is_interim_progress,
            last_user_follow_up,
            recent_tool_activity,
            last_user_requested_progressive_work,
        )
    };

    if planning_active {
        return d(false, "planning_active");
    }

    if !is_interim_progress {
        let not_conclusive = !last_clause_contains_conclusive_marker(&lower);
        let has_relaxed_continuation_intent = has_relaxed_continuation_intent(&lower);
        // Relaxed: if the model just ran tools, or the user asked for progressive work,
        // and the text still contains a continuation-intent clause, treat long
        // analysis text as continuation-worthy even if it exceeded the strict
        // interim-progress shape.
        if not_conclusive && has_relaxed_continuation_intent && recent_tool_activity {
            return d(true, "recent_tool_activity_relaxed");
        }
        if not_conclusive && has_relaxed_continuation_intent && last_user_requested_progressive_work
        {
            return d(true, "progressive_relaxed");
        }
        return d(false, "non_interim_text");
    }

    if last_user_follow_up {
        return d(true, "follow_up_prompt");
    }

    if recent_tool_activity {
        return d(true, "recent_tool_activity");
    }

    if last_user_requested_progressive_work {
        return d(true, "progressive_request");
    }

    d(
        false,
        if full_auto {
            "awaiting_model_action"
        } else {
            "interactive_mode"
        },
    )
}

pub(super) fn push_system_directive_once(history: &mut Vec<uni::Message>, directive: &str) {
    let already_present = history.iter().rev().take(3).any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text().trim() == directive
    });
    if !already_present {
        history.push(uni::Message::system(directive.to_string()));
    }
}

/// Returns true when the **last** clause (after the final sentence boundary) contains
/// a conclusive marker like "completed", "done", "fixed", "summary", etc.
/// A middle clause with "completed" followed by "Now let me ..." does NOT match —
/// only the final clause determines conclusiveness.
fn last_clause_contains_conclusive_marker(lower: &str) -> bool {
    let conclusive_markers = [
        "completed",
        "done",
        "fixed",
        "resolved",
        "summary",
        "final review",
        "final blocker",
        "next action",
        "what changed",
        "validation",
        "passed",
        "passes",
        "cannot proceed",
        "can't proceed",
        "blocked by",
        "all set",
    ];
    let last_clause = lower
        .rfind(|ch| ['.', '!', '\n', '—', '…'].contains(&ch))
        .map(|idx| lower[idx + 1..].trim_start())
        .unwrap_or(lower);
    conclusive_markers
        .iter()
        .any(|marker| last_clause.contains(marker))
}

pub(super) fn is_interim_progress_update(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.len() > 800 {
        return false;
    }

    let lower = trimmed.to_ascii_lowercase();
    if !has_interim_intent_clause(&lower) {
        return false;
    }

    let user_input_markers = [
        "could you",
        "can you",
        "please provide",
        "need your",
        "need you to",
        "which option",
    ];
    if trimmed.contains('?')
        || user_input_markers
            .iter()
            .any(|marker| lower.contains(marker))
    {
        return false;
    }

    !last_clause_contains_conclusive_marker(&lower)
}

fn last_user_message_is_follow_up(history: &[uni::Message]) -> bool {
    history
        .iter()
        .rev()
        .find(|message| message.role == uni::MessageRole::User)
        .is_some_and(|message| {
            crate::agent::runloop::unified::state::is_follow_up_prompt_like(
                message.content.as_text().as_ref(),
            )
        })
}

fn has_recent_tool_activity(history: &[uni::Message]) -> bool {
    history.iter().rev().take(16).any(|message| {
        message.role == uni::MessageRole::Tool
            || message.tool_call_id.is_some()
            || message.tool_calls.is_some()
    })
}

fn last_user_requested_progressive_work(history: &[uni::Message]) -> bool {
    let Some(text) = last_user_message_text(history) else {
        return false;
    };
    [
        "explore",
        "inspect",
        "look into",
        "investigate",
        "debug",
        "trace",
        "check",
        "review",
        "analy",
        "walk through",
        "run ",
        "execute",
        "format",
        "cargo fmt",
        "cargo check",
        "cargo test",
        "fix",
        "edit",
        "update",
        "change",
        "modify",
        "scan",
        "search",
        "grep",
        "ast-grep",
        "find ",
        "use vt code",
        "semantic code understanding",
        "show me how",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn has_interim_intent_clause(lower: &str) -> bool {
    if clause_has_continuation_intent(lower) {
        return true;
    }

    for (idx, ch) in lower.char_indices() {
        if matches!(ch, '.' | '!' | '?' | ':' | ';' | '\n' | '—' | '…') {
            let remainder = lower[idx + ch.len_utf8()..].trim_start();
            if !remainder.is_empty() && clause_has_continuation_intent(remainder) {
                return true;
            }
        }
    }

    false
}

fn has_relaxed_continuation_intent(lower: &str) -> bool {
    if has_interim_intent_clause(lower) {
        return true;
    }

    [
        " let me ",
        " i'll ",
        " i will ",
        " i need to ",
        " i want to ",
        " i'd like to ",
        " next step ",
        " next up:",
        " continuing ",
        " time to ",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/// Returns true when a single clause expresses an intent to continue working.
///
/// Instead of matching an ever-growing list of specific prefixes, this
/// normalizes away clause-initial transition words ("now", "next", "then",
/// "first") and subjects ("i", "we"), then checks a compact set of
/// grammatical patterns that capture the underlying linguistic structure
/// of continuation intent.  This handles many more phrasings automatically
/// than explicitly listing every variant.
fn clause_has_continuation_intent(clause: &str) -> bool {
    let clause = clause.trim_start();
    if clause.is_empty() {
        return false;
    }

    let normalized = normalize_clause(clause);
    if normalized.is_empty() {
        return false;
    }

    core_intent_matches(normalized) || starts_with_present_progress_update(normalized)
}

/// Strip leading transition words and subjects to reach the core intent
/// expression.  Reduces hundreds of possible phrasings down to a handful
/// of grammatical patterns checked by [`core_intent_matches`].
///
/// Transitions: "now", "next" (only before i/we), "then", "first"
/// Possessives: "my" (for "my next step is ...")
/// Subjects: "i" (and optionally "am"), "we"
fn normalize_clause(s: &str) -> &str {
    let s = s.trim_start();
    let s = s.strip_prefix("now ").unwrap_or(s);
    let s = s.strip_prefix("then ").unwrap_or(s);
    let s = s.strip_prefix("first, ").unwrap_or(s);
    let s = s.strip_prefix("first ").unwrap_or(s);
    let s = s.strip_prefix("next, ").unwrap_or(s);
    // "next" before a subject is a transition; otherwise it may be
    // part of an intent expression ("next step", "next up").
    let s = match s.strip_prefix("next ") {
        Some(after) if after.starts_with("i ") || after.starts_with("we ") => after,
        _ => s,
    };
    let s = s.strip_prefix("my ").unwrap_or(s);
    let s = s.strip_prefix("i am ").unwrap_or(s);
    // Handle both "i " (uncontracted) and contractions (i'll, i'd, i'm, i've).
    // For contractions we strip only the "i", keeping the apostrophe so that
    // patterns like "'ll " and "'d like to " still match.
    let s = if let Some(after) = s.strip_prefix("i ") {
        after
    } else if let Some(after) = s.strip_prefix("i").filter(|after| after.starts_with('\'')) {
        after
    } else {
        s
    };
    let s = s.strip_prefix("we ").unwrap_or(s);
    s.trim_start()
}

/// Check the core grammatical patterns of continuation intent.
fn core_intent_matches(text: &str) -> bool {
    // "let {me|us|'s}" + action verb
    if text.starts_with("let ") {
        return true;
    }

    // <intent-verb> "to" <action>
    // Covers: need to, want to, going to, plan to, intend to,
    //         have to, 'm going to, 'd like to, hope to, etc.
    const TO_INTENTS: &[&str] = &[
        "need to ",
        "want to ",
        "going to ",
        "plan to ",
        "intend to ",
        "have to ",
        "'m going to ",
        "'d like to ",
        "hope to ",
    ];
    if TO_INTENTS.iter().any(|v| text.starts_with(v)) {
        return true;
    }

    // <modal> <action> — exclude conclusive follow-ups
    if let Some(rest) = text
        .strip_prefix("will ")
        .or_else(|| text.strip_prefix("'ll "))
    {
        return !rest.starts_with("be ") && !rest.starts_with("now be ");
    }

    // Standalone expressions that don't fit the verb patterns above
    if text.starts_with("time to ")
        || text.starts_with("next up:")
        || text.starts_with("next step ")
        || text.starts_with("continuing")
    {
        return true;
    }

    false
}

fn starts_with_present_progress_update(lower: &str) -> bool {
    let present_progress_prefixes = [
        "running ",
        "checking ",
        "formatting ",
        "scanning ",
        "inspecting ",
        "searching ",
        "reading ",
        "reviewing ",
        "tracing ",
        "debugging ",
    ];
    let forward_markers = [
        " now",
        " then ",
        " next ",
        " follow-up",
        " to confirm",
        " to check",
        " to verify",
        " to inspect",
    ];

    present_progress_prefixes
        .iter()
        .any(|prefix| lower.starts_with(prefix))
        && forward_markers.iter().any(|marker| lower.contains(marker))
}

fn last_user_message_text(history: &[uni::Message]) -> Option<String> {
    history
        .iter()
        .rev()
        .find(|message| message.role == uni::MessageRole::User)
        .map(|message| message.content.as_text().to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runloop::unified::run_loop_context::RecoveryMode;
    use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnLoopResult};
    use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;

    #[test]
    fn follow_up_prompt_detection_accepts_continue_variants() {
        assert!(crate::agent::runloop::unified::state::is_follow_up_prompt_like("continue"));
        assert!(crate::agent::runloop::unified::state::is_follow_up_prompt_like("continue."));
        assert!(crate::agent::runloop::unified::state::is_follow_up_prompt_like("go on"));
        assert!(crate::agent::runloop::unified::state::is_follow_up_prompt_like("please continue"));
        assert!(
            crate::agent::runloop::unified::state::is_follow_up_prompt_like(
                "Continue autonomously from the last stalled turn. Stall reason: x."
            )
        );
        assert!(
            !crate::agent::runloop::unified::state::is_follow_up_prompt_like(
                "run cargo clippy and fix"
            )
        );
    }

    #[test]
    fn interim_progress_detection_requires_non_conclusive_intent_text() {
        assert!(is_interim_progress_update(
            "Let me fix the second collapsible if statement:"
        ));
        assert!(is_interim_progress_update(
            "Let me fix the second collapsible if statement in the Anthropic provider:"
        ));
        assert!(is_interim_progress_update(
            "Now I need to update the function body to use settings.reasoning_effort and settings.verbosity:"
        ));
        assert!(is_interim_progress_update(
            "I'll continue with the next fix."
        ));
        assert!(is_interim_progress_update(
            "Running formatter now, then I'll do a quick follow-up check (`cargo check`) to confirm nothing regressed."
        ));
        assert!(is_interim_progress_update(
            "The structural search keeps returning empty results. Let me verify the indexer is working and try with a simpler known pattern:"
        ));
        assert!(!is_interim_progress_update(
            "I need you to choose which option to apply."
        ));
        assert!(!is_interim_progress_update(
            "Running cargo fmt uses rustfmt to rewrite the source files."
        ));
        assert!(!is_interim_progress_update(
            "Completed. All requested fixes are done."
        ));
        assert!(!is_interim_progress_update(
            "Final review: two blockers remain with next action."
        ));
    }

    #[test]
    fn autonomous_continue_triggers_for_follow_up_and_interim_text() {
        let history = vec![uni::Message::user("continue".to_string())];
        assert!(
            evaluate_interim_text_continuation(true, false, &history, "Let me fix the next issue.")
                .should_continue
        );
        assert!(
            !evaluate_interim_text_continuation(true, true, &history, "Let me fix the next issue.")
                .should_continue
        );
        assert!(
            evaluate_interim_text_continuation(
                false,
                false,
                &history,
                "Let me fix the next issue."
            )
            .should_continue
        );
    }

    #[test]
    fn autonomous_continue_triggers_for_interim_text_after_tool_activity() {
        let history = vec![
            uni::Message::user("run cargo clippy and fix".to_string()),
            uni::Message::assistant("I will run cargo clippy now.".to_string()).with_tool_calls(
                vec![uni::ToolCall::function(
                    "call_1".to_string(),
                    "unified_exec".to_string(),
                    "{}".to_string(),
                )],
            ),
            uni::Message::tool_response("call_1".to_string(), "warning: ...".to_string()),
        ];

        assert!(evaluate_interim_text_continuation(
            true,
            false,
            &history,
            "Now I need to update the function body to use settings.reasoning_effort and settings.verbosity:"
        )
        .should_continue);
    }

    #[test]
    fn autonomous_continue_triggers_for_execution_request_without_prior_tool_activity() {
        let history = vec![
            uni::Message::user("run cargo clippy and fix".to_string()),
            uni::Message::assistant("I will start now.".to_string()),
        ];

        assert!(evaluate_interim_text_continuation(
            true,
            false,
            &history,
            "Now I need to update the function body to use settings.reasoning_effort and settings.verbosity:"
        )
        .should_continue);
    }

    #[test]
    fn autonomous_continue_triggers_for_exploration_request_without_full_auto() {
        let history = vec![
            uni::Message::user("explore about vtcode core agent loop".to_string()),
            uni::Message::assistant("I can help.".to_string()),
        ];

        assert!(evaluate_interim_text_continuation(
            false,
            false,
            &history,
            "I'll quickly inspect the actual vtcode-core runloop files and then summarize the core agent loop concretely from code."
        )
        .should_continue);
    }

    #[test]
    fn autonomous_continue_does_not_trigger_for_explanatory_request_without_full_auto() {
        let history = vec![
            uni::Message::user("tell me about core agent loop".to_string()),
            uni::Message::assistant("I can help.".to_string()),
        ];

        assert!(!evaluate_interim_text_continuation(
            false,
            false,
            &history,
            "I'll quickly inspect the actual vtcode-core runloop files and then summarize the core agent loop concretely from code."
        )
        .should_continue);
    }

    #[tokio::test]
    async fn recovery_pass_progress_only_text_completes_turn() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.activate_recovery("loop detector");
        assert!(ctx.consume_recovery_pass());

        let outcome = ctx
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
            outcome,
            TurnHandlerOutcome::Break(TurnLoopResult::Completed)
        ));
        assert!(!ctx.is_recovery_active());
    }

    #[tokio::test]
    async fn recovery_pass_diagnostic_then_next_step_text_completes_turn() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.activate_recovery("turn balancer");
        assert!(ctx.consume_recovery_pass());

        let outcome = ctx
            .handle_text_response(
                "The structural search keeps returning empty results. Let me verify the indexer is working and try with a simpler known pattern:".to_string(),
                Vec::new(),
                None,
                None,
                false,
            )
            .await
            .expect("recovery response should be handled");

        assert!(matches!(
            outcome,
            TurnHandlerOutcome::Break(TurnLoopResult::Completed)
        ));
        assert!(!ctx.is_recovery_active());
    }

    #[test]
    fn detects_new_intent_prefixes_as_interim() {
        assert!(is_interim_progress_update(
            "I'd like to check the next file before proceeding."
        ));
        assert!(is_interim_progress_update(
            "I want to verify the output of the previous step."
        ));
        assert!(is_interim_progress_update(
            "My next step is to run the full test suite."
        ));
        assert!(is_interim_progress_update(
            "Time to fix the remaining lint warnings."
        ));
        assert!(is_interim_progress_update(
            "Let me now inspect the second module for regressions."
        ));
    }

    #[test]
    fn em_dash_boundary_detected_as_interim() {
        assert!(is_interim_progress_update(
            "First check passed—now let me verify the second component."
        ));
        assert!(has_interim_intent_clause(
            "done with the first task—now i'll move to the next"
        ));
    }

    #[test]
    fn ellipsis_boundary_detected_as_interim() {
        assert!(has_interim_intent_clause(
            "checking results…now i need to update the config"
        ));
        assert!(is_interim_progress_update(
            "Scanning the logs…now let me check for error patterns."
        ));
    }

    #[test]
    fn long_text_with_progressive_request_continues_via_relaxed_path() {
        // User asked for progressive work ("fix"), model responds with long analysis
        // (> 800 chars, non-interim pattern), no tool activity.
        // Should continue via "progressive_relaxed" path.
        let long_analysis_base = "The root cause of this issue is a race condition in the connection \
            pool initialization. When multiple threads attempt to acquire connections \
            simultaneously, the pool's internal counter can overflow. This happens because \
            the increment operation is not atomic. The fix should wrap the counter update \
            in a mutex lock. Additionally, we should consider using atomic operations for \
            better performance. ";
        // Pad to exceed 800 chars
        let padding = "x".repeat(820usize.saturating_sub(long_analysis_base.len()));
        let long_text = format!(
            "{}{} Let me now implement the mutex-based fix in the connection pool module.",
            long_analysis_base, padding
        );
        assert!(
            long_text.len() > 800,
            "test text must exceed 800-char limit to trigger relaxed path"
        );

        let history = vec![
            uni::Message::user("fix the race condition in connection pool".to_string()),
            uni::Message::assistant("I'll look into it.".to_string()),
        ];

        // Without tool activity, the text is > 800 chars → not interim → hits relaxed path
        // last_user_requested_progressive_work is true → should continue
        assert!(
            evaluate_interim_text_continuation(false, false, &history, &long_text).should_continue
        );
    }

    #[test]
    fn long_text_with_tool_activity_and_not_conclusive_continues() {
        let long_analysis = "I've reviewed the output from the linter. There are several \
            warnings in the networking module. The main issues are unused imports and \
            a potential memory leak in the connection handler. The fixes are \
            straightforward: remove the unused imports and add proper cleanup in the \
            deinit method. Let me apply these changes to the affected files now. \
            Starting with the networking module, I'll remove the unused imports and \
            then fix the memory leak in the connection handler. After that, I'll \
            run the linter again to verify the warnings are resolved.";
        assert!(
            long_analysis.len() > 280,
            "test text must exceed original 280-char limit"
        );

        let history = vec![
            uni::Message::user("run cargo clippy and fix warnings".to_string()),
            uni::Message::assistant("Running clippy now.".to_string()).with_tool_calls(vec![
                uni::ToolCall::function(
                    "call_1".to_string(),
                    "unified_exec".to_string(),
                    "{}".to_string(),
                ),
            ]),
            uni::Message::tool_response("call_1".to_string(), "warning: ...".to_string()),
        ];

        assert!(
            evaluate_interim_text_continuation(true, false, &history, long_analysis)
                .should_continue
        );
    }

    #[test]
    fn short_completion_after_tool_activity_does_not_continue_via_relaxed_path() {
        let history = vec![
            uni::Message::user("create a simple rust hello world program".to_string()),
            uni::Message::assistant("Let me compile and run it to confirm it works:".to_string())
                .with_tool_calls(vec![uni::ToolCall::function(
                    "call_1".to_string(),
                    "unified_exec".to_string(),
                    "{}".to_string(),
                )]),
            uni::Message::tool_response("call_1".to_string(), "Hello, World!".to_string()),
        ];

        assert!(
            !evaluate_interim_text_continuation(
                true,
                false,
                &history,
                "It works! The program compiled and ran successfully, printing `Hello, World!`."
            )
            .should_continue
        );
    }

    #[test]
    fn short_completion_after_progressive_request_does_not_continue_via_relaxed_path() {
        let history = vec![
            uni::Message::user("fix the parser regression".to_string()),
            uni::Message::assistant("I'll inspect the parser.".to_string()),
        ];

        assert!(
            !evaluate_interim_text_continuation(
                false,
                false,
                &history,
                "I updated the parser logic and the targeted regression test now passes."
            )
            .should_continue
        );
    }

    #[tokio::test]
    async fn tool_enabled_recovery_pass_can_continue_after_interim_progress() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.working_history.push(uni::Message::user(
            "run cargo fmt and follow up".to_string(),
        ));
        ctx.activate_recovery_with_mode("empty response", RecoveryMode::ToolEnabledRetry);
        assert!(ctx.consume_recovery_pass());

        let outcome = ctx
            .handle_text_response(
                "Running formatter now, then I'll do a quick follow-up check (`cargo check`) to confirm nothing regressed."
                    .to_string(),
                Vec::new(),
                None,
                None,
                false,
            )
            .await
            .expect("tool-enabled recovery response should be handled");

        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(!ctx.is_recovery_active());
        assert!(ctx.recovery_pass_used());
    }

    #[tokio::test]
    async fn continuing_text_response_is_recorded_as_commentary_phase() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.working_history
            .push(uni::Message::user("fix the parser regression".to_string()));

        let outcome = ctx
            .handle_text_response(
                "Now I need to update the parser branch and rerun the targeted test.".to_string(),
                Vec::new(),
                None,
                None,
                false,
            )
            .await
            .expect("continuing text response should be handled");

        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        let last_assistant = ctx
            .working_history
            .iter()
            .rev()
            .find(|message| message.role == uni::MessageRole::Assistant)
            .expect("assistant message should be recorded");
        assert_eq!(last_assistant.phase, Some(uni::AssistantPhase::Commentary));
    }

    #[tokio::test]
    async fn completed_text_response_is_recorded_as_final_answer_phase() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.working_history
            .push(uni::Message::user("explain the core loop".to_string()));

        let outcome = ctx
            .handle_text_response(
                "The core loop requests model output, dispatches tool calls, and ends once a final textual answer is produced."
                    .to_string(),
                Vec::new(),
                None,
                None,
                false,
            )
            .await
            .expect("completed text response should be handled");

        assert!(matches!(
            outcome,
            TurnHandlerOutcome::Break(TurnLoopResult::Completed)
        ));
        let last_assistant = ctx
            .working_history
            .iter()
            .rev()
            .find(|message| message.role == uni::MessageRole::Assistant)
            .expect("assistant message should be recorded");
        assert_eq!(last_assistant.phase, Some(uni::AssistantPhase::FinalAnswer));
    }
}
