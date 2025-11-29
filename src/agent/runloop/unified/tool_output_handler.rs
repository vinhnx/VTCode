use crate::agent::runloop::mcp_events::McpPanelState;
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::tool_output_helpers::{
    check_write_effect_common, handle_modified_files_common, record_token_usage_common,
    render_error_common, render_tool_output_common,
};
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::token_budget::TokenBudgetManager;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::tools::result_cache::ToolResultCache;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::tool_output::render_tool_output;
use crate::agent::runloop::unified::run_loop_context::RunLoopContext;
use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
use crate::agent::runloop::unified::tool_summary::render_tool_call_summary_with_status;

pub(crate) async fn handle_pipeline_output(
    ctx: &mut RunLoopContext<'_>,
    name: &str,
    args_val: &serde_json::Value,
    outcome: &ToolPipelineOutcome,
    vt_config: Option<&VTCodeConfig>,
    token_budget: &TokenBudgetManager,
) -> Result<(bool, Vec<PathBuf>, Option<String>)> {
    let mut any_write_effect = false;
    let mut turn_modified_files: Vec<PathBuf> = Vec::new();
    let mut last_tool_stdout: Option<String> = None;

    match &outcome.status {
        ToolExecutionStatus::Success {
            output,
            stdout,
            modified_files,
            command_success,
            has_more: _,
        } => {
            // Record tool usage (session stats)
            ctx.session_stats.record_tool(name);

            // Handle MCP events and rendering
            if let Some(tool_name) = name.strip_prefix("mcp_") {
                let mut mcp_event = crate::agent::runloop::mcp_events::McpEvent::new(
                    "mcp".to_string(),
                    tool_name.to_string(),
                    Some(args_val.to_string()),
                );
                mcp_event.success(None);
                ctx.mcp_panel_state.add_event(mcp_event);
            } else {
                render_tool_output_common(
                    ctx.renderer,
                    name,
                    args_val,
                    output,
                    *command_success,
                    vt_config,
                    token_budget,
                )?;
            }

            // Record token usage
            record_token_usage_common(name, output, token_budget, vt_config).await?;

            last_tool_stdout = if *command_success {
                stdout.clone()
            } else {
                None
            };

            // Check for write effects and handle modified files
            if check_write_effect_common(name) {
                any_write_effect = true;
            }

            if !modified_files.is_empty() {
                for file in modified_files.iter() {
                    turn_modified_files.push(PathBuf::from(file));
                    // invalidate cache for modified files
                    let mut cache = ctx.tool_result_cache.write().await;
                    cache.invalidate_for_path(file);
                }
            }
        }
        ToolExecutionStatus::Failure { error } => {
            render_error_common(ctx.renderer, name, &error.to_string(), "failure")?;
        }
        ToolExecutionStatus::Timeout { error } => {
            render_error_common(ctx.renderer, name, &error.to_string(), "timed out")?;
        }
        ToolExecutionStatus::Cancelled => {
            ctx.renderer
                .line(MessageStyle::Info, "Tool execution cancelled")?;
        }
        ToolExecutionStatus::Progress(_) => {
            // progress handled in pipeline
        }
    }

    Ok((any_write_effect, turn_modified_files, last_tool_stdout))
}

