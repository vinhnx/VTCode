use crate::agent::runloop::unified::turn::session::slash_commands::{
    SlashCommandContext, SlashCommandControl,
};
use anyhow::Result;
use chrono::{DateTime, Local, Utc};
use vtcode_core::core::agent::snapshots::{
    CheckpointRestore, RevertScope, SnapshotManager, SnapshotMetadata,
};
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_ui::tui::app::RewindAction;
use vtcode_ui::tui::app::{
    InlineHandle, InlineListItem, InlineListSearchConfig, InlineListSelection,
};

use super::ui;

fn resolve_prompt_boundary_in_history(
    metadata: &SnapshotMetadata,
    history: &[uni::Message],
) -> Option<usize> {
    let prompt_text = metadata
        .prompt_text
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if let Some(index) = metadata
        .prompt_message_index
        .filter(|index| *index < history.len())
    {
        let message = &history[index];
        let matches_prompt =
            prompt_text.is_none_or(|text| message.content.as_text().trim() == text);
        if message.role == uni::MessageRole::User && matches_prompt {
            return Some(index);
        }
    }

    prompt_text.and_then(|prompt_text| {
        history
            .iter()
            .enumerate()
            .filter(|(_, message)| {
                message.role == uni::MessageRole::User
                    && message.content.as_text().trim() == prompt_text
            })
            .min_by_key(|(index, _)| {
                metadata
                    .prompt_message_index
                    .map_or(usize::MAX / 2, |target| target.abs_diff(*index))
            })
            .map(|(index, _)| index)
    })
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

fn rewind_checkpoint_subtitle(metadata: &SnapshotMetadata) -> String {
    let created = DateTime::<Utc>::from_timestamp(metadata.created_at as i64, 0)
        .map(|dt| dt.with_timezone(&Local))
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| metadata.created_at.to_string());
    format!(
        "turn {}  {created}  {} msgs  {} files",
        metadata.turn_number, metadata.message_count, metadata.file_count
    )
}

fn show_rewind_checkpoint_modal(handle: &InlineHandle, snapshots: &[SnapshotMetadata]) {
    let items = snapshots
        .iter()
        .map(|snapshot| InlineListItem {
            title: rewind_checkpoint_title(snapshot),
            subtitle: Some(rewind_checkpoint_subtitle(snapshot)),
            badge: Some(format!("turn {}", snapshot.turn_number)),
            indent: 0,
            selection: Some(InlineListSelection::RewindCheckpoint(snapshot.turn_number)),
            search_value: Some(format!(
                "{} {} {}",
                snapshot.turn_number,
                snapshot.prompt_text.clone().unwrap_or_default(),
                snapshot.description
            )),
        })
        .collect();
    handle.show_list_modal(
        "Rewind".to_string(),
        vec![
            "Select a checkpoint prompt from this session.".to_string(),
            "Then choose whether to restore code, restore conversation, or summarize from that point.".to_string(),
        ],
        items,
        snapshots
            .first()
            .map(|snapshot| InlineListSelection::RewindCheckpoint(snapshot.turn_number)),
        Some(InlineListSearchConfig {
            label: "Checkpoint filter".to_string(),
            placeholder: Some("Search by prompt text or turn".to_string()),
        }),
    );
}

