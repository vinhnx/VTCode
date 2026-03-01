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
    handle: &vtcode_tui::InlineHandle,
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
        let stream_command = extract_pty_stream_command(name, args_val);
        let tail_limit = resolve_stdout_tail_limit(vt_cfg);
        let (runtime, callback) = PtyStreamRuntime::start(
            handle.clone(),
            progress_reporter.clone(),
            tail_limit,
            stream_command,
        );
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
        if is_cacheable_tool && should_cache_success_output(name, output, *command_success) {
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

fn should_cache_success_output(name: &str, output: &Value, command_success: bool) -> bool {
    if !command_success {
        return false;
    }

    if !is_command_tool(name) {
        return true;
    }

    if output.get("has_more").and_then(Value::as_bool) == Some(true) {
        return false;
    }
    if output.get("follow_up_prompt").is_some() {
        return false;
    }
    if output.get("process_id").is_some() {
        return false;
    }
    if output.get("is_exited").and_then(Value::as_bool) == Some(false) {
        return false;
    }
    if output.get("is_exited").is_some() && output.get("exit_code").is_none() {
        return false;
    }

    true
}

fn is_command_tool(name: &str) -> bool {
    matches!(
        name,
        tools::RUN_PTY_CMD
            | tools::SHELL
            | tools::UNIFIED_EXEC
            | tools::EXEC_PTY_CMD
            | tools::EXEC
            | tools::SEND_PTY_INPUT
    )
}

fn extract_pty_stream_command(tool_name: &str, args: &Value) -> Option<String> {
    let command_value = match tool_name {
        tools::RUN_PTY_CMD | tools::SHELL => {
            args.get("command").or_else(|| args.get("raw_command"))
        }
        tools::UNIFIED_EXEC | tools::EXEC_PTY_CMD | tools::EXEC => {
            let action = args.get("action").and_then(Value::as_str).or_else(|| {
                if args.get("command").is_some()
                    || args.get("cmd").is_some()
                    || args.get("raw_command").is_some()
                {
                    Some("run")
                } else {
                    None
                }
            });
            if action == Some("run") {
                args.get("command")
                    .or_else(|| args.get("cmd"))
                    .or_else(|| args.get("raw_command"))
            } else {
                None
            }
        }
        _ => None,
    }?;

    let command = command_value_to_string(command_value)?;
    match extract_command_args_suffix(args) {
        Some(suffix) => Some(format!("{} {}", command, suffix)),
        None => Some(command),
    }
}

fn command_value_to_string(value: &Value) -> Option<String> {
    if let Some(command) = value.as_str() {
        let normalized = command.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalized.is_empty() {
            None
        } else {
            Some(normalized)
        }
    } else if let Some(parts) = value.as_array() {
        let joined = parts
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if joined.is_empty() {
            None
        } else {
            Some(joined)
        }
    } else {
        None
    }
}

fn extract_command_args_suffix(args: &Value) -> Option<String> {
    let arg_values = args.get("args")?.as_array()?;
    let suffix = arg_values
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if suffix.is_empty() {
        None
    } else {
        Some(suffix)
    }
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
    let cached_output = {
        let cache = tool_result_cache.read().await;
        cache.get(&cache_key)
    };
    if let Some(cached_output) = cached_output {
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
                let mut cache = tool_result_cache.write().await;
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
    let cached_output = {
        let cache = tool_result_cache.read().await;
        cache.get(&cache_key)
    };
    if let Some(cached_output) = cached_output {
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
                let mut cache = tool_result_cache.write().await;
                cache.invalidate_for_path(cache_target);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vtcode_core::config::constants::tools;

    use super::{extract_pty_stream_command, should_cache_success_output};

    #[test]
    fn extracts_command_for_run_pty_cmd() {
        let args = json!({ "command": "cargo check -p vtcode-core" });
        assert_eq!(
            extract_pty_stream_command(tools::RUN_PTY_CMD, &args),
            Some("cargo check -p vtcode-core".to_string())
        );
    }

    #[test]
    fn extracts_command_for_unified_exec_run_action() {
        let args = json!({
            "action": "run",
            "command": ["cargo", "check", "-p", "vtcode-core"]
        });
        assert_eq!(
            extract_pty_stream_command(tools::UNIFIED_EXEC, &args),
            Some("cargo check -p vtcode-core".to_string())
        );
    }

    #[test]
    fn ignores_non_run_unified_exec_actions() {
        let args = json!({
            "action": "poll",
            "session_id": "run-123"
        });
        assert_eq!(extract_pty_stream_command(tools::UNIFIED_EXEC, &args), None);
    }

    #[test]
    fn appends_args_suffix_for_run_pty_cmd() {
        let args = json!({
            "command": "cargo",
            "args": ["check", "-p", "vtcode-core"]
        });
        assert_eq!(
            extract_pty_stream_command(tools::RUN_PTY_CMD, &args),
            Some("cargo check -p vtcode-core".to_string())
        );
    }

    #[test]
    fn caches_completed_command_outputs_only() {
        let completed = json!({
            "output": "diff --git a b",
            "exit_code": 0,
            "is_exited": true
        });
        let partial = json!({
            "output": "partial",
            "is_exited": false,
            "process_id": "run-123",
            "follow_up_prompt": "read more"
        });

        assert!(should_cache_success_output(
            tools::RUN_PTY_CMD,
            &completed,
            true
        ));
        assert!(!should_cache_success_output(
            tools::RUN_PTY_CMD,
            &partial,
            true
        ));
    }

    #[test]
    fn caches_non_command_success_outputs() {
        let output = json!({
            "matches": []
        });

        assert!(should_cache_success_output("read_file", &output, true));
    }
}
