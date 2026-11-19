//! Model-initiated computer turns through the `computer_use` tool.
//!
//! The turn loop intercepts validated `computer_use` calls here instead of
//! registry execution: the dispatcher, the permission-flow approval, and the
//! isolated target all live on the binary side of the core/binary boundary,
//! where registry executors cannot reach. The registered executor stays a
//! fail-closed stub for defense in depth.
//!
//! Every outcome maps onto the generic [`ToolPipelineOutcome`] contract so
//! batch tracking, circuit diagnostics, harness events, and transcript
//! rendering behave exactly like any other tool: completed turns report
//! redacted lines, denials and cancellations are `Cancelled`, and every
//! fail-closed refusal is a `Failure` carrying its guidance text.

use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use vtcode_core::llm::provider::{LLMRequest, Message, MessageContent, MessageRole};

use super::execution::{
    ComputerTurnOutcome, ComputerTurnOutcomeKind, ComputerTurnProposal, ComputerTurnRuntime, execute_proposed_turn,
    propose_computer_turn,
};
use super::report::turn_report_lines;
use super::{TOOL_NAME, request_target_approval};
use crate::agent::runloop::unified::tool_pipeline::status::{ToolExecutionStatus, ToolPipelineOutcome};
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

/// Run one model-requested computer turn for `task`, approving through the
/// permission flow. Borrows never overlap: propose, approve, and execute
/// each take the turn context in turn.
pub(crate) async fn handle_computer_use_tool_call(
    ctx: &mut TurnProcessingContext<'_>,
    args_val: &Value,
) -> Result<ToolPipelineOutcome> {
    let Some(task) = args_val
        .get("task")
        .and_then(Value::as_str)
        .filter(|task| !task.trim().is_empty())
    else {
        return Ok(failure_outcome("computer_use needs a non-empty task describing one screen-sized goal."));
    };
    let computer_use = match ctx.vt_cfg.as_ref() {
        Some(config) => config.computer_use.clone(),
        None => crate::main_helpers::load_workspace_config(&ctx.config.workspace)
            .context("Failed to load VT Code configuration")?
            .config()
            .computer_use
            .clone(),
    };
    let model = ctx.config.model.clone();
    let provider_name = ctx.provider_client.name().to_string();
    let request = LLMRequest {
        model,
        messages: Arc::new(vec![Message {
            role: MessageRole::User,
            content: MessageContent::Text(task.to_string()),
            ..Default::default()
        }]),
        ..Default::default()
    };
    let proposal = propose_computer_turn(
        &**ctx.provider_client,
        request,
        &computer_use,
        &mut ComputerTurnRuntime {
            stop: ctx.ctrl_c_state,
            notify: ctx.ctrl_c_notify,
            stats: ctx.session_stats,
            emitter: ctx.harness_emitter,
        },
    )
    .await?;
    let outcome = match proposal {
        ComputerTurnProposal::Done(outcome) => outcome,
        ComputerTurnProposal::Proceed(proposed) => {
            // Approval infrastructure failures are tool data, not turn
            // aborts (matching sibling tools); usage still records.
            let granted = match request_target_approval(ctx, &proposed.approval).await {
                Ok(granted) => granted,
                Err(err) => {
                    ctx.session_stats.record_usage(&provider_name, proposed.usage.as_ref());
                    return Ok(failure_outcome(&format!("Computer turn approval failed: {err:#}")));
                }
            };
            execute_proposed_turn(
                proposed,
                &computer_use,
                &mut ComputerTurnRuntime {
                    stop: ctx.ctrl_c_state,
                    notify: ctx.ctrl_c_notify,
                    stats: ctx.session_stats,
                    emitter: ctx.harness_emitter,
                },
                granted,
            )
            .await?
        }
    };
    ctx.session_stats.record_usage(&provider_name, outcome.usage.as_ref());
    Ok(map_computer_outcome(&outcome))
}

fn failure_outcome(message: &str) -> ToolPipelineOutcome {
    ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: vtcode_core::tools::registry::ToolExecutionError::from_anyhow(
            TOOL_NAME,
            &anyhow!(message.to_string()),
            0,
            false,
            false,
            Some("unified_runloop"),
        ),
    })
}