// Minimal adapter that uses only the renderer and a subset of control structures
// This helps other parts of the codebase call into the same rendering logic without
// needing to construct a full RunLoopContext.
#[allow(dead_code)]
pub(crate) async fn handle_pipeline_output_renderer(
    renderer: &mut AnsiRenderer,
    session_stats: &mut SessionStats,
    mcp_panel_state: &mut McpPanelState,
    tool_result_cache: Option<&Arc<RwLock<ToolResultCache>>>,
    _decision_ledger: Option<&Arc<RwLock<DecisionTracker>>>,
    name: &str,
    args_val: &serde_json::Value,
    outcome: &ToolPipelineOutcome,
    vt_config: Option<&VTCodeConfig>,
    token_budget: &TokenBudgetManager,
) -> Result<(bool, Vec<PathBuf>, Option<String>)> {
    use crate::agent::runloop::mcp_events;
    use crate::agent::runloop::tool_output::render_tool_output;
    use crate::agent::runloop::unified::tool_summary::render_tool_call_summary_with_status;

    let mut any_write_effect = false;
    let mut turn_modified_files: Vec<PathBuf> = Vec::new();
    let mut last_tool_stdout: Option<String> = None;

    match &outcome.status {
        ToolExecutionStatus::Success {
            output,
            stdout,
            modified_files,
            command_success,
            has_more: _,
        } => {
            // Record tool usage
            session_stats.record_tool(name);

            if let Some(tool_name) = name.strip_prefix("mcp_") {
                let mut mcp_event = mcp_events::McpEvent::new(
                    "mcp".to_string(),
                    tool_name.to_string(),
                    Some(args_val.to_string()),
                );
                mcp_event.success(None);
                mcp_panel_state.add_event(mcp_event);
            } else {
                let exit_code = output.get("exit_code").and_then(|v| v.as_i64());
                let status_icon = if *command_success { "✓" } else { "✗" };
                render_tool_call_summary_with_status(
                    renderer,
                    name,
                    args_val,
                    status_icon,
                    exit_code,
                )?;
            }

            // Extract and record max_tokens usage if present in tool output
            if let Some(applied_max_tokens) =
                output.get("applied_max_tokens").and_then(|v| v.as_u64())
            {
                let context = format!(
                    "Tool: {}, Command: {}",
                    name,
                    output
                        .get("command")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&args_val.to_string())
                );
                let _ = token_budget
                    .record_max_tokens_usage(name, Some(applied_max_tokens as usize), &context)
                    .await;

                // Also record to global token budget for CLI access
                if let Some(global_token_budget) =
                    vtcode_core::core::global_token_manager::get_global_token_budget()
                {
                    let _ = global_token_budget
                        .record_max_tokens_usage(name, Some(applied_max_tokens as usize), &context)
                        .await;
                }
            }

            render_tool_output(renderer, Some(name), output, vt_config, Some(token_budget)).await?;

            last_tool_stdout = if *command_success {
                stdout.clone()
            } else {
                None
            };

            if matches!(
                name,
                "write_file" | "edit_file" | "create_file" | "delete_file"
            ) {
                any_write_effect = true;
            }

            if !modified_files.is_empty() {
                for file in modified_files.iter() {
                    turn_modified_files.push(PathBuf::from(file));
                    // invalidate cache for modified files if available
                    if let Some(cache_arc) = tool_result_cache {
                        let mut cache = cache_arc.write().await;
                        cache.invalidate_for_path(file);
                    }
                }
            }
        }
        ToolExecutionStatus::Failure { error } => {
            let err_msg = format!("Tool '{}' failure: {:?}", name, error);
            renderer.line(MessageStyle::Error, &err_msg)?;
        }
        ToolExecutionStatus::Timeout { error } => {
            let err_msg = format!("Tool '{}' timed out: {:?}", name, error);
            renderer.line(MessageStyle::Error, &err_msg)?;
        }
        ToolExecutionStatus::Cancelled => {
            renderer.line(MessageStyle::Info, "Tool execution cancelled")?;
        }
        ToolExecutionStatus::Progress(_) => {
            // progress handled elsewhere
        }
    }

    Ok((any_write_effect, turn_modified_files, last_tool_stdout))
}

