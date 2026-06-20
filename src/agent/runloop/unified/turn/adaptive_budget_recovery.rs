//! Adaptive recovery decision point when a turn budget is nearly exhausted.
#![allow(dead_code)]

use crate::agent::runloop::unified::run_loop_context::RecoveryMode;
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;
use anyhow::{Context as _, Result};
use serde_json::json;
use std::time::Duration;
use vtcode_core::config::constants::tools::RECOVERY_DECISION;
use vtcode_core::llm::provider as uni;
use vtcode_core::llm::provider::{LLMRequest, ToolChoice, ToolDefinition};

/// Maximum number of times the adaptive recovery path may re-decide after
/// compaction before it forces a final synthesis. This prevents infinite loops.
pub(crate) const MAX_ADAPTIVE_REDECISIONS: u8 = 2;

/// If the remaining turn budget is at or below this floor, skip the recovery
/// decision LLM call entirely and go straight to a tool-free synthesis. This
/// guards against a slow provider burning the reserve on the decision call and
/// re-triggering the policy-violation loop it is meant to prevent. 15s is well
/// below the minimum reserve sized by `llm_attempt_timeout_secs` (>= 30s), so it
/// will not fire prematurely.
pub(crate) const CRITICAL_BUDGET_FLOOR: Duration = Duration::from_secs(15);

/// Wall-clock cap on the recovery decision LLM call. If the provider does not
/// respond within this window, the caller falls back to a tool-free synthesis.
const RECOVERY_DECISION_TIMEOUT: Duration = Duration::from_secs(20);

/// Returns `true` when the remaining turn budget is too low to justify spending
/// it on a recovery decision LLM call, and a direct synthesis should be used.
pub(crate) fn should_short_circuit_recovery(remaining: Duration) -> bool {
    remaining <= CRITICAL_BUDGET_FLOOR
}

/// Actions the agent may choose when the turn wall-clock budget is nearly
/// exhausted. Only a subset is active in the first iteration; the remaining
/// variants are reserved for future work and currently fall back safely to
/// `SummarizeAndConclude`.
#[derive(Debug)]
pub(crate) enum AdaptiveRecoveryAction {
    SummarizeAndConclude,
    CompactContext,
    RequestMoreResources { prompt: Option<String> },
    AdjustPlan { guidance: Option<String> },
}

pub(crate) struct AdaptiveRecoveryDecision {
    pub action: AdaptiveRecoveryAction,
    pub reason: Option<String>,
}

pub(crate) enum AdaptiveRecoveryOutcome {
    Continue,
    /// The agent voluntarily paused to await the user. The turn should end
    /// gracefully (not as a failure) and the session should wait for the next
    /// user message.
    AwaitUser {
        reason: String,
    },
}

/// Definition of the synthetic tool used to make the recovery decision.
pub(crate) fn recovery_decision_tool_definition() -> ToolDefinition {
    ToolDefinition::function(
        RECOVERY_DECISION.to_string(),
        "Choose how to recover when the turn budget is nearly exhausted.".to_string(),
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": [
                        "summarize_and_conclude",
                        "compact_context",
                        "request_more_resources",
                        "adjust_plan"
                    ],
                    "description": "Recovery action to take."
                },
                "reason": {
                    "type": "string",
                    "description": "Brief explanation for the chosen action."
                },
                "user_prompt": {
                    "type": "string",
                    "description": "For request_more_resources: the prompt to show the user."
                },
                "plan_adjustment": {
                    "type": "string",
                    "description": "For adjust_plan: guidance on how to scope down the plan."
                }
            },
            "required": ["action"]
        }),
    )
}

/// Ask the model to choose a recovery action and return the parsed decision
/// together with any token usage from the decision request.
pub(crate) async fn decide_recovery_action(
    ctx: &mut TurnProcessingContext<'_>,
    active_model: &str,
) -> Result<(AdaptiveRecoveryDecision, Option<uni::Usage>)> {
    let tool = recovery_decision_tool_definition();

    let recent_messages: Vec<uni::Message> = ctx
        .working_history
        .iter()
        .rev()
        .take(20)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    let system_prompt = format!(
        "The turn wall-clock budget is nearly exhausted. Choose a recovery action by calling the `{}` tool.\n\n- summarize_and_conclude: produce a final tool-free summary from the evidence already collected.\n- compact_context: compact earlier conversation history to free capacity, then re-evaluate.\n- request_more_resources: stop the turn and ask the user for more resources, with suggested next actions.\n- adjust_plan: produce a reduced-scope final answer that states what was completed, what is deferred, and how to resume; use `plan_adjustment` to describe the scoping. Use this when the original plan cannot be finished in the remaining budget.\n\nIf you are unsure, choose summarize_and_conclude.",
        RECOVERY_DECISION
    );

    let request = LLMRequest {
        messages: recent_messages,
        system_prompt: Some(std::sync::Arc::new(system_prompt)),
        tools: Some(std::sync::Arc::new(vec![tool])),
        model: active_model.to_string(),
        max_tokens: Some(512),
        stream: false,
        tool_choice: Some(ToolChoice::function(RECOVERY_DECISION.to_string())),
        ..Default::default()
    };

    let response = tokio::time::timeout(
        RECOVERY_DECISION_TIMEOUT,
        ctx.provider_client.generate(request),
    )
    .await
    .map_err(|_| {
        anyhow::anyhow!(
            "recovery decision call exceeded {}s timeout",
            RECOVERY_DECISION_TIMEOUT.as_secs()
        )
    })
    .and_then(|inner| inner.map_err(anyhow::Error::from))?;

    let usage = response.usage.clone();
    let decision = parse_recovery_decision(&response);
    Ok((decision, usage))
}

