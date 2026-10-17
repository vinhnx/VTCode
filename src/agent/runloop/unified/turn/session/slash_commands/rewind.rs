use crate::agent::runloop::ui_list;
use crate::agent::runloop::unified::reasoning::model_supports_reasoning;
use crate::agent::runloop::unified::turn::session::slash_commands::{SlashCommandContext, SlashCommandControl};
use anyhow::{Context, Result};
use chrono::{DateTime, Local, Utc};
use vtcode_core::core::agent::snapshots::{CheckpointRestore, RevertScope, SnapshotManager, SnapshotMetadata};
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_ui::tui::app::{InlineHandle, InlineListSearchConfig, InlineListSelection, RewindAction};

use super::ui;

#[cfg(test)]
fn resolve_prompt_boundary_in_history(metadata: &SnapshotMetadata, history: &[uni::Message]) -> Option<usize> {
    let prompt_text = metadata.prompt_text.as_deref().map(str::trim).filter(|value| !value.is_empty());

    if let Some(index) = metadata.prompt_message_index.filter(|index| *index < history.len()) {
        let message = &history[index];
        let matches_prompt = prompt_text.is_none_or(|text| message.content.as_text().trim() == text);
        if message.role == uni::MessageRole::User && matches_prompt {
            return Some(index);
        }
    }

    prompt_text.and_then(|prompt_text| {
        history
            .iter()
            .enumerate()
            .filter(|(_, message)| {
                message.role == uni::MessageRole::User && message.content.as_text().trim() == prompt_text
            })
            .min_by_key(|(index, _)| {
                metadata
                    .prompt_message_index
                    .map_or(usize::MAX / 2, |target| target.abs_diff(*index))
            })
            .map(|(index, _)| index)
    })
}

fn restore_prompt_input(
    handle: &InlineHandle,
    metadata: &SnapshotMetadata,
    conversation: &[vtcode_core::utils::session_archive::SessionMessage],
) -> bool {
    let Some(prompt) = metadata.resolved_prompt_text(conversation) else {
        return false;
    };
    handle.set_input(prompt.to_string());
    handle.force_redraw();
    true
}

fn restore_prompt_input_and_report(
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    metadata: &SnapshotMetadata,
    conversation: &[vtcode_core::utils::session_archive::SessionMessage],
) -> Result<()> {
    if restore_prompt_input(handle, metadata, conversation) {
        renderer.line(MessageStyle::Info, "Restored the selected prompt into the input field.")?;
    }
    Ok(())
}

