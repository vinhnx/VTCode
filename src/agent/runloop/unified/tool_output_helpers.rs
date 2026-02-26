//! Helper functions to eliminate code duplication in tool output handlers
#![allow(dead_code, clippy::too_many_arguments)]

use crate::agent::runloop::mcp_events::McpPanelState;
use crate::agent::runloop::unified::state::SessionStats;
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::tools::result_cache::ToolResultCache;
use vtcode_core::tools::tool_intent;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

/// Common logic for recording tool usage across all handlers
pub fn record_tool_usage_common(session_stats: &mut SessionStats, name: &str) {
    session_stats.record_tool(name);
}

/// Common logic for handling MCP tool events
pub async fn handle_mcp_event_common(
    mcp_panel_state: &mut McpPanelState,
    tool_name: &str,
    args_val: &serde_json::Value,
    output: Option<&serde_json::Value>,
    is_success: bool,
    renderer: &mut AnsiRenderer,
    vt_config: Option<&VTCodeConfig>,
) -> Result<()> {
    let mut mcp_event = crate::agent::runloop::mcp_events::McpEvent::new(
        "mcp".to_string(),
        tool_name.to_string(),
        Some(args_val.to_string()),
    );

    if is_success {
        mcp_event.success(None);
        if let Some(output_val) = output {
            crate::agent::runloop::tool_output::render_tool_output(
                renderer,
                Some(&format!("mcp_{}", tool_name)),
                output_val,
                vt_config,
            )
            .await?;
        }
    } else {
        let error_msg = output
            .and_then(|o| o.get("error"))
            .and_then(|e| e.as_str())
            .unwrap_or("Unknown error");
        mcp_event.failure(Some(error_msg.to_string()));

        let error_json = serde_json::json!({ "error": error_msg });
        crate::agent::runloop::tool_output::render_tool_output(
            renderer,
            Some(&format!("mcp_{}", tool_name)),
            &error_json,
            vt_config,
        )
        .await?;
    }

    mcp_panel_state.add_event(mcp_event);
    Ok(())
}

/// Common logic for caching tool results
pub async fn cache_tool_result_common(
    tool_result_cache: Option<&Arc<RwLock<ToolResultCache>>>,
    name: &str,
    args_val: &serde_json::Value,
    output: &serde_json::Value,
) -> Result<()> {
    if let Some(cache) = tool_result_cache {
        let mut cache_guard = cache.write().await;
        let output_str = serde_json::to_string(output).unwrap_or_default();
        let cache_key =
            vtcode_core::tools::result_cache::ToolCacheKey::from_json(name, args_val, "");
        cache_guard.insert(cache_key, output_str);
    }
    Ok(())
}

/// Common logic for handling modified files
pub fn handle_modified_files_common(
    modified_files: &[PathBuf],
    tool_result_cache: Option<&Arc<RwLock<ToolResultCache>>>,
    any_write_effect: &mut bool,
    turn_modified_files: &mut Vec<PathBuf>,
) -> Result<()> {
    if !modified_files.is_empty() {
        *any_write_effect = true;
        turn_modified_files.extend(modified_files.iter().cloned());

        // Invalidate cache for modified files
        if let Some(cache) = tool_result_cache {
            let mut cache_guard = cache.blocking_write();
            for file in modified_files {
                cache_guard.invalidate_for_path(file.to_str().unwrap_or(""));
            }
        }
    }
    Ok(())
}

/// Common logic for determining if a tool causes write effects
pub fn check_write_effect_common(name: &str) -> bool {
    matches!(
        name,
        "write_file" | "edit_file" | "create_file" | "delete_file"
    )
}

fn is_run_pty_tool(name: &str, args_val: &serde_json::Value) -> bool {
    if matches!(name, tool_names::RUN_PTY_CMD | tool_names::SHELL) {
        return true;
    }
    if name == tool_names::UNIFIED_EXEC {
        return tool_intent::unified_exec_action(args_val).unwrap_or("run") == "run";
    }
    false
}

fn compact_run_completion_line(
    output: &serde_json::Value,
    command_success: bool,
) -> Option<String> {
    if let Some(exit_code) = output.get("exit_code").and_then(serde_json::Value::as_i64) {
        if exit_code == 0 {
            return Some("✓ run completed (exit code: 0)".to_string());
        }
        return Some(format!("✗ run error, exit code: {}", exit_code));
    }

    if output.get("is_exited").and_then(serde_json::Value::as_bool) == Some(true) {
        if command_success {
            return Some("✓ done".to_string());
        }
        return Some("✗ failed".to_string());
    }

    None
}

/// Common logic for rendering tool output with error handling
pub async fn render_tool_output_common(
    renderer: &mut AnsiRenderer,
    name: &str,
    args_val: &serde_json::Value,
    output: &serde_json::Value,
    command_success: bool,
    vt_config: Option<&VTCodeConfig>,
) -> Result<()> {
    if renderer.supports_inline_ui() && is_run_pty_tool(name, args_val) {
        if let Some(completion) = compact_run_completion_line(output, command_success) {
            renderer.line(MessageStyle::ToolDetail, &completion)?;
        }
        return Ok(());
    }

    let stream_label = crate::agent::runloop::unified::tool_summary::stream_label_from_output(
        output,
        command_success,
    );
    crate::agent::runloop::unified::tool_summary::render_tool_call_summary(
        renderer,
        name,
        args_val,
        stream_label,
    )?;

    crate::agent::runloop::tool_output::render_tool_output(renderer, Some(name), output, vt_config)
        .await
}

/// Common logic for rendering error messages
pub fn render_error_common(
    renderer: &mut AnsiRenderer,
    name: &str,
    error: &str,
    error_type: &str,
) -> Result<()> {
    let err_msg = format!("Tool '{}' {}: {}", name, error_type, error);
    renderer.line(vtcode_core::utils::ansi::MessageStyle::Error, &err_msg)?;
    Ok(())
}

/// Common logic for handling tool success
pub async fn handle_tool_success_common(
    session_stats: &mut SessionStats,
    name: &str,
    args_val: &serde_json::Value,
    output: &serde_json::Value,
    stdout: &Option<String>,
    modified_files: &[PathBuf],
    command_success: bool,
    _vt_config: Option<&VTCodeConfig>,
    tool_result_cache: Option<&Arc<RwLock<ToolResultCache>>>,
    any_write_effect: &mut bool,
    turn_modified_files: &mut Vec<PathBuf>,
    last_tool_stdout: &mut Option<String>,
) -> Result<()> {
    // Record tool usage
    record_tool_usage_common(session_stats, name);

    // Cache successful tool results
    cache_tool_result_common(tool_result_cache, name, args_val, output).await?;

    // Handle modified files
    handle_modified_files_common(
        modified_files,
        tool_result_cache,
        any_write_effect,
        turn_modified_files,
    )?;

    // Check for write effects
    if check_write_effect_common(name) {
        *any_write_effect = true;
    }

    // Store stdout for potential follow-up processing
    if command_success {
        *last_tool_stdout = stdout.clone();
    }

    Ok(())
}