fn parse_recovery_decision(response: &uni::LLMResponse) -> AdaptiveRecoveryDecision {
    let Some(tool_calls) = response.tool_calls.as_ref() else {
        return fallback_summarize("no tool call returned");
    };

    let Some(call) = tool_calls.iter().find(|c| {
        c.tool_name()
            .map_or(false, |name| name == RECOVERY_DECISION)
    }) else {
        return fallback_summarize("unexpected tool call");
    };

    let args = match call.execution_arguments() {
        Ok(args) => args,
        Err(_) => return fallback_summarize("failed to parse tool arguments"),
    };

    let action_str = args.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let reason = args
        .get("reason")
        .and_then(|v| v.as_str())
        .map(String::from);

    match action_str {
        "summarize_and_conclude" => AdaptiveRecoveryDecision {
            action: AdaptiveRecoveryAction::SummarizeAndConclude,
            reason,
        },
        "compact_context" => AdaptiveRecoveryDecision {
            action: AdaptiveRecoveryAction::CompactContext,
            reason,
        },
        "request_more_resources" => AdaptiveRecoveryDecision {
            action: AdaptiveRecoveryAction::RequestMoreResources {
                prompt: args
                    .get("user_prompt")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            },
            reason,
        },
        "adjust_plan" => AdaptiveRecoveryDecision {
            action: AdaptiveRecoveryAction::AdjustPlan {
                guidance: args
                    .get("plan_adjustment")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            },
            reason,
        },
        _ => fallback_summarize("unknown action"),
    }
}

fn fallback_summarize(reason: &str) -> AdaptiveRecoveryDecision {
    AdaptiveRecoveryDecision {
        action: AdaptiveRecoveryAction::SummarizeAndConclude,
        reason: Some(format!("adaptive recovery fallback: {reason}")),
    }
}

