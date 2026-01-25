//! Tool outcome handlers for the agent turn loop.
//!
//! This module contains the functions for handling tool execution outcomes:
//! - Permission checking (prepare)
//! - Execution with caching
//! - Success/failure/timeout/cancelled handling

mod helpers;
mod handlers;
mod execution;
mod execution_result;
mod messages;
mod dispatch;

use crate::agent::runloop::unified::turn::context::{
    TurnLoopResult, TurnOutcomeContext,
};
use crate::agent::runloop::unified::turn::turn_loop::TurnLoopOutcome;
use anyhow::Result;
use vtcode_core::utils::session_archive::SessionMessage;

use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::MessageStyle;

pub(crate) use messages::{handle_assistant_response, handle_text_response, HandleTextResponseParams};
pub(crate) use dispatch::handle_tool_calls;

pub async fn apply_turn_outcome(
    outcome: &TurnLoopOutcome,
    ctx: TurnOutcomeContext<'_>,
) -> Result<()> {
    match outcome.result {
        TurnLoopResult::Cancelled => {
            if ctx.ctrl_c_state.is_exit_requested() {
                *ctx.session_end_reason = crate::hooks::lifecycle::SessionEndReason::Exit;
                return Ok(());
            }
            ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Interrupted current task. Press Ctrl+C again to exit.",
            )?;
            ctx.handle.clear_input();
            ctx.handle.set_placeholder(ctx.default_placeholder.clone());
            ctx.ctrl_c_state.clear_cancel();
            *ctx.session_end_reason = crate::hooks::lifecycle::SessionEndReason::Cancelled;
            Ok(())
        }
        TurnLoopResult::Aborted => {
            if let Some(last) = ctx.conversation_history.last() {
                match last.role {
                    uni::MessageRole::Assistant | uni::MessageRole::Tool => {
                        let _ = ctx.conversation_history.pop();
                    }
                    _ => {}
                }
            }
            ctx.ctrl_c_state.clear_cancel();
            Ok(())
        }
        TurnLoopResult::Blocked { reason: _ } => {
            *ctx.conversation_history = outcome.working_history.clone();
            ctx.handle.clear_input();
            ctx.handle.set_placeholder(ctx.default_placeholder.clone());
            ctx.ctrl_c_state.clear_cancel();
            Ok(())
        }
        TurnLoopResult::Completed => {
            *ctx.conversation_history = outcome.working_history.clone();
            if let Some(manager) = ctx.checkpoint_manager {
                let conversation_snapshot: Vec<SessionMessage> = outcome
                    .working_history
                    .iter()
                    .map(SessionMessage::from)
                    .collect();
                let turn_number = *ctx.next_checkpoint_turn;
                let description = outcome
                    .working_history
                    .last()
                    .map(|msg| msg.content.as_text())
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                match manager
                    .create_snapshot(
                        turn_number,
                        description.as_str(),
                        &conversation_snapshot,
                        &outcome.turn_modified_files,
                    )
                    .await
                {
                    Ok(Some(meta)) => {
                        *ctx.next_checkpoint_turn = meta.turn_number.saturating_add(1);
                    }
                    Ok(None) => {}
                    Err(err) => tracing::warn!(
                        "Failed to create checkpoint for turn {}: {}",
                        turn_number,
                        err
                    ),
                }
            }
            ctx.ctrl_c_state.clear_cancel();
            Ok(())
        }
    }
}

#[allow(dead_code)]
pub enum PrepareToolCallResult {
    Approved,
    Denied,
    Exit,
    Interrupted,
}
