use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use vtcode_core::core::pruning_decisions::PruningDecisionLedger;
use vtcode_core::llm::provider as uni;
use vtcode_core::ui::tui::InlineHandle;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive::{SessionArchive, SessionMessage};
use vtcode_core::utils::transcript;

use crate::agent::runloop::unified::async_mcp_manager::AsyncMcpManager;
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::workspace_links::{LinkedDirectory, remove_directory_symlink};
use crate::hooks::lifecycle::{LifecycleHookEngine, SessionEndReason};

use super::utils::render_hook_messages;

/// Export pruning decisions to JSON file for analysis
async fn export_pruning_decisions_to_json(
    ledger_arc: &Arc<RwLock<PruningDecisionLedger>>,
    session_archive_path: &std::path::Path,
) -> Result<std::path::PathBuf> {
    let ledger = ledger_arc.read().await;
    let decisions = ledger.get_decisions();
    let stats = ledger.statistics();

    let retention_ratio = if stats.total_messages_evaluated > 0 {
        stats.messages_kept as f64 / stats.total_messages_evaluated as f64
    } else {
        0.0
    };

    let export_data = serde_json::json!({
        "session_info": {
            "total_decisions": decisions.len(),
            "total_messages_evaluated": stats.total_messages_evaluated,
            "messages_kept": stats.messages_kept,
            "messages_removed": stats.messages_removed,
        },
        "statistics": {
            "retention_ratio": retention_ratio,
            "average_semantic_score_kept": stats.average_semantic_score_kept,
            "average_semantic_score_removed": stats.average_semantic_score_removed,
            "total_tokens_removed": stats.total_tokens_removed,
            "pruning_rounds": stats.pruning_rounds,
        },
        "decisions": decisions,
    });

    let json_str = serde_json::to_string_pretty(&export_data)?;
    let json_path = session_archive_path.with_extension("pruning.json");

    tokio::fs::write(&json_path, json_str).await?;
    Ok(json_path)
}

pub(super) async fn finalize_session(
    renderer: &mut AnsiRenderer,
    lifecycle_hooks: Option<&LifecycleHookEngine>,
    session_end_reason: SessionEndReason,
    session_archive: &mut Option<SessionArchive>,
    session_stats: &SessionStats,
    conversation_history: &[uni::Message],
    linked_directories: Vec<LinkedDirectory>,
    async_mcp_manager: Option<&AsyncMcpManager>,
    handle: &InlineHandle,
    pruning_ledger: Option<&Arc<RwLock<PruningDecisionLedger>>>,
) -> Result<()> {
    let transcript_lines = transcript::snapshot();

    if let Some(archive) = session_archive.take() {
        let distinct_tools = session_stats.sorted_tools();
        let total_messages = conversation_history.len();
        let session_messages: Vec<SessionMessage> = conversation_history
            .iter()
            .map(SessionMessage::from)
            .collect();

        match archive.finalize(
            transcript_lines,
            total_messages,
            distinct_tools,
            session_messages,
        ) {
            Ok(path) => {
                if let Some(hooks) = lifecycle_hooks {
                    hooks.update_transcript_path(Some(path.clone())).await;
                }
                renderer.line(
                    MessageStyle::Info,
                    &format!("Session saved to {}", path.display()),
                )?;
                renderer.line_if_not_empty(MessageStyle::Output)?;

                // Log pruning statistics if available
                if let Some(ledger_arc) = pruning_ledger {
                    let ledger = ledger_arc.read().await;
                    let report = ledger.generate_report();
                    if report.statistics.total_messages_evaluated > 0 {
                        renderer.line(MessageStyle::Info, "Context Optimization Report:")?;

                        // Summary statistics
                        renderer.line(
                            MessageStyle::Output,
                            &format!(
                                "  Messages: {} evaluated, {} kept, {} removed",
                                report.statistics.total_messages_evaluated,
                                report.statistics.messages_kept,
                                report.statistics.messages_removed
                            ),
                        )?;

                        // Retention metrics
                        renderer.line(
                            MessageStyle::Output,
                            &format!(
                                "  Retention: {:.1}% of messages preserved",
                                report.message_retention_ratio * 100.0
                            ),
                        )?;

                        // Semantic efficiency
                        renderer.line(
                            MessageStyle::Output,
                            &format!(
                                "  Semantic efficiency: {:.2} (avg value per message)",
                                report.semantic_efficiency
                            ),
                        )?;

                        // Token savings
                        if report.statistics.total_tokens_removed > 0 {
                            renderer.line(
                                MessageStyle::Output,
                                &format!(
                                    "  Tokens saved by pruning: ~{}",
                                    report.statistics.total_tokens_removed
                                ),
                            )?;
                        }

                        // Pruning rounds
                        if report.statistics.pruning_rounds > 0 {
                            renderer.line(
                                MessageStyle::Output,
                                &format!(
                                    "  Pruning rounds executed: {}",
                                    report.statistics.pruning_rounds
                                ),
                            )?;
                        }

                        renderer.line_if_not_empty(MessageStyle::Output)?;

                        // Export decision patterns to JSON for analysis
                        match export_pruning_decisions_to_json(ledger_arc, &path).await {
                            Ok(json_path) => {
                                renderer.line(
                                    MessageStyle::Info,
                                    &format!("Pruning details exported to {}", json_path.display()),
                                )?;
                            }
                            Err(e) => {
                                eprintln!("Warning: Failed to export pruning decisions: {}", e);
                            }
                        }
                    }
                }
            }
            Err(err) => {
                renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to save session: {}", err),
                )?;
                renderer.line_if_not_empty(MessageStyle::Output)?;
            }
        }
    }

    for linked in linked_directories {
        if let Err(err) = remove_directory_symlink(&linked.link_path).await {
            eprintln!(
                "Warning: failed to remove linked directory {}: {}",
                linked.link_path.display(),
                err
            );
        }
    }

    if let Some(hooks) = lifecycle_hooks {
        match hooks.run_session_end(session_end_reason).await {
            Ok(messages) => {
                render_hook_messages(renderer, &messages)?;
            }
            Err(err) => {
                renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to run session end hooks: {}", err),
                )?;
            }
        }
    }

    if let Some(mcp_manager) = async_mcp_manager {
        if let Err(e) = mcp_manager.shutdown().await {
            let error_msg = e.to_string();
            if error_msg.contains("EPIPE")
                || error_msg.contains("Broken pipe")
                || error_msg.contains("write EPIPE")
            {
                eprintln!(
                    "Info: MCP client shutdown encountered pipe errors (normal): {}",
                    e
                );
            } else {
                eprintln!("Warning: Failed to shutdown MCP client cleanly: {}", e);
            }
        }
    }

    handle.shutdown();

    transcript::clear_inline_handle();

    unsafe {
        std::env::remove_var("VTCODE_TUI_MODE");
    }

    Ok(())
}