fn show_rewind_action_modal(handle: &InlineHandle, snapshot: &SnapshotMetadata) {
    let items = vec![
        InlineListItem {
            title: "Restore code and conversation".to_string(),
            subtitle: Some("Revert both files and conversation to this checkpoint.".to_string()),
            badge: Some("Both".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::RewindAction(RewindAction::RestoreBoth)),
            search_value: Some("restore both code conversation".to_string()),
        },
        InlineListItem {
            title: "Restore conversation".to_string(),
            subtitle: Some("Rewind the transcript but keep current files on disk.".to_string()),
            badge: Some("Chat".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::RewindAction(
                RewindAction::RestoreConversation,
            )),
            search_value: Some("restore conversation chat".to_string()),
        },
        InlineListItem {
            title: "Restore code".to_string(),
            subtitle: Some(
                "Revert tracked file edits but keep the current conversation.".to_string(),
            ),
            badge: Some("Code".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::RewindAction(RewindAction::RestoreCode)),
            search_value: Some("restore code files".to_string()),
        },
        InlineListItem {
            title: "Summarize from here".to_string(),
            subtitle: Some(
                "Compact the selected prompt onward without changing files.".to_string(),
            ),
            badge: Some("Summary".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::RewindAction(
                RewindAction::SummarizeFromHere,
            )),
            search_value: Some("summarize compact history".to_string()),
        },
        InlineListItem {
            title: "Never mind".to_string(),
            subtitle: Some("Close the rewind picker without changing anything.".to_string()),
            badge: Some("Cancel".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::RewindAction(RewindAction::NeverMind)),
            search_value: Some("cancel never mind".to_string()),
        },
    ];
    handle.show_list_modal(
        format!("Rewind turn {}", snapshot.turn_number),
        vec![
            rewind_checkpoint_title(snapshot),
            "Choose what to do with the selected checkpoint.".to_string(),
        ],
        items,
        Some(InlineListSelection::RewindAction(RewindAction::RestoreBoth)),
        None,
    );
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
        renderer.line(
            MessageStyle::Info,
            "Restored the selected prompt into the input field.",
        )?;
    }
    Ok(())
}

pub(crate) async fn handle_open_rewind_picker(
    mut ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
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
        Some(manager) => manager.list_snapshots().await,
        None => {
            ctx.renderer.line(
                MessageStyle::Info,
                "In-chat rewind requires access to the checkpoint manager.",
            )?;
            return Ok(SlashCommandControl::Continue);
        }
    };

    let snapshots = match snapshots {
        Ok(snapshots) => snapshots,
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to list checkpoints: {}", err),
            )?;
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
        ctx.renderer
            .line(MessageStyle::Info, "Rewind picker cancelled.")?;
        return Ok(SlashCommandControl::Continue);
    };
    let InlineListSelection::RewindCheckpoint(turn) = selection else {
        ctx.renderer.line(
            MessageStyle::Error,
            "Unsupported rewind checkpoint selection.",
        )?;
        return Ok(SlashCommandControl::Continue);
    };
    let Some(snapshot) = snapshots
        .iter()
        .find(|snapshot| snapshot.turn_number == turn)
    else {
        ctx.renderer.line(
            MessageStyle::Error,
            &format!("Checkpoint turn {} is no longer available.", turn),
        )?;
        return Ok(SlashCommandControl::Continue);
    };

    show_rewind_action_modal(ctx.handle, snapshot);
    let Some(selection) = ui::wait_for_list_modal_selection(&mut ctx).await else {
        ctx.renderer
            .line(MessageStyle::Info, "Rewind action cancelled.")?;
        return Ok(SlashCommandControl::Continue);
    };
    let InlineListSelection::RewindAction(action) = selection else {
        ctx.renderer
            .line(MessageStyle::Error, "Unsupported rewind action selection.")?;
        return Ok(SlashCommandControl::Continue);
    };

    match action {
        RewindAction::RestoreBoth => handle_rewind_to_turn(ctx, turn, RevertScope::Both).await,
        RewindAction::RestoreConversation => {
            handle_rewind_to_turn(ctx, turn, RevertScope::Conversation).await
        }
        RewindAction::RestoreCode => handle_rewind_to_turn(ctx, turn, RevertScope::Code).await,
        RewindAction::SummarizeFromHere => summarize_rewind_from_checkpoint(ctx, turn).await,
        RewindAction::NeverMind => {
            ctx.renderer.line(MessageStyle::Info, "Rewind cancelled.")?;
            Ok(SlashCommandControl::Continue)
        }
    }
}