/// Map a dispatcher outcome onto the pipeline contract. Denials and user
/// cancellations are `Cancelled` (not failures); refusals carry their
/// guidance as the failure message so the model can relay it.
fn map_computer_outcome(outcome: &ComputerTurnOutcome) -> ToolPipelineOutcome {
    match &outcome.kind {
        ComputerTurnOutcomeKind::ApprovalDenied | ComputerTurnOutcomeKind::Cancelled => {
            ToolPipelineOutcome::from_status(ToolExecutionStatus::Cancelled)
        }
        ComputerTurnOutcomeKind::Completed { .. } => ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: Value::String(turn_report_lines(outcome).join("\n")),
            stdout: None,
            modified_files: Vec::new(),
            command_success: true,
        }),
        _ => failure_outcome(&turn_report_lines(outcome).join("\n")),
    }
}

#[cfg(test)]
mod tests {
    use super::super::execution::{NativeTurnResults, RedactedTurnSummary, TeardownOutcome};
    use super::*;
    use vtcode_core::llm::provider::{
        FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, NativeCall, NativeCallBody, NativeProvider,
        NativeResponse,
    };

    struct ScriptedNativeProvider;

    #[async_trait::async_trait]
    impl LLMProvider for ScriptedNativeProvider {
        fn name(&self) -> &str {
            "scripted-native"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            Err(LLMError::Provider {
                message: "computer turns generate natives, never chat completions".to_string(),
                metadata: None,
            })
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["scripted-native-model".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }

        fn supports_computer_use(&self) -> bool {
            true
        }

        async fn generate_native(&self, _request: LLMRequest) -> Result<NativeResponse, LLMError> {
            Ok(NativeResponse {
                content: Some("acting now".to_string()),
                reasoning: None,
                model: "scripted-native-model".to_string(),
                usage: None,
                finish_reason: FinishReason::ToolCalls,
                tool_calls: Vec::new(),
                native_calls: vec![NativeCall {
                    id: "shot".to_string(),
                    provider: NativeProvider::Anthropic,
                    index: 1,
                    body: NativeCallBody::Anthropic {
                        name: "screenshot".to_string(),
                        action: Ok(vtcode_computer_use::Action::Screenshot),
                    },
                }],
                assistant_blocks: Vec::new(),
                request_id: None,
                organization_id: None,
            })
        }
    }

    fn outcome(kind: ComputerTurnOutcomeKind) -> ComputerTurnOutcome {
        ComputerTurnOutcome { kind, model: "test-model".to_string(), usage: None }
    }

    #[test]
    fn computer_outcome_mapping_keeps_denials_out_of_failures() {
        for kind in [
            ComputerTurnOutcomeKind::ApprovalDenied,
            ComputerTurnOutcomeKind::Cancelled,
        ] {
            let mapped = map_computer_outcome(&outcome(kind));
            assert!(matches!(mapped.status, ToolExecutionStatus::Cancelled), "denials must not trip failure tracking");
            assert!(!mapped.status.is_failure_like());
        }
    }

    #[test]
    fn computer_outcome_mapping_reports_completion_and_refusals() {
        let summary = RedactedTurnSummary {
            target_kind: "browser",
            target: "fixture-app".to_string(),
            recipient: "test-model (test)".to_string(),
            action_kinds: vec!["screenshot".to_string()],
            action_count: 1,
            executed_units: 1,
            review: None,
            teardown_trigger: "finished",
            teardown: TeardownOutcome::Removed,
        };
        let completed = map_computer_outcome(&outcome(ComputerTurnOutcomeKind::Completed {
            results: NativeTurnResults::OpenAi(vec![serde_json::json!({"call_id": "call_1"})]),
            summary,
        }));
        match &completed.status {
            ToolExecutionStatus::Success { output, command_success, .. } => {
                assert!(command_success);
                assert!(
                    output
                        .as_str()
                        .is_some_and(|text| text.contains("completed (test-model): 1 units"))
                );
                assert!(output.as_str().is_some_and(|text| text.contains("call call_1: ok")));
            }
            status => panic!("completed turn must succeed, got {status:?}"),
        }
        assert!(!completed.status.is_failure_like());

        for (kind, needle) in [
            (ComputerTurnOutcomeKind::Disabled { reason: "disabled" }, "did not run: disabled."),
            (ComputerTurnOutcomeKind::BudgetExceeded { proposed: 60, max: 50 }, "exceed the budget of 50"),
            (ComputerTurnOutcomeKind::ReviewHalted, "SUSPECT"),
        ] {
            let mapped = map_computer_outcome(&outcome(kind));
            match &mapped.status {
                ToolExecutionStatus::Failure { error } => assert!(error.message.contains(needle), "{needle}"),
                status => panic!("refusals must fail loudly, got {status:?}"),
            }
            assert!(mapped.status.is_failure_like());
        }
    }