/// Apply the recovery decision chosen by the model.
pub(crate) async fn apply_recovery_decision(
    ctx: &mut TurnProcessingContext<'_>,
    decision: AdaptiveRecoveryDecision,
    turn_history_start_len: usize,
) -> Result<AdaptiveRecoveryOutcome> {
    use crate::agent::runloop::unified::turn::compaction::{
        CompactionContext, CompactionState, compact_history_for_recovery_in_place,
    };

    match decision.action {
        AdaptiveRecoveryAction::SummarizeAndConclude => {
            let _ = ctx.renderer.line(
                vtcode_core::utils::ansi::MessageStyle::Info,
                "Budget nearly exhausted; producing a final answer from collected evidence.",
            );
            ctx.push_system_message(
                "Adaptive recovery: synthesize a final answer from the evidence already collected.",
            );
            ctx.harness_state
                .set_recovery_mode(RecoveryMode::ToolFreeSynthesis);
            ctx.harness_state.reset_recovery_phase_to_pending();
            Ok(AdaptiveRecoveryOutcome::Continue)
        }

        AdaptiveRecoveryAction::CompactContext => {
            let _ = ctx.renderer.line(
                vtcode_core::utils::ansi::MessageStyle::Info,
                "Compacting conversation history to free capacity, then re-evaluating.",
            );
            let workspace = ctx.config.workspace.clone();
            let outcome = compact_history_for_recovery_in_place(
                CompactionContext::new(
                    ctx.provider_client.as_ref(),
                    &ctx.config.model,
                    &ctx.harness_state.run_id.0,
                    &ctx.harness_state.turn_id.0,
                    &workspace,
                    ctx.vt_cfg,
                    ctx.lifecycle_hooks,
                    ctx.harness_emitter,
                ),
                CompactionState::new(ctx.working_history, ctx.session_stats, ctx.context_manager),
                turn_history_start_len,
            )
            .await
            .context("adaptive recovery compaction failed")?;

            if outcome.is_some() {
                ctx.push_system_message("Context compacted. Re-evaluating recovery action.");
            } else {
                let _ = ctx.renderer.line(
                    vtcode_core::utils::ansi::MessageStyle::Info,
                    "Compaction had no effect; producing a final answer.",
                );
                ctx.push_system_message("Compaction had no effect; forcing a final synthesis.");
            }

            let attempts = ctx.harness_state.increment_adaptive_recovery_decisions();
            if attempts > MAX_ADAPTIVE_REDECISIONS {
                ctx.push_system_message(
                    "Adaptive re-decision limit reached; forcing final synthesis.",
                );
                ctx.harness_state
                    .set_recovery_mode(RecoveryMode::ToolFreeSynthesis);
            }

            ctx.harness_state.reset_recovery_phase_to_pending();
            Ok(AdaptiveRecoveryOutcome::Continue)
        }

        AdaptiveRecoveryAction::RequestMoreResources { prompt } => {
            let user_prompt = prompt.unwrap_or_else(|| {
                "The turn budget is nearly exhausted and more resources are needed to continue."
                    .to_string()
            });
            let suggested_commands = "Suggested next actions: `/compact` to compact conversation history and continue, type a message to proceed with a fresh turn, or `/stop` to end the current task.";
            let message = format!("{user_prompt} {suggested_commands}");
            ctx.push_system_message(message.clone());
            let _ = ctx
                .renderer
                .line(vtcode_core::utils::ansi::MessageStyle::Info, &message);
            Ok(AdaptiveRecoveryOutcome::AwaitUser { reason: message })
        }

        AdaptiveRecoveryAction::AdjustPlan { guidance } => {
            let _ = ctx.renderer.line(
                vtcode_core::utils::ansi::MessageStyle::Info,
                "Budget nearly exhausted; producing a reduced-scope plan and final answer.",
            );
            let guidance_text = guidance.unwrap_or_default();
            let system_msg = if guidance_text.trim().is_empty() {
                "Adaptive recovery: the budget is nearly exhausted and the original plan cannot \
                 be finished. Produce a reduced-scope final answer that states what was completed, \
                 what is deferred, and how to resume."
                    .to_string()
            } else {
                format!(
                    "Adaptive recovery: the budget is nearly exhausted and the original plan \
                     cannot be finished. Produce a reduced-scope final answer that states what was \
                     completed, what is deferred, and how to resume. Plan adjustment guidance: \
                     {guidance_text}"
                )
            };
            ctx.push_system_message(system_msg);
            ctx.harness_state
                .set_recovery_mode(RecoveryMode::ToolFreeSynthesis);
            ctx.harness_state.reset_recovery_phase_to_pending();
            Ok(AdaptiveRecoveryOutcome::Continue)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_response_with_tool_call(arguments: &str) -> uni::LLMResponse {
        uni::LLMResponse {
            content: None,
            model: "test-model".to_string(),
            tool_calls: Some(vec![uni::ToolCall::function(
                "call_1".to_string(),
                RECOVERY_DECISION.to_string(),
                arguments.to_string(),
            )]),
            usage: None,
            finish_reason: uni::FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            organization_id: None,
            request_id: None,
            tool_references: Vec::new(),
            compaction: None,
        }
    }

    #[test]
    fn recovery_decision_tool_definition_has_expected_name() {
        let def = recovery_decision_tool_definition();
        assert_eq!(def.function_name(), RECOVERY_DECISION);
    }

    #[test]
    fn parse_recovery_decision_parses_summarize_and_conclude() {
        let response = make_response_with_tool_call(
            r#"{"action":"summarize_and_conclude","reason":"low budget"}"#,
        );
        let decision = parse_recovery_decision(&response);
        match decision.action {
            AdaptiveRecoveryAction::SummarizeAndConclude => {}
            other => panic!("expected SummarizeAndConclude, got {other:?}"),
        }
        assert_eq!(decision.reason.as_deref(), Some("low budget"));
    }

    #[test]
    fn parse_recovery_decision_parses_compact_context() {
        let response = make_response_with_tool_call(r#"{"action":"compact_context"}"#);
        let decision = parse_recovery_decision(&response);
        match decision.action {
            AdaptiveRecoveryAction::CompactContext => {}
            other => panic!("expected CompactContext, got {other:?}"),
        }
    }

    #[test]
    fn parse_recovery_decision_falls_back_on_missing_tool_call() {
        let response = uni::LLMResponse {
            content: Some("I will summarize.".to_string()),
            model: "test-model".to_string(),
            tool_calls: None,
            usage: None,
            finish_reason: uni::FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            organization_id: None,
            request_id: None,
            tool_references: Vec::new(),
            compaction: None,
        };
        let decision = parse_recovery_decision(&response);
        match decision.action {
            AdaptiveRecoveryAction::SummarizeAndConclude => {}
            other => panic!("expected fallback SummarizeAndConclude, got {other:?}"),
        }
    }

    #[test]
    fn parse_recovery_decision_parses_adjust_plan() {
        let response = make_response_with_tool_call(
            r#"{"action":"adjust_plan","plan_adjustment":"scope down"}"#,
        );
        let decision = parse_recovery_decision(&response);
        match decision.action {
            AdaptiveRecoveryAction::AdjustPlan { guidance } => {
                assert_eq!(guidance.as_deref(), Some("scope down"));
            }
            other => panic!("expected AdjustPlan, got {other:?}"),
        }
    }

    #[test]
    fn parse_recovery_decision_falls_back_on_unknown_action() {
        let response = make_response_with_tool_call(r#"{"action":"invalid_action"}"#);
        let decision = parse_recovery_decision(&response);
        match decision.action {
            AdaptiveRecoveryAction::SummarizeAndConclude => {}
            other => panic!("expected fallback SummarizeAndConclude, got {other:?}"),
        }
    }

    #[test]
    fn parse_recovery_decision_parses_request_more_resources() {
        let response = make_response_with_tool_call(
            r#"{"action":"request_more_resources","user_prompt":"Need more time to run tests."}"#,
        );
        let decision = parse_recovery_decision(&response);
        match decision.action {
            AdaptiveRecoveryAction::RequestMoreResources { prompt } => {
                assert_eq!(prompt.as_deref(), Some("Need more time to run tests."));
            }
            other => panic!("expected RequestMoreResources, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn apply_recovery_decision_request_more_resources_awaits_user() {
        use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;

        let mut backing = TestTurnProcessingBacking::new(10).await;
        let mut ctx = backing.turn_processing_context();
        let decision = AdaptiveRecoveryDecision {
            action: AdaptiveRecoveryAction::RequestMoreResources {
                prompt: Some("Need more budget.".to_string()),
            },
            reason: Some("low budget".to_string()),
        };
        let outcome = apply_recovery_decision(&mut ctx, decision, 0)
            .await
            .unwrap();
        assert!(matches!(outcome, AdaptiveRecoveryOutcome::AwaitUser { .. }));
        if let AdaptiveRecoveryOutcome::AwaitUser { reason } = outcome {
            assert!(reason.contains("Need more budget."));
            assert!(reason.contains("`/compact`"));
        }
    }

    #[tokio::test]
    async fn apply_recovery_decision_adjust_plan_sets_tool_free_synthesis() {
        use crate::agent::runloop::unified::run_loop_context::RecoveryMode;
        use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;

        let mut backing = TestTurnProcessingBacking::new(10).await;
        let mut ctx = backing.turn_processing_context();
        let decision = AdaptiveRecoveryDecision {
            action: AdaptiveRecoveryAction::AdjustPlan {
                guidance: Some("Finish only the auth module; defer the rest.".to_string()),
            },
            reason: Some("low budget".to_string()),
        };
        let outcome = apply_recovery_decision(&mut ctx, decision, 0)
            .await
            .unwrap();
        assert!(matches!(outcome, AdaptiveRecoveryOutcome::Continue));
        assert_eq!(
            ctx.harness_state.recovery_mode(),
            Some(RecoveryMode::ToolFreeSynthesis)
        );
    }

    #[tokio::test]
    async fn apply_recovery_decision_summarize_sets_tool_free_synthesis() {
        use crate::agent::runloop::unified::run_loop_context::RecoveryMode;
        use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;

        let mut backing = TestTurnProcessingBacking::new(10).await;
        let mut ctx = backing.turn_processing_context();
        let decision = AdaptiveRecoveryDecision {
            action: AdaptiveRecoveryAction::SummarizeAndConclude,
            reason: None,
        };
        let outcome = apply_recovery_decision(&mut ctx, decision, 0)
            .await
            .unwrap();
        assert!(matches!(outcome, AdaptiveRecoveryOutcome::Continue));
        assert_eq!(
            ctx.harness_state.recovery_mode(),
            Some(RecoveryMode::ToolFreeSynthesis)
        );
    }

    #[test]
    fn should_short_circuit_recovery_at_or_below_floor() {
        assert!(should_short_circuit_recovery(Duration::from_secs(0)));
        assert!(should_short_circuit_recovery(Duration::from_secs(15)));
    }

    #[test]
    fn should_not_short_circuit_recovery_above_floor() {
        assert!(!should_short_circuit_recovery(Duration::from_secs(16)));
        assert!(!should_short_circuit_recovery(Duration::from_secs(120)));
    }
}