pub(crate) async fn handle_rewind_latest(
    ctx: SlashCommandContext<'_>,
    scope: RevertScope,
) -> Result<SlashCommandControl> {
    let Some(manager) = ctx.checkpoint_manager else {
        ctx.renderer.line(
            MessageStyle::Info,
            "In-chat rewind requires access to the checkpoint manager.",
        )?;
        return Ok(SlashCommandControl::Continue);
    };

    let snapshots = match manager.list_snapshots().await {
        Ok(snapshots) => snapshots,
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to list checkpoints: {}", err),
            )?;
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
    ctx: SlashCommandContext<'_>,
    turn: usize,
    scope: RevertScope,
) -> Result<SlashCommandControl> {
    if let Some(manager) = ctx.checkpoint_manager {
        restore_rewind_from_checkpoint(
            ctx.renderer,
            ctx.handle,
            ctx.conversation_history,
            manager,
            turn,
            scope,
        )
        .await?;
    } else {
        render_rewind_cli_guidance(ctx.renderer, turn, scope)?;
    }

    Ok(SlashCommandControl::Continue)
}

async fn restore_rewind_from_checkpoint(
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    conversation_history: &mut Vec<uni::Message>,
    manager: &SnapshotManager,
    turn: usize,
    scope: RevertScope,
) -> Result<()> {
    match manager.restore_snapshot(turn, scope).await {
        Ok(Some(restored)) => render_rewind_restore_success(
            renderer,
            handle,
            conversation_history,
            turn,
            scope,
            restored,
        ),
        Ok(None) => renderer.line(
            MessageStyle::Error,
            &format!("No checkpoint found for turn {}", turn),
        ),
        Err(err) => renderer.line(
            MessageStyle::Error,
            &format!("Failed to restore checkpoint for turn {}: {}", turn, err),
        ),
    }
}

fn render_rewind_restore_success(
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    conversation_history: &mut Vec<uni::Message>,
    turn: usize,
    scope: RevertScope,
    restored: CheckpointRestore,
) -> Result<()> {
    if scope.includes_conversation() {
        *conversation_history = restored
            .conversation
            .iter()
            .map(uni::Message::from)
            .collect();
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Restored conversation history from turn {} ({} messages)",
                turn,
                restored.conversation.len()
            ),
        )?;
        restore_prompt_input_and_report(
            renderer,
            handle,
            &restored.metadata,
            &restored.conversation,
        )?;
    }

    if scope.includes_code() {
        renderer.line(
            MessageStyle::Info,
            &format!("Applied code changes from turn {}", turn),
        )?;
    }

    renderer.line(
        MessageStyle::Info,
        &format!(
            "Successfully rewound to turn {} with scope {:?}",
            turn, scope
        ),
    )?;
    Ok(())
}