    #[tokio::test]
    async fn tool_call_without_a_task_fails_before_any_model_call() {
        let mut backing = super::super::super::tests::TestContextBacking::new().await;
        let mut ctx = backing.turn_processing_context();
        for args in [serde_json::json!({}), serde_json::json!({"task": "  "})] {
            let outcome = handle_computer_use_tool_call(&mut ctx, &args).await.expect("tool result");
            match &outcome.status {
                ToolExecutionStatus::Failure { error } => {
                    assert!(error.message.contains("non-empty task"), "{}", error.message)
                }
                status => panic!("missing task must fail loudly, got {status:?}"),
            }
        }
    }

    // The headless harness auto-allows before reaching the interactive
    // prompt, so the scope-prompt skip itself is not headless-observable
    // (verified by mutation: proceed-without-session-skip passes with and
    // without the skip). The headless-pinned properties are that validation
    // proceeds by default (usability pin, not a skip proof) and that denies
    // still apply, since the skip sits after every deny check.
    #[tokio::test]
    async fn computer_use_validation_proceeds_by_default() {
        let mut backing = super::super::super::tests::TestContextBacking::new().await;
        let mut ctx = backing.turn_processing_context();
        ctx.session.set_skip_confirmations(false);
        let result = super::super::super::validation::validate_tool_call(
            &mut ctx,
            "proceed-1",
            "computer_use",
            &serde_json::json!({"task": "take a screenshot"}),
        )
        .await
        .expect("validate");
        assert!(
            !matches!(result, super::super::super::types::ValidationResult::Blocked),
            "default validation must proceed for computer_use"
        );
    }

    #[tokio::test]
    async fn computer_use_cached_denial_still_blocks() {
        let mut backing = super::super::super::tests::TestContextBacking::new().await;
        let mut ctx = backing.turn_processing_context();
        ctx.tool_permission_cache
            .write()
            .await
            .cache_grant_tool("computer_use", vtcode_core::acp::PermissionGrant::Denied);
        let result = super::super::super::validation::validate_tool_call(
            &mut ctx,
            "deny-1",
            "computer_use",
            &serde_json::json!({"task": "take a screenshot"}),
        )
        .await
        .expect("validate");
        assert!(
            matches!(result, super::super::super::types::ValidationResult::Blocked),
            "cached denial must still block computer_use"
        );
    }

    #[tokio::test]
    async fn tool_call_headless_denial_cancels_before_launch() {
        let mut backing = super::super::super::tests::TestContextBacking::new().await;
        let mut ctx = backing.turn_processing_context();
        let mut vt_cfg = vtcode_core::config::loader::VTCodeConfig::default();
        vt_cfg.computer_use.enabled = true;
        vt_cfg.computer_use.browser_build = Some(std::path::PathBuf::from("/tmp/fixture-build"));
        ctx.vt_cfg = Some(&vt_cfg);
        *ctx.provider_client = Box::new(ScriptedNativeProvider);
        let outcome = handle_computer_use_tool_call(&mut ctx, &serde_json::json!({"task": "take a screenshot"}))
            .await
            .expect("tool result");
        // Headless permission flow denies: the turn must end cancelled with
        // nothing launched (no docker needed to prove it).
        assert!(
            matches!(outcome.status, ToolExecutionStatus::Cancelled),
            "headless denial must cancel, got {:?}",
            outcome.status
        );
        assert!(!outcome.status.is_failure_like());
    }
}