pub(crate) async fn handle_open_rewind_picker(mut ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    maybe_auto_recover_pending_rewind(&mut ctx).await?;
    if !ctx.renderer.supports_inline_ui() {
        ctx.renderer.line(
            MessageStyle::Info,
            "Interactive rewind picker is available in inline UI only. Use `/rewind <turn> [conversation|code|both]`.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    if !ui::ensure_selection_ui_available(&mut ctx, "opening rewind picker")? {
        return Ok(SlashCommandControl::Continue);
    }

    let snapshots = match ctx.checkpoint_manager {
        Some(manager) => {
            manager
                .rewind_points(&ctx.tool_registry.harness_context_snapshot().session_id)
                .await
        }
        None => {
            ctx.renderer
                .line(MessageStyle::Info, "In-chat rewind requires access to the checkpoint manager.")?;
            return Ok(SlashCommandControl::Continue);
        }
    };

    let snapshots = match snapshots {
        Ok(snapshots) => snapshots,
        Err(err) => {
            ctx.renderer
                .line(MessageStyle::Error, &format!("Failed to list checkpoints: {err}"))?;
            return Ok(SlashCommandControl::Continue);
        }
    };

    if snapshots.is_empty() {
        ctx.renderer
            .line(MessageStyle::Warning, "No checkpoints available to rewind.")?;
        return Ok(SlashCommandControl::Continue);
    }

    show_rewind_checkpoint_modal(ctx.handle, &snapshots);
    let Some(selection) = ui::wait_for_list_modal_selection(&mut ctx).await else {
        ctx.renderer.line(MessageStyle::Info, "Rewind picker cancelled.")?;
        return Ok(SlashCommandControl::Continue);
    };
    let InlineListSelection::RewindCheckpoint(turn) = selection else {
        ctx.renderer
            .line(MessageStyle::Error, "Unsupported rewind checkpoint selection.")?;
        return Ok(SlashCommandControl::Continue);
    };
    let Some(snapshot) = snapshots.iter().find(|snapshot| snapshot.turn_number == turn) else {
        ctx.renderer
            .line(MessageStyle::Error, &format!("Checkpoint turn {turn} is no longer available."))?;
        return Ok(SlashCommandControl::Continue);
    };

    show_rewind_action_modal(ctx.handle, snapshot);
    let Some(selection) = ui::wait_for_list_modal_selection(&mut ctx).await else {
        ctx.renderer.line(MessageStyle::Info, "Rewind action cancelled.")?;
        return Ok(SlashCommandControl::Continue);
    };
    let InlineListSelection::RewindAction(action) = selection else {
        ctx.renderer.line(MessageStyle::Error, "Unsupported rewind action selection.")?;
        return Ok(SlashCommandControl::Continue);
    };

    match action {
        RewindAction::RestoreBoth => handle_rewind_to_turn(ctx, turn, RevertScope::Both).await,
        RewindAction::RestoreConversation => handle_rewind_to_turn(ctx, turn, RevertScope::Conversation).await,
        RewindAction::RestoreCode => handle_rewind_to_turn(ctx, turn, RevertScope::Code).await,
        RewindAction::NeverMind => {
            ctx.renderer.line(MessageStyle::Info, "Rewind cancelled.")?;
            Ok(SlashCommandControl::Continue)
        }
        RewindAction::SummarizeFromHere => {
            ctx.renderer
                .line(MessageStyle::Info, "Summarize from a checkpoint is not available. Use `/compact` instead.")?;
            Ok(SlashCommandControl::Continue)
        }
    }
}

fn rewind_checkpoint_title(metadata: &SnapshotMetadata) -> String {
    if metadata.description.trim().is_empty() {
        metadata
            .prompt_text
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("turn {}", metadata.turn_number))
    } else {
        metadata.description.clone()
    }
}

fn show_rewind_checkpoint_modal(handle: &InlineHandle, snapshots: &[SnapshotMetadata]) {
    let items = snapshots
        .iter()
        .map(|snapshot| {
            let timestamp = DateTime::<Utc>::from_timestamp(snapshot.created_at as i64, 0)
                .map(|dt| dt.with_timezone(&Local))
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| snapshot.created_at.to_string());
            let event_text = rewind_checkpoint_title(snapshot);
            ui_list::choice(
                timestamp,
                Some(event_text),
                Some(InlineListSelection::RewindCheckpoint(snapshot.turn_number)),
            )
            .with_search_value(format!(
                "{} {} {}",
                snapshot.turn_number,
                snapshot.prompt_text.clone().unwrap_or_default(),
                snapshot.description
            ))
        })
        .collect();
    handle.show_list_modal(
        "Rewind".to_string(),
        vec![
            "Select a checkpoint prompt from this session.".to_string(),
            "Then choose whether to restore code, restore conversation, or both.".to_string(),
        ],
        items,
        snapshots
            .first()
            .map(|snapshot| InlineListSelection::RewindCheckpoint(snapshot.turn_number)),
        Some(InlineListSearchConfig {
            label: "Checkpoint filter".to_string(),
            placeholder: Some("Search by prompt text or turn".to_string()),
            fuzzy: false,
        }),
    );
}

