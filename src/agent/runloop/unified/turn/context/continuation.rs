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

pub(super) fn evaluate_interim_text_continuation(
    full_auto: bool,
    plan_mode: bool,
    history: &[uni::Message],
    text: &str,
) -> InterimTextContinuationDecision {
    let is_interim_progress = is_interim_progress_update(text);
    let last_user_follow_up = last_user_message_is_follow_up(history);
    let recent_tool_activity = has_recent_tool_activity(history);
    let last_user_requested_progressive_work = last_user_requested_progressive_work(history);

    if plan_mode {
        return InterimTextContinuationDecision {
            should_continue: false,
            reason: "plan_mode",
            is_interim_progress,
            last_user_follow_up,
            recent_tool_activity,
            last_user_requested_progressive_work,
        };
    }

    if !is_interim_progress {
        // Relaxed: if the model just ran tools and the final clause isn't conclusive,
        // treat a long analysis text as continuation-worthy even if it doesn't match
        // the strict interim-intent prefixes.
        if recent_tool_activity
            && !last_clause_contains_conclusive_marker(&text.to_ascii_lowercase())
        {
            return InterimTextContinuationDecision {
                should_continue: true,
                reason: "recent_tool_activity_relaxed",
                is_interim_progress,
                last_user_follow_up,
                recent_tool_activity,
                last_user_requested_progressive_work,
            };
        }

        return InterimTextContinuationDecision {
            should_continue: false,
            reason: "non_interim_text",
            is_interim_progress,
            last_user_follow_up,
            recent_tool_activity,
            last_user_requested_progressive_work,
        };
    }

    if last_user_follow_up {
        return InterimTextContinuationDecision {
            should_continue: true,
            reason: "follow_up_prompt",
            is_interim_progress,
            last_user_follow_up,
            recent_tool_activity,
            last_user_requested_progressive_work,
        };
    }

    if recent_tool_activity {
        return InterimTextContinuationDecision {
            should_continue: true,
            reason: "recent_tool_activity",
            is_interim_progress,
            last_user_follow_up,
            recent_tool_activity,
            last_user_requested_progressive_work,
        };
    }

    if last_user_requested_progressive_work {
        return InterimTextContinuationDecision {
            should_continue: true,
            reason: "progressive_request",
            is_interim_progress,
            last_user_follow_up,
            recent_tool_activity,
            last_user_requested_progressive_work,
        };
    }

    InterimTextContinuationDecision {
        should_continue: false,
        reason: if full_auto {
            "awaiting_model_action"
        } else {
            "interactive_mode"
        },
        is_interim_progress,
        last_user_follow_up,
        recent_tool_activity,
        last_user_requested_progressive_work,
    }
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
        .rfind(|ch| matches!(ch, '.' | '!' | '\n'))
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
    if starts_with_interim_intent(lower) {
        return true;
    }

    for (idx, ch) in lower.char_indices() {
        if matches!(ch, '.' | '!' | '?' | ':' | ';' | '\n') {
            let remainder = lower[idx + ch.len_utf8()..].trim_start();
            if !remainder.is_empty() && starts_with_interim_intent(remainder) {
                return true;
            }
        }
    }

    false
}

fn starts_with_interim_intent(lower: &str) -> bool {
    let intent_prefixes = [
        "let me ",
        "i'll ",
        "i will ",
        "i need to ",
        "i am going to ",
        "i'm going to ",
        "now i need to ",
        "now let me ",
        "continuing ",
        "next i need to ",
        "next, i'll ",
        "now i'll ",
        "let us ",
    ];

    intent_prefixes
        .iter()
        .any(|prefix| lower.starts_with(prefix))
        || starts_with_present_progress_update(lower)
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
}