// Adapter for TurnLoopContext (to avoid duplication when handling tool output in the turn loop)
pub(crate) async fn handle_pipeline_output_from_turn_ctx(
    ctx: &mut crate::agent::runloop::unified::turn::TurnLoopContext<'_>,
    name: &str,
    args_val: &serde_json::Value,
    outcome: &ToolPipelineOutcome,
    vt_config: Option<&VTCodeConfig>,
    token_budget: &TokenBudgetManager,
    traj: &TrajectoryLogger,
) -> Result<(bool, Vec<PathBuf>, Option<String>)> {
    // Build a RunLoopContext on top of the TurnLoopContext so we can reuse the generic handler
    use crate::agent::runloop::unified::run_loop_context::RunLoopContext as GenericRunLoopContext;

    let mut run_ctx = GenericRunLoopContext {
        renderer: ctx.renderer,
        handle: ctx.handle,
        tool_registry: ctx.tool_registry,
        tools: ctx.tools,
        tool_result_cache: ctx.tool_result_cache,
        tool_permission_cache: ctx.tool_permission_cache,
        decision_ledger: ctx.decision_ledger,
        pruning_ledger: ctx.pruning_ledger,
        session_stats: ctx.session_stats,
        mcp_panel_state: ctx.mcp_panel_state,
        approval_recorder: ctx.approval_recorder,
        session: ctx.session,
        traj,
    };

    handle_pipeline_output(
        &mut run_ctx,
        name,
        args_val,
        outcome,
        vt_config,
        token_budget,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::RwLock;
    use vtcode_core::acp::ToolPermissionCache;
    use vtcode_core::config::loader::VTCodeConfig;
    use vtcode_core::core::pruning_decisions::PruningDecisionLedger;
    use vtcode_core::core::trajectory::TrajectoryLogger;
    use vtcode_core::tools::ApprovalRecorder;
    use vtcode_core::tools::registry::ToolRegistry;
    use vtcode_core::tools::result_cache::CacheKey;
    use vtcode_core::ui::theme;
    use vtcode_core::ui::tui::{spawn_session, theme_from_styles};

    // Use Tokio runtime for async test blocks
    #[tokio::test]
    async fn test_renderer_records_tool_and_invalidates_cache_on_modified_file() {
        // Setup a stdout renderer
        let mut renderer = vtcode_core::utils::ansi::AnsiRenderer::stdout();

        // Prepare session stats and mcp state
        let mut stats = SessionStats::default();
        let mut mcp = McpPanelState::default();

        // Initialize a small cache and insert an entry for /tmp/foo.txt
        let cache = Arc::new(RwLock::new(ToolResultCache::new(8)));
        let key = CacheKey::new("read_file", "{}", "/tmp/foo.txt");
        {
            let mut c = cache.write().await;
            c.insert_arc(key.clone(), Arc::new("{}".to_string()));
            assert!(c.get(&key).is_some());
        }

        // Create an outcome that indicates write to /tmp/foo.txt
        let output_json = serde_json::json!({"result":"ok"});
        let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: output_json.clone(),
            stdout: None,
            modified_files: vec!["/tmp/foo.txt".to_string()],
            command_success: true,
            has_more: false,
        });

        // Invoke the renderer adapter
        let (_any_write, mod_files, _last_stdout) = handle_pipeline_output_renderer(
            &mut renderer,
            &mut stats,
            &mut mcp,
            Some(&cache),
            None,
            "write_file",
            &serde_json::json!({}),
            &outcome,
            None::<&VTCodeConfig>,
            &TokenBudgetManager::default(),
        )
        .await
        .expect("render should succeed");

        // Confirm the function recorded the tool call
        let recorded = stats.sorted_tools();
        assert!(recorded.contains(&"write_file".to_string()));

        // Confirm the modified files list contains our path
        assert_eq!(mod_files, vec![PathBuf::from("/tmp/foo.txt")]);

        // Ensure cache invalidation removed the entry
        {
            let mut c = cache.write().await;
            assert!(c.get(&key).is_none());
        }
    }

    #[tokio::test]
    async fn test_renderer_records_mcp_event_for_mcp_tool() {
        let mut renderer = vtcode_core::utils::ansi::AnsiRenderer::stdout();

        // Note: tests involving `apply_turn_outcome` live in `turn/turn_loop.rs` and can be added there
        let mut stats = SessionStats::default();
        let mut mcp = McpPanelState::new(32, true); // enabled
        let cache = Arc::new(RwLock::new(ToolResultCache::new(8)));

        let output_json = serde_json::json!({"exit_code":0});
        let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: output_json.clone(),
            stdout: Some("ok".to_string()),
            modified_files: vec![],
            command_success: true,
            has_more: false,
        });

        let (_any_write, _mod_files, _last_stdout) = handle_pipeline_output_renderer(
            &mut renderer,
            &mut stats,
            &mut mcp,
            Some(&cache),
            None,
            "mcp_example",
            &serde_json::json!({}),
            &outcome,
            None::<&VTCodeConfig>,
            &TokenBudgetManager::default(),
        )
        .await
        .expect("render should succeed");

        // Ensure mcp panel recorded an event
        assert!(mcp.event_count() > 0);
    }

    #[tokio::test]
    async fn test_handle_pipeline_output_invalidates_cache_and_records_stats() {
        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path().to_path_buf();

        let mut registry = ToolRegistry::new(workspace.clone()).await;
        let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));

        let mut session = spawn_session(
            theme_from_styles(&theme::active_styles()),
            None,
            vtcode_core::config::types::UiSurfacePreference::default(),
            10,
            false,
            None,
        )
        .unwrap();
        let handle = session.clone_inline_handle();
        let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

        let cache = Arc::new(RwLock::new(ToolResultCache::new(8)));
        let key = CacheKey::new("read_file", "{}", "/tmp/foo.txt");
        {
            let mut c = cache.write().await;
            c.insert_arc(key.clone(), Arc::new("{}".to_string()));
            assert!(c.get(&key).is_some());
        }

        let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
        let pruning_ledger = Arc::new(RwLock::new(PruningDecisionLedger::new()));
        let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
        let mut mcp_panel = crate::agent::runloop::mcp_events::McpPanelState::new(10, true);
        let approval_recorder = ApprovalRecorder::new(workspace.clone());
        let traj = TrajectoryLogger::new(&workspace);
        let tools = Arc::new(RwLock::new(Vec::new()));

        let mut ctx = RunLoopContext {
            renderer: &mut renderer,
            handle: &handle,
            tool_registry: &mut registry,
            tools: &tools,
            tool_result_cache: &cache,
            tool_permission_cache: &permission_cache_arc,
            decision_ledger: &decision_ledger,
            pruning_ledger: &pruning_ledger,
            session_stats: &mut session_stats,
            mcp_panel_state: &mut mcp_panel,
            approval_recorder: &approval_recorder,
            session: &mut session,
            traj: &traj,
        };

        let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: serde_json::json!({"ok": true}),
            stdout: None,
            modified_files: vec!["/tmp/foo.txt".to_string()],
            command_success: true,
            has_more: false,
        });

        let (any_write, mod_files, _last_stdout) = handle_pipeline_output(
            &mut ctx,
            "read_file",
            &serde_json::json!({}),
            &outcome,
            None::<&VTCodeConfig>,
            &TokenBudgetManager::default(),
        )
        .await
        .expect("handle should succeed");

        assert!(!any_write); // read_file not write
        assert_eq!(mod_files, vec![PathBuf::from("/tmp/foo.txt")]);

        // Ensure cache invalidated
        {
            let mut c = cache.write().await;
            assert!(c.get(&key).is_none());
        }

        // Ensure session stats were updated
        let rec = session_stats.sorted_tools();
        assert!(rec.contains(&"read_file".to_string()));
    }

    #[tokio::test]
    async fn test_handle_pipeline_output_mcp_events() {
        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path().to_path_buf();

        let mut registry = ToolRegistry::new(workspace.clone()).await;
        let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));

        let mut session = spawn_session(
            theme_from_styles(&theme::active_styles()),
            None,
            vtcode_core::config::types::UiSurfacePreference::default(),
            10,
            false,
            None,
        )
        .unwrap();
        let handle = session.clone_inline_handle();
        let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

        let cache = Arc::new(RwLock::new(ToolResultCache::new(8)));
        let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
        let pruning_ledger = Arc::new(RwLock::new(PruningDecisionLedger::new()));
        let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
        let mut mcp_panel = crate::agent::runloop::mcp_events::McpPanelState::new(10, true);
        let approval_recorder = ApprovalRecorder::new(workspace.clone());
        let traj = TrajectoryLogger::new(&workspace);
        let tools = Arc::new(RwLock::new(Vec::new()));

        let mut ctx = RunLoopContext {
            renderer: &mut renderer,
            handle: &handle,
            tool_registry: &mut registry,
            tools: &tools,
            tool_result_cache: &cache,
            tool_permission_cache: &permission_cache_arc,
            decision_ledger: &decision_ledger,
            pruning_ledger: &pruning_ledger,
            session_stats: &mut session_stats,
            mcp_panel_state: &mut mcp_panel,
            approval_recorder: &approval_recorder,
            session: &mut session,
            traj: &traj,
        };

        let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: serde_json::json!({"exit_code": 0}),
            stdout: Some("ok".to_string()),
            modified_files: vec![],
            command_success: true,
            has_more: false,
        });

        let (_any_write, _mod_files, _last_stdout) = handle_pipeline_output(
            &mut ctx,
            "mcp_example",
            &serde_json::json!({}),
            &outcome,
            None::<&VTCodeConfig>,
            &TokenBudgetManager::default(),
        )
        .await
        .expect("handle should succeed");

        assert!(ctx.mcp_panel_state.event_count() > 0);
    }
}