fn show_rewind_action_modal(handle: &InlineHandle, snapshot: &SnapshotMetadata) {
    let items = vec![
        ui_list::choice(
            "Rewind & Run",
            Some("Restore code and conversation, then re-run from this checkpoint.".to_string()),
            Some(InlineListSelection::RewindAction(RewindAction::RestoreBoth)),
        )
        .with_search_value("rewind run restore both code conversation".to_string()),
        ui_list::choice(
            "Rewind",
            Some("Restore conversation only, keeping current files on disk.".to_string()),
            Some(InlineListSelection::RewindAction(RewindAction::RestoreConversation)),
        )
        .with_search_value("rewind restore conversation chat".to_string()),
        ui_list::choice(
            "Restore code",
            Some("Revert tracked file edits but keep the current conversation.".to_string()),
            Some(InlineListSelection::RewindAction(RewindAction::RestoreCode)),
        )
        .with_search_value("restore code files".to_string()),
        ui_list::choice(
            "Cancel",
            Some("Close the rewind picker without changing anything.".to_string()),
            Some(InlineListSelection::RewindAction(RewindAction::NeverMind)),
        )
        .with_search_value("cancel never mind".to_string()),
    ];
    handle.show_list_modal(
        format!("Rewind turn {}", snapshot.turn_number),
        vec![
            rewind_checkpoint_title(snapshot),
            "Choose what to do with the selected checkpoint.".to_string(),
            "/redo returns to the state before this rewind.".to_string(),
        ],
        items,
        Some(InlineListSelection::RewindAction(RewindAction::RestoreBoth)),
        None,
    );
}

pub(crate) async fn handle_rewind_latest(
    mut ctx: SlashCommandContext<'_>,
    scope: RevertScope,
) -> Result<SlashCommandControl> {
    maybe_auto_recover_pending_rewind(&mut ctx).await?;
    let Some(manager) = ctx.checkpoint_manager else {
        ctx.renderer
            .line(MessageStyle::Info, "In-chat rewind requires access to the checkpoint manager.")?;
        return Ok(SlashCommandControl::Continue);
    };

    let snapshots = match manager
        .rewind_points(&ctx.tool_registry.harness_context_snapshot().session_id)
        .await
    {
        Ok(snapshots) => snapshots,
        Err(err) => {
            ctx.renderer
                .line(MessageStyle::Error, &format!("Failed to list checkpoints: {err}"))?;
            return Ok(SlashCommandControl::Continue);
        }
    };

    let Some(latest) = snapshots.first() else {
        ctx.renderer
            .line(MessageStyle::Warning, "No checkpoints available to rewind.")?;
        return Ok(SlashCommandControl::Continue);
    };

    handle_rewind_to_turn(ctx, latest.turn_number, scope).await
}

