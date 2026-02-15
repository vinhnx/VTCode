use std::sync::Arc;

use serde_json::Value;
use tokio::sync::Notify;
use tracing::warn;
use vtcode_core::config::constants::tools;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::tools::result_cache::ToolResultCache;

use crate::agent::runloop::tool_output::resolve_stdout_tail_limit;
use crate::agent::runloop::unified::progress::ProgressReporter;
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner;

use super::cache::{cache_target_path, create_enhanced_cache_key, is_tool_cacheable};
use super::execution_attempts::execute_tool_with_timeout_ref_prevalidated;
use super::execution_helpers::{
    build_tool_status_message, is_loop_detection_status, parse_cached_output,
};
use super::pty_stream::PtyStreamRuntime;
use super::status::ToolExecutionStatus;

pub(super) async fn execute_with_cache_and_streaming(
    registry: &mut ToolRegistry,
    tool_result_cache: &Arc<tokio::sync::RwLock<ToolResultCache>>,
    name: &str,
    args_val: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    handle: &vtcode_core::ui::tui::InlineHandle,
    vt_cfg: Option<&VTCodeConfig>,
    max_tool_retries: usize,
) -> ToolExecutionStatus {
    let is_cacheable_tool = is_tool_cacheable(name, args_val);
    let cache_target = cache_target_path(name, args_val);

    if let Some(cached_status) = try_cache_hit(
        registry,
        tool_result_cache,
        name,
        args_val,
        &cache_target,
        is_cacheable_tool,
    )
    .await
    {
        return cached_status;
    }

    handle.force_redraw();

    let progress_reporter = ProgressReporter::new();
    progress_reporter.set_total(100).await;
    progress_reporter.set_progress(0).await;
    progress_reporter
        .set_message(format!("Starting {}...", name))
        .await;

    let status_message = build_tool_status_message(name, args_val);
    let tool_spinner = PlaceholderSpinner::with_progress(
        handle,
        Some(String::new()),
        Some(String::new()),
        status_message,
        Some(&progress_reporter),
    );

    let should_stream_pty = matches!(
        name,
        tools::RUN_PTY_CMD | tools::UNIFIED_EXEC | tools::SEND_PTY_INPUT
    );
    let mut pty_stream_runtime: Option<PtyStreamRuntime> = None;
    let previous_progress_callback = if should_stream_pty {
        let tail_limit = resolve_stdout_tail_limit(vt_cfg);
        let (runtime, callback) =
            PtyStreamRuntime::start(handle.clone(), progress_reporter.clone(), tail_limit);
        pty_stream_runtime = Some(runtime);
        Some(registry.replace_progress_callback(Some(callback)))
    } else {
        None
    };

    let outcome = execute_tool_with_timeout_ref_prevalidated(
        registry,
        name,
        args_val,
        ctrl_c_state,
        ctrl_c_notify,
        Some(&progress_reporter),
        max_tool_retries,
    )
    .await;

    if let Some(previous) = previous_progress_callback {
        let _ = registry.replace_progress_callback(previous);
    }
    if let Some(runtime) = pty_stream_runtime {
        runtime.shutdown().await;
    }

    let outcome = if is_cacheable_tool && is_loop_detection_status(&outcome) {
        match try_loop_detection_cache_hit(
            registry,
            tool_result_cache,
            name,
            args_val,
            &cache_target,
        )
        .await
        {
            Some(status) => {
                tool_spinner.finish();
                return status;
            }
            None => outcome,
        }
    } else {
        outcome
    };

    if let ToolExecutionStatus::Success {
        output,
        command_success,
        ..
    } = &outcome
    {
        tool_spinner.finish();
        if is_cacheable_tool && *command_success {
            let workspace_path = registry.workspace_root().to_string_lossy().to_string();
            let cache_key =
                create_enhanced_cache_key(name, args_val, &cache_target, &workspace_path);
            let mut cache = tool_result_cache.write().await;
            let output_json = serde_json::to_string(output).unwrap_or_else(|_| "{}".to_string());
            cache.insert_arc(cache_key, Arc::new(output_json));
        }
    }

    outcome
}

async fn try_cache_hit(
    registry: &ToolRegistry,
    tool_result_cache: &Arc<tokio::sync::RwLock<ToolResultCache>>,
    name: &str,
    args_val: &Value,
    cache_target: &str,
    is_cacheable_tool: bool,
) -> Option<ToolExecutionStatus> {
    if !is_cacheable_tool {
        return None;
    }

    let workspace_path = registry.workspace_root().to_string_lossy().to_string();
    let cache_key = create_enhanced_cache_key(name, args_val, cache_target, &workspace_path);
    let mut cache = tool_result_cache.write().await;
    if let Some(cached_output) = cache.get(&cache_key) {
        match parse_cached_output(&cached_output) {
            Ok(cached_json) => {
                tracing::debug!(
                    target: "vtcode.performance.cache",
                    "Cache hit for tool: {} (workspace: {})",
                    name,
                    workspace_path
                );
                return Some(ToolExecutionStatus::Success {
                    output: cached_json,
                    stdout: None,
                    modified_files: vec![],
                    command_success: true,
                    has_more: false,
                });
            }
            Err(error) => {
                warn!(
                    target: "vtcode.performance.cache",
                    tool = name,
                    error = %error,
                    "Discarding malformed cached output"
                );
                cache.invalidate_for_path(cache_target);
            }
        }
    } else {
        tracing::debug!(
            target: "vtcode.performance.cache",
            "Cache miss for tool: {} (workspace: {})",
            name,
            workspace_path
        );
    }
    None
}

async fn try_loop_detection_cache_hit(
    registry: &ToolRegistry,
    tool_result_cache: &Arc<tokio::sync::RwLock<ToolResultCache>>,
    name: &str,
    args_val: &Value,
    cache_target: &str,
) -> Option<ToolExecutionStatus> {
    let workspace_path = registry.workspace_root().to_string_lossy().to_string();
    let cache_key = create_enhanced_cache_key(name, args_val, cache_target, &workspace_path);
    let mut cache = tool_result_cache.write().await;
    if let Some(cached_output) = cache.get(&cache_key) {
        match parse_cached_output(&cached_output) {
            Ok(cached_json) => {
                return Some(ToolExecutionStatus::Success {
                    output: cached_json,
                    stdout: None,
                    modified_files: vec![],
                    command_success: true,
                    has_more: false,
                });
            }
            Err(error) => {
                warn!(
                    target: "vtcode.performance.cache",
                    tool = name,
                    error = %error,
                    "Discarding malformed cached output after loop detection"
                );
                cache.invalidate_for_path(cache_target);
            }
        }
    }
    None
}
