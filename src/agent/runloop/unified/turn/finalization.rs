#![allow(clippy::too_many_arguments)]
use anyhow::Result;
use std::io;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive::{SessionArchive, SessionMessage};
use vtcode_core::utils::transcript;
use vtcode_tui::InlineHandle;

use crate::agent::runloop::unified::async_mcp_manager::AsyncMcpManager;
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::workspace_links::{LinkedDirectory, remove_directory_symlink};
use crate::hooks::lifecycle::{LifecycleHookEngine, SessionEndReason};

use super::utils::render_hook_messages;

/// Restore terminal to a clean state after session exit
/// This ensures that raw mode is disabled and the terminal is left in a usable state
/// even if the TUI didn't exit cleanly (e.g., due to Ctrl+C)
fn restore_terminal_on_exit() -> io::Result<()> {
    // Use the centralized TUI restoration logic from vtcode-core
    // This handles draining events, clearing the line, and proper restoration order
    vtcode_tui::panic_hook::restore_tui()
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
            tracing::warn!(
                "Failed to remove linked directory {}: {}",
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

    if let Some(mcp_manager) = async_mcp_manager
        && let Err(e) = mcp_manager.shutdown().await
    {
        let error_msg = e.to_string();
        if error_msg.contains("EPIPE")
            || error_msg.contains("Broken pipe")
            || error_msg.contains("write EPIPE")
        {
            tracing::debug!(
                "MCP client shutdown encountered pipe errors (normal): {}",
                e
            );
        } else {
            tracing::warn!("Failed to shutdown MCP client cleanly: {}", e);
        }
    }

    handle.shutdown();

    // Give the TUI a brief moment to shut down cleanly before we forcefully restore
    // The TUI runs in a background task and may need a moment to clean up
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Ensure terminal is properly restored in case TUI didn't exit cleanly
    // This is critical because the TUI task may still be holding terminal state
    let _ = restore_terminal_on_exit();

    transcript::clear_inline_handle();

    unsafe {
        std::env::remove_var("VTCODE_TUI_MODE");
    }

    // Phase 4 Telemetry: Report Resilience Metrics
    let open_circuits = session_stats.circuit_breaker.get_open_circuits();
    if !open_circuits.is_empty() {
        renderer.line(
            MessageStyle::Warning,
            &format!("Open Circuit Breakers ({}):", open_circuits.len()),
        )?;
        for tool in &open_circuits {
            renderer.line(MessageStyle::Warning, &format!("  - {}", tool))?;
        }
        renderer.line_if_not_empty(MessageStyle::Output)?;
    }

    let all_stats = session_stats.tool_health_tracker.get_all_tool_stats();
    let mut unhealthy_tools: Vec<_> = all_stats
        .iter()
        .filter(|(name, _)| !session_stats.tool_health_tracker.is_healthy(name))
        .collect();

    // Sort for stable output
    unhealthy_tools.sort_by(|a, b| a.0.cmp(b.0));

    if !unhealthy_tools.is_empty() {
        renderer.line(
            MessageStyle::Warning,
            &format!("Unhealthy Tools ({}):", unhealthy_tools.len()),
        )?;
        for (name, _) in unhealthy_tools {
            let (_, reason) = session_stats.tool_health_tracker.check_health(name);
            if let Some(r) = reason {
                renderer.line(MessageStyle::Warning, &format!("  - {}: {}", name, r))?;
            }
        }
        renderer.line_if_not_empty(MessageStyle::Output)?;
    }

    Ok(())
}