pub(crate) async fn handle_rewind_to_turn(
    mut ctx: SlashCommandContext<'_>,
    turn: usize,
    scope: RevertScope,
) -> Result<SlashCommandControl> {
    maybe_auto_recover_pending_rewind(&mut ctx).await?;
    if let Some(manager) = ctx.checkpoint_manager {
        let supports_reasoning = model_supports_reasoning(&**ctx.provider_client, &ctx.config.model);
        let result = restore_rewind_from_checkpoint(
            ctx.renderer,
            ctx.handle,
            ctx.conversation_history,
            manager,
            turn,
            scope,
            supports_reasoning,
            &ctx.tool_registry.harness_context_snapshot().session_id,
        )
        .await;
        if result.is_ok() {
            if let Some(emitter) = ctx.harness_emitter {
                let _ = emitter.emit(crate::agent::runloop::unified::inline_events::harness::harness_event(
                    vtcode_core::exec::events::HarnessEventKind::SnapshotRestored,
                    Some(format!("Rewound to turn {turn}")),
                    None,
                    None,
                    None,
                ));
            }
        }
        result?;
    } else {
        render_rewind_cli_guidance(ctx.renderer, turn, scope)?;
    }

    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_redo(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    let manager = ctx.checkpoint_manager.context("No checkpoint manager available")?;
    let current: Vec<_> = ctx
        .conversation_history
        .iter()
        .map(vtcode_core::utils::session_archive::SessionMessage::from)
        .collect();
    let restored = manager
        .navigate_prompt(None, RevertScope::Both, &ctx.tool_registry.harness_context_snapshot().session_id, &current)
        .await?;
    let supports_reasoning = model_supports_reasoning(&**ctx.provider_client, &ctx.config.model);
    render_redo_restore_success(ctx.renderer, ctx.handle, ctx.conversation_history, restored, supports_reasoning)?;
    Ok(SlashCommandControl::Continue)
}

/// Auto-recover an interrupted rewind before running the requested rewind.
///
/// Returns `Ok(true)` when a pending restore was resumed so callers can note
/// it in the transcript. Absence of a pending restore is not an error.
async fn maybe_auto_recover_pending_rewind(ctx: &mut SlashCommandContext<'_>) -> Result<bool> {
    let Some(manager) = ctx.checkpoint_manager else {
        return Ok(false);
    };
    let session_id = ctx.tool_registry.harness_context_snapshot().session_id;
    let restored = match manager.recover_pending_rewind(&session_id).await {
        Ok(restored) => restored,
        Err(_) => return Ok(false),
    };
    let supports_reasoning = model_supports_reasoning(&**ctx.provider_client, &ctx.config.model);
    render_redo_restore_success(ctx.renderer, ctx.handle, ctx.conversation_history, restored, supports_reasoning)?;
    ctx.renderer
        .line(MessageStyle::Info, "Auto-recovered an interrupted rewind before continuing.")?;
    Ok(true)
}

async fn restore_rewind_from_checkpoint(
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    conversation_history: &mut Vec<uni::Message>,
    manager: &SnapshotManager,
    turn: usize,
    scope: RevertScope,
    supports_reasoning: bool,
    session_id: &str,
) -> Result<()> {
    let current: Vec<_> = conversation_history
        .iter()
        .map(vtcode_core::utils::session_archive::SessionMessage::from)
        .collect();
    match manager.navigate_prompt(Some(turn), scope, session_id, &current).await {
        Ok(restored) => render_rewind_restore_success(
            renderer,
            handle,
            conversation_history,
            turn,
            scope,
            restored,
            supports_reasoning,
        ),
        Err(err) => Err(err.context(format!("Failed to restore checkpoint for turn {turn}"))),
    }
}

fn replace_conversation_and_rerender(
    renderer: &mut AnsiRenderer,
    conversation_history: &mut Vec<uni::Message>,
    restored: &CheckpointRestore,
    supports_reasoning: bool,
) -> Result<()> {
    *conversation_history = restored.conversation.iter().map(uni::Message::from).collect();

    renderer.clear_screen();
    let resume_lines = crate::agent::runloop::unified::session_setup::build_structured_resume_lines(
        conversation_history,
        supports_reasoning,
    );
    crate::agent::runloop::unified::session_setup::render_resume_lines(renderer, &resume_lines)?;
    Ok(())
}

fn render_rewind_restore_success(
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    conversation_history: &mut Vec<uni::Message>,
    turn: usize,
    scope: RevertScope,
    restored: CheckpointRestore,
    supports_reasoning: bool,
) -> Result<()> {
    if scope.includes_conversation() {
        replace_conversation_and_rerender(renderer, conversation_history, &restored, supports_reasoning)?;

        renderer.line(
            MessageStyle::Info,
            &format!("Restored conversation history from turn {} ({} messages)", turn, restored.conversation.len()),
        )?;
        restore_prompt_input_and_report(renderer, handle, &restored.metadata, &restored.conversation)?;
    }

    if scope.includes_code() {
        renderer.line(MessageStyle::Info, &format!("Applied code changes from turn {turn}"))?;
    }

    renderer.line(MessageStyle::Info, &format!("Successfully rewound to turn {turn} with scope {scope:?}"))?;
    Ok(())
}

fn render_redo_restore_success(
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    conversation_history: &mut Vec<uni::Message>,
    restored: CheckpointRestore,
    supports_reasoning: bool,
) -> Result<()> {
    // The redo snapshot reuses the rewind target's metadata, so its turn_number
    // still identifies the earlier checkpoint even though the payload is the
    // pre-rewind state. Report redo without inferring a turn from metadata.
    replace_conversation_and_rerender(renderer, conversation_history, &restored, supports_reasoning)?;

    renderer.line(
        MessageStyle::Info,
        &format!("Restored conversation history from before rewind ({} messages)", restored.conversation.len()),
    )?;
    restore_prompt_input_and_report(renderer, handle, &restored.metadata, &restored.conversation)?;

    renderer.line(MessageStyle::Info, "Applied code changes from before rewind")?;

    renderer.line(MessageStyle::Info, "Successfully restored state from before rewind")?;
    Ok(())
}

fn render_rewind_cli_guidance(renderer: &mut AnsiRenderer, turn: usize, scope: RevertScope) -> Result<()> {
    renderer.line(MessageStyle::Info, &format!("Rewinding to turn {turn} with scope {scope:?}..."))?;
    renderer.line(
        MessageStyle::Info,
        &format!("Use: `vtcode revert --turn {} --partial {}` from command line", turn, rewind_partial_arg(scope)),
    )?;
    renderer.line(MessageStyle::Info, "Note: In-chat rewind requires access to the checkpoint manager.")?;
    Ok(())
}

fn rewind_partial_arg(scope: RevertScope) -> &'static str {
    match scope {
        RevertScope::Conversation => "conversation",
        RevertScope::Code => "code",
        RevertScope::Both => "both",
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_prompt_boundary_in_history, rewind_checkpoint_title, rewind_partial_arg};
    use vtcode_core::core::agent::snapshots::{RevertScope, SnapshotMetadata};

    fn snapshot_metadata(description: &str, prompt_text: Option<&str>) -> SnapshotMetadata {
        SnapshotMetadata {
            id: "turn_2".to_string(),
            turn_number: 2,
            created_at: 0,
            description: description.to_string(),
            message_count: 3,
            file_count: 0,
            touched_files: Vec::new(),
            prompt_text: prompt_text.map(str::to_string),
            prompt_message_index: Some(2),
            session_id: None,
            runtime_turn_id: None,
            session_turn_number: None,
            turn_diagnostics: None,
        }
    }

    #[test]
    fn rewind_partial_arg_matches_cli_scope_values() {
        assert_eq!(rewind_partial_arg(RevertScope::Conversation), "conversation");
        assert_eq!(rewind_partial_arg(RevertScope::Code), "code");
        assert_eq!(rewind_partial_arg(RevertScope::Both), "both");
    }

    #[test]
    fn rewind_checkpoint_title_prefers_description_then_prompt_then_turn() {
        assert_eq!(rewind_checkpoint_title(&snapshot_metadata("refactor parser", Some("prompt"))), "refactor parser");
        assert_eq!(rewind_checkpoint_title(&snapshot_metadata("  ", Some("prompt text"))), "prompt text");
        assert_eq!(rewind_checkpoint_title(&snapshot_metadata("", None)), "turn 2");
        assert_eq!(rewind_checkpoint_title(&snapshot_metadata("", Some("   "))), "turn 2");
    }

    #[test]
    fn resolve_prompt_boundary_prefers_metadata_index_when_it_matches() {
        let history = vec![
            vtcode_core::llm::provider::Message::user("first".to_string()),
            vtcode_core::llm::provider::Message::assistant("reply".to_string()),
            vtcode_core::llm::provider::Message::user("target".to_string()),
        ];
        let metadata = snapshot_metadata("target", Some("target"));

        assert_eq!(resolve_prompt_boundary_in_history(&metadata, &history), Some(2));
    }

    #[test]
    fn resolve_prompt_boundary_falls_back_to_nearest_prompt_match() {
        let history = vec![
            vtcode_core::llm::provider::Message::user("target".to_string()),
            vtcode_core::llm::provider::Message::assistant("reply".to_string()),
            vtcode_core::llm::provider::Message::user("target".to_string()),
        ];
        let metadata = snapshot_metadata("target", Some("target"));

        assert_eq!(resolve_prompt_boundary_in_history(&metadata, &history), Some(2));
    }
}