async fn summarize_rewind_from_checkpoint(
    ctx: SlashCommandContext<'_>,
    turn: usize,
) -> Result<SlashCommandControl> {
    let Some(manager) = ctx.checkpoint_manager else {
        render_rewind_cli_guidance(ctx.renderer, turn, RevertScope::Conversation)?;
        return Ok(SlashCommandControl::Continue);
    };
    let restored = match manager.load_snapshot(turn).await {
        Ok(Some(restored)) => restored,
        Ok(None) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("No checkpoint found for turn {}", turn),
            )?;
            return Ok(SlashCommandControl::Continue);
        }
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to load checkpoint for turn {}: {}", turn, err),
            )?;
            return Ok(SlashCommandControl::Continue);
        }
    };

    let Some(start_index) =
        resolve_prompt_boundary_in_history(&restored.metadata, ctx.conversation_history.as_slice())
    else {
        ctx.renderer.line(
            MessageStyle::Error,
            &format!("Could not locate the prompt boundary for turn {}.", turn),
        )?;
        return Ok(SlashCommandControl::Continue);
    };

    let harness_snapshot = ctx.tool_registry.harness_context_snapshot();
    let outcome =
        crate::agent::runloop::unified::turn::compaction::compact_history_from_index_in_place(
            ctx.provider_client.as_ref(),
            &ctx.config.model,
            &harness_snapshot.session_id,
            &ctx.config.workspace,
            ctx.vt_cfg.as_ref(),
            ctx.conversation_history,
            start_index,
            ctx.session_stats,
            ctx.context_manager,
        )
        .await;

    match outcome {
        Ok(Some(result)) => {
            restore_prompt_input_and_report(
                ctx.renderer,
                ctx.handle,
                &restored.metadata,
                &restored.conversation,
            )?;
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "Summarized conversation from turn {} ({} -> {} messages).",
                    turn, result.original_len, result.compacted_len
                ),
            )?;
            ctx.renderer
                .line(MessageStyle::Info, "Files on disk were left unchanged.")?;
        }
        Ok(None) => {
            restore_prompt_input_and_report(
                ctx.renderer,
                ctx.handle,
                &restored.metadata,
                &restored.conversation,
            )?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Conversation is already compact from that point.",
            )?;
        }
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to summarize from turn {}: {}", turn, err),
            )?;
        }
    }

    Ok(SlashCommandControl::Continue)
}

fn render_rewind_cli_guidance(
    renderer: &mut AnsiRenderer,
    turn: usize,
    scope: RevertScope,
) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        &format!("Rewinding to turn {} with scope {:?}...", turn, scope),
    )?;
    renderer.line(
        MessageStyle::Info,
        &format!(
            "Use: `vtcode revert --turn {} --partial {}` from command line",
            turn,
            rewind_partial_arg(scope)
        ),
    )?;
    renderer.line(
        MessageStyle::Info,
        "Note: In-chat rewind requires access to the checkpoint manager.",
    )?;
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
    use super::{resolve_prompt_boundary_in_history, rewind_partial_arg};
    use vtcode_core::core::agent::snapshots::{RevertScope, SnapshotMetadata};

    #[test]
    fn rewind_partial_arg_matches_cli_scope_values() {
        assert_eq!(
            rewind_partial_arg(RevertScope::Conversation),
            "conversation"
        );
        assert_eq!(rewind_partial_arg(RevertScope::Code), "code");
        assert_eq!(rewind_partial_arg(RevertScope::Both), "both");
    }

    #[test]
    fn resolve_prompt_boundary_prefers_metadata_index_when_it_matches() {
        let history = vec![
            vtcode_core::llm::provider::Message::user("first".to_string()),
            vtcode_core::llm::provider::Message::assistant("reply".to_string()),
            vtcode_core::llm::provider::Message::user("target".to_string()),
        ];
        let metadata = SnapshotMetadata {
            id: "turn_2".to_string(),
            turn_number: 2,
            created_at: 0,
            description: "target".to_string(),
            message_count: 3,
            file_count: 0,
            prompt_text: Some("target".to_string()),
            prompt_message_index: Some(2),
        };

        assert_eq!(
            resolve_prompt_boundary_in_history(&metadata, &history),
            Some(2)
        );
    }

    #[test]
    fn resolve_prompt_boundary_falls_back_to_nearest_prompt_match() {
        let history = vec![
            vtcode_core::llm::provider::Message::user("target".to_string()),
            vtcode_core::llm::provider::Message::assistant("reply".to_string()),
            vtcode_core::llm::provider::Message::user("target".to_string()),
        ];
        let metadata = SnapshotMetadata {
            id: "turn_2".to_string(),
            turn_number: 2,
            created_at: 0,
            description: "target".to_string(),
            message_count: 3,
            file_count: 0,
            prompt_text: Some("target".to_string()),
            prompt_message_index: Some(2),
        };

        assert_eq!(
            resolve_prompt_boundary_in_history(&metadata, &history),
            Some(2)
        );
    }
}
