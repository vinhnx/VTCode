use anyhow::Result;
use serde_json::Value;

use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext,
};

use super::super::helpers::push_tool_response;
use super::ValidationResult;
use super::fallbacks::build_validation_error_content_with_fallback;

fn circuit_breaker_default_blocked(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    canonical_tool_name: &str,
    fallback_tool: Option<String>,
    fallback_tool_args: Option<Value>,
) -> ValidationResult {
    let parts = ctx.parts_mut();
    push_tool_response(
        parts.state.working_history,
        tool_call_id.to_string(),
        build_validation_error_content_with_fallback(
            format!(
                "Tool '{}' is temporarily disabled due to high failure rate (Circuit Breaker OPEN).",
                canonical_tool_name
            ),
            "circuit_breaker",
            fallback_tool,
            fallback_tool_args,
        ),
    );
    ValidationResult::Blocked
}

/// Attempt interactive recovery when a circuit breaker blocks a tool.
pub(crate) async fn try_interactive_circuit_recovery(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    canonical_tool_name: &str,
    fallback_tool: Option<String>,
    fallback_tool_args: Option<Value>,
) -> Result<Option<ValidationResult>> {
    use crate::agent::runloop::unified::turn::recovery_flow::{self, RecoveryAction};

    if ctx.full_auto || !ctx.error_recovery.read().await.can_prompt_user() {
        return Ok(Some(circuit_breaker_default_blocked(
            ctx,
            tool_call_id,
            canonical_tool_name,
            fallback_tool,
            fallback_tool_args,
        )));
    }

    let prompt_args = {
        let open_circuits = ctx.circuit_breaker.get_open_circuits();
        let diagnostics = ctx
            .error_recovery
            .read()
            .await
            .get_diagnostics(&open_circuits, 10);
        recovery_flow::build_recovery_prompt_from_diagnostics(&diagnostics).build()
    };
    ctx.error_recovery.write().await.mark_prompt_shown();

    let action = recovery_flow::execute_recovery_prompt(
        ctx.handle,
        ctx.session,
        &prompt_args,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
    )
    .await
    .ok()
    .and_then(|r| recovery_flow::parse_recovery_response(&r));

    let Some(action) = action else {
        return Ok(Some(circuit_breaker_default_blocked(
            ctx,
            tool_call_id,
            canonical_tool_name,
            fallback_tool,
            fallback_tool_args,
        )));
    };

    match action {
        RecoveryAction::ResetAllCircuits => {
            ctx.circuit_breaker.reset_all();
            ctx.error_recovery.write().await.clear_circuit_events();
            tracing::info!("Recovery: user chose Reset All & Retry");
            Ok(None)
        }
        RecoveryAction::Continue => {
            tracing::info!(action = ?action, "Recovery: user chose to continue");
            Ok(None)
        }
        RecoveryAction::SkipStep => {
            tracing::info!("Recovery: user chose Skip This Step");
            let parts = ctx.parts_mut();
            push_tool_response(
                parts.state.working_history,
                tool_call_id.to_string(),
                serde_json::json!({
                    "skipped": true,
                    "reason": "Skipped by user via recovery wizard. Try a different approach."
                })
                .to_string(),
            );
            Ok(Some(ValidationResult::Handled))
        }
        RecoveryAction::SaveAndExit => {
            tracing::info!("Recovery: user chose Save & Exit");
            Ok(Some(ValidationResult::Outcome(TurnHandlerOutcome::Break(
                TurnLoopResult::Exit,
            ))))
        }
    }
}
