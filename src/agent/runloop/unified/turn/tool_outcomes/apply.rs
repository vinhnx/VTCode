use crate::agent::runloop::unified::turn::context::{TurnLoopResult, TurnOutcomeContext};
use crate::agent::runloop::unified::turn::turn_loop::TurnLoopOutcome;
use anyhow::Result;
use std::time::Duration;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::session_archive::SessionMessage;

fn format_turn_elapsed_label(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    if total_seconds < 60 {
        return format!("{total_seconds}s");
    }

    if total_seconds < 3600 {
        let minutes = total_seconds / 60;
        let seconds = total_seconds % 60;
        return format!("{minutes}m {seconds}s");
    }

    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    format!("{hours}h {minutes}m")
}

pub async fn apply_turn_outcome(
    outcome: &TurnLoopOutcome,
    ctx: TurnOutcomeContext<'_>,
) -> Result<()> {
    match &outcome.result {
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
        TurnLoopResult::Exit => {
            *ctx.session_end_reason = crate::hooks::lifecycle::SessionEndReason::Exit;
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
        TurnLoopResult::Blocked { reason } => {
            *ctx.conversation_history = outcome.working_history.clone();
            if let Some(reason) = reason.as_deref() {
                let _ = ctx.renderer.line(MessageStyle::Info, reason);
            }
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
            if ctx.show_turn_timer {
                ctx.renderer.line(
                    MessageStyle::Info,
                    &format!("Worked for {}", format_turn_elapsed_label(ctx.turn_elapsed)),
                )?;
            }
            ctx.ctrl_c_state.clear_cancel();
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runloop::unified::state::CtrlCState;
    use std::collections::BTreeSet;
    use std::sync::Arc;
    use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
    use vtcode_core::ui::tui::{InlineCommand, InlineHandle};
    use vtcode_core::utils::ansi::AnsiRenderer;

    fn renderer_with_channel() -> (InlineHandle, AnsiRenderer, UnboundedReceiver<InlineCommand>) {
        let (tx, rx) = unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
        (handle, renderer, rx)
    }

    fn drain_appended_lines(receiver: &mut UnboundedReceiver<InlineCommand>) -> Vec<String> {
        let mut lines = Vec::new();
        while let Ok(command) = receiver.try_recv() {
            if let InlineCommand::AppendLine { segments, .. } = command {
                let line = segments
                    .into_iter()
                    .map(|segment| segment.text)
                    .collect::<String>();
                if !line.trim().is_empty() {
                    lines.push(line);
                }
            }
        }
        lines
    }

    #[test]
    fn format_turn_elapsed_label_mixed_adaptive() {
        assert_eq!(format_turn_elapsed_label(Duration::from_secs(59)), "59s");
        assert_eq!(format_turn_elapsed_label(Duration::from_secs(90)), "1m 30s");
        assert_eq!(
            format_turn_elapsed_label(Duration::from_secs(3600)),
            "1h 0m"
        );
    }

    #[tokio::test]
    async fn completed_turn_emits_worked_for_divider() {
        let (handle, mut renderer, mut receiver) = renderer_with_channel();
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let default_placeholder = None;
        let mut session_end_reason = crate::hooks::lifecycle::SessionEndReason::Completed;
        let mut next_checkpoint_turn = 1usize;
        let mut conversation_history = Vec::new();
        let outcome = TurnLoopOutcome {
            result: TurnLoopResult::Completed,
            working_history: vec![uni::Message::assistant("done".to_string())],
            turn_modified_files: BTreeSet::new(),
        };

        apply_turn_outcome(
            &outcome,
            TurnOutcomeContext {
                conversation_history: &mut conversation_history,
                renderer: &mut renderer,
                handle: &handle,
                ctrl_c_state: &ctrl_c_state,
                default_placeholder: &default_placeholder,
                checkpoint_manager: None,
                next_checkpoint_turn: &mut next_checkpoint_turn,
                session_end_reason: &mut session_end_reason,
                turn_elapsed: Duration::from_secs(90),
                show_turn_timer: true,
            },
        )
        .await
        .expect("apply completed outcome");

        let lines = drain_appended_lines(&mut receiver);
        assert!(lines.iter().any(|line| line == "Worked for 1m 30s"));
    }

    #[tokio::test]
    async fn cancelled_turn_does_not_emit_worked_for_divider() {
        let (handle, mut renderer, mut receiver) = renderer_with_channel();
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let default_placeholder = None;
        let mut session_end_reason = crate::hooks::lifecycle::SessionEndReason::Completed;
        let mut next_checkpoint_turn = 1usize;
        let mut conversation_history = Vec::new();
        let outcome = TurnLoopOutcome {
            result: TurnLoopResult::Cancelled,
            working_history: Vec::new(),
            turn_modified_files: BTreeSet::new(),
        };

        apply_turn_outcome(
            &outcome,
            TurnOutcomeContext {
                conversation_history: &mut conversation_history,
                renderer: &mut renderer,
                handle: &handle,
                ctrl_c_state: &ctrl_c_state,
                default_placeholder: &default_placeholder,
                checkpoint_manager: None,
                next_checkpoint_turn: &mut next_checkpoint_turn,
                session_end_reason: &mut session_end_reason,
                turn_elapsed: Duration::from_secs(90),
                show_turn_timer: true,
            },
        )
        .await
        .expect("apply cancelled outcome");

        let lines = drain_appended_lines(&mut receiver);
        assert!(!lines.iter().any(|line| line.contains("Worked for")));
    }

    #[tokio::test]
    async fn completed_turn_skips_timer_when_disabled() {
        let (handle, mut renderer, mut receiver) = renderer_with_channel();
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let default_placeholder = None;
        let mut session_end_reason = crate::hooks::lifecycle::SessionEndReason::Completed;
        let mut next_checkpoint_turn = 1usize;
        let mut conversation_history = Vec::new();
        let outcome = TurnLoopOutcome {
            result: TurnLoopResult::Completed,
            working_history: vec![uni::Message::assistant("done".to_string())],
            turn_modified_files: BTreeSet::new(),
        };

        apply_turn_outcome(
            &outcome,
            TurnOutcomeContext {
                conversation_history: &mut conversation_history,
                renderer: &mut renderer,
                handle: &handle,
                ctrl_c_state: &ctrl_c_state,
                default_placeholder: &default_placeholder,
                checkpoint_manager: None,
                next_checkpoint_turn: &mut next_checkpoint_turn,
                session_end_reason: &mut session_end_reason,
                turn_elapsed: Duration::from_secs(90),
                show_turn_timer: false,
            },
        )
        .await
        .expect("apply completed outcome with timer disabled");

        let lines = drain_appended_lines(&mut receiver);
        assert!(!lines.iter().any(|line| line.contains("Worked for")));
    }
}
