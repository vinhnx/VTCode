use anyhow::Result;
use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext, TurnProcessingResult,
};
use crate::agent::runloop::unified::turn::guards::handle_turn_balancer;
use crate::agent::runloop::unified::turn::tool_outcomes::ToolOutcomeContext;

/// Result of processing a single turn.
pub(crate) struct HandleTurnProcessingResultParams<'a> {
    pub ctx: &'a mut TurnProcessingContext<'a>,
    pub processing_result: TurnProcessingResult,
    pub response_streamed: bool,
    pub step_count: usize,
    pub repeated_tool_attempts:
        &'a mut crate::agent::runloop::unified::turn::tool_outcomes::helpers::LoopTracker,
    pub turn_modified_files: &'a mut BTreeSet<PathBuf>,
    pub session_end_reason: &'a mut crate::hooks::lifecycle::SessionEndReason,
    /// Pre-computed max tool loops limit for efficiency.
    pub max_tool_loops: usize,
    /// Pre-computed tool repeat limit for efficiency.
    pub tool_repeat_limit: usize,
}

/// Dispatch the appropriate response handler based on the processing result.
pub(crate) async fn handle_turn_processing_result<'a>(
    params: HandleTurnProcessingResultParams<'a>,
) -> Result<TurnHandlerOutcome> {
    match params.processing_result {
        TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text,
            reasoning,
        } => {
            params.ctx.handle_assistant_response(
                assistant_text,
                reasoning,
                params.response_streamed,
            )?;

            let outcome = {
                let mut t_ctx_inner = ToolOutcomeContext {
                    ctx: &mut *params.ctx,
                    repeated_tool_attempts: &mut *params.repeated_tool_attempts,
                    turn_modified_files: &mut *params.turn_modified_files,
                };

                crate::agent::runloop::unified::turn::tool_outcomes::handle_tool_calls(
                    &mut t_ctx_inner,
                    &tool_calls,
                )
                .await?
            };

            if let Some(res) = outcome {
                return Ok(res);
            }

            Ok(handle_turn_balancer(
                &mut *params.ctx,
                params.step_count,
                &mut *params.repeated_tool_attempts,
                params.max_tool_loops,
                params.tool_repeat_limit,
            )
            .await)
        }
        TurnProcessingResult::TextResponse {
            text,
            reasoning,
            proposed_plan,
        } => {
            params
                .ctx
                .handle_text_response(
                    text.clone(),
                    reasoning.clone(),
                    proposed_plan.clone(),
                    params.response_streamed,
                )
                .await
        }
        TurnProcessingResult::Empty | TurnProcessingResult::Completed => {
            Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed))
        }
        TurnProcessingResult::Cancelled => {
            *params.session_end_reason = crate::hooks::lifecycle::SessionEndReason::Cancelled;
            Ok(TurnHandlerOutcome::Break(TurnLoopResult::Cancelled))
        }
        TurnProcessingResult::Aborted => Ok(TurnHandlerOutcome::Break(TurnLoopResult::Aborted)),
    }
}
