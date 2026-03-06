use std::sync::{Arc, Mutex as StdMutex};

use serde_json::Value;
use tokio::sync::Notify;
use tracing::warn;
use vtcode_core::config::constants::tools;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::tools::registry::{ToolProgressCallback, ToolRegistry};
use vtcode_core::tools::result_cache::{ToolCacheKey, ToolResultCache};

use crate::agent::runloop::tool_output::resolve_stdout_tail_limit;
use crate::agent::runloop::unified::inline_events::harness::{
    HarnessEventEmitter, tool_updated_event,
};
use crate::agent::runloop::unified::progress::ProgressReporter;
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner;

use super::cache::{
    cache_target_path, create_enhanced_cache_key, is_tool_cacheable, stream_command_parts,
};
use super::execution_attempts::execute_tool_with_timeout_ref_prevalidated;
use super::execution_helpers::{
    build_tool_status_message, is_loop_detection_status, parse_cached_output,
};
use super::pty_stream::PtyStreamRuntime;
use super::status::ToolExecutionStatus;

struct ProgressCallbackGuard<'a> {
    registry: &'a ToolRegistry,
    previous: Option<Option<ToolProgressCallback>>,
}

impl<'a> ProgressCallbackGuard<'a> {
    fn replace(registry: &'a ToolRegistry, callback: ToolProgressCallback) -> Self {
        let previous = registry.replace_progress_callback(Some(callback));
        Self {
            registry,
            previous: Some(previous),
        }
    }
}

impl Drop for ProgressCallbackGuard<'_> {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            let _ = self.registry.replace_progress_callback(previous);
        }
    }
}

fn build_streaming_progress_callback(
    base_callback: ToolProgressCallback,
    harness_emitter: Option<HarnessEventEmitter>,
    tool_item_id: &str,
) -> ToolProgressCallback {
    let Some(harness_emitter) = harness_emitter else {
        return base_callback;
    };

    let tool_item_id = tool_item_id.to_string();
    let aggregated_output = Arc::new(StdMutex::new(String::new()));

    Arc::new(move |progress_tool_name, chunk| {
        base_callback(progress_tool_name, chunk);

        let mut output = aggregated_output
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        output.push_str(chunk);
        let _ = harness_emitter.emit(tool_updated_event(tool_item_id.clone(), output.clone()));
    })
}

#[derive(Clone, Copy)]
enum CacheLookupPhase {
    Initial,
    LoopDetection,
}

impl CacheLookupPhase {
    fn malformed_entry_log(self) -> &'static str {
        match self {
            Self::Initial => "Discarding malformed cached output",
            Self::LoopDetection => "Discarding malformed cached output after loop detection",
        }
    }

    fn should_log_miss(self) -> bool {
        matches!(self, Self::Initial)
    }
}

pub(super) async fn execute_with_cache_and_streaming(
    registry: &mut ToolRegistry,
    tool_result_cache: &Arc<tokio::sync::RwLock<ToolResultCache>>,
    name: &str,
    tool_item_id: &str,
    args_val: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    handle: &vtcode_tui::InlineHandle,
    harness_emitter: Option<HarnessEventEmitter>,
    vt_cfg: Option<&VTCodeConfig>,
    max_tool_retries: usize,
) -> ToolExecutionStatus {
    let is_cacheable_tool = is_tool_cacheable(name, args_val);
    let cache_target = cache_target_path(name, args_val);

    if is_cacheable_tool
        && let Some(cached_status) = lookup_cached_status(
            registry,
            tool_result_cache,
            name,
            args_val,
            &cache_target,
            CacheLookupPhase::Initial,
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
    let _progress_callback_guard = if should_stream_pty {
        let stream_command = extract_pty_stream_command(name, args_val);
        let tail_limit = resolve_stdout_tail_limit(vt_cfg);
        let (runtime, callback) = PtyStreamRuntime::start(
            handle.clone(),
            progress_reporter.clone(),
            tail_limit,
            stream_command,
        );
        let callback = build_streaming_progress_callback(callback, harness_emitter, tool_item_id);
        pty_stream_runtime = Some(runtime);
        Some(ProgressCallbackGuard::replace(registry, callback))
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

    if let Some(runtime) = pty_stream_runtime {
        runtime.shutdown().await;
    }

    let outcome = if is_cacheable_tool && is_loop_detection_status(&outcome) {
        match lookup_cached_status(
            registry,
            tool_result_cache,
            name,
            args_val,
            &cache_target,
            CacheLookupPhase::LoopDetection,
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
            let (_, cache_key) =
                workspace_scoped_cache_key(registry, name, args_val, &cache_target);
            let output_json = serde_json::to_string(output).unwrap_or_else(|_| "{}".to_string());
            let mut cache = tool_result_cache.write().await;
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
            | "shell"
            | tools::UNIFIED_EXEC
            | "exec_pty_cmd"
            | "exec"
            | tools::SEND_PTY_INPUT
    )
}

fn extract_pty_stream_command(tool_name: &str, args: &Value) -> Option<String> {
    stream_command_parts(tool_name, args).map(|parts| parts.join(" "))
}

fn workspace_scoped_cache_key(
    registry: &ToolRegistry,
    name: &str,
    args_val: &Value,
    cache_target: &str,
) -> (String, ToolCacheKey) {
    let workspace_path = registry.workspace_root().to_string_lossy().to_string();
    let cache_key = create_enhanced_cache_key(name, args_val, cache_target, &workspace_path);
    (workspace_path, cache_key)
}

async fn lookup_cached_status(
    registry: &ToolRegistry,
    tool_result_cache: &Arc<tokio::sync::RwLock<ToolResultCache>>,
    name: &str,
    args_val: &Value,
    cache_target: &str,
    phase: CacheLookupPhase,
) -> Option<ToolExecutionStatus> {
    let (workspace_path, cache_key) =
        workspace_scoped_cache_key(registry, name, args_val, cache_target);
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
                    "{}",
                    phase.malformed_entry_log()
                );
                let mut cache = tool_result_cache.write().await;
                cache.invalidate_key(&cache_key);
            }
        }
    } else if phase.should_log_miss() {
        tracing::debug!(
            target: "vtcode.performance.cache",
            "Cache miss for tool: {} (workspace: {})",
            name,
            workspace_path
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::json;
    use vtcode_core::config::constants::tools;
    use vtcode_core::tools::registry::ToolRegistry;

    use super::{ProgressCallbackGuard, extract_pty_stream_command, should_cache_success_output};
    use tempfile::TempDir;

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

    #[tokio::test]
    async fn progress_callback_guard_restores_previous_on_drop() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        let first_hits = Arc::new(AtomicUsize::new(0));
        let first_hits_clone = Arc::clone(&first_hits);
        registry.set_progress_callback(Arc::new(move |_, _| {
            let _ = first_hits_clone.fetch_add(1, Ordering::SeqCst);
        }));

        let second_hits = Arc::new(AtomicUsize::new(0));
        let second_hits_clone = Arc::clone(&second_hits);

        {
            let _guard = ProgressCallbackGuard::replace(
                &registry,
                Arc::new(move |_, _| {
                    let _ = second_hits_clone.fetch_add(1, Ordering::SeqCst);
                }),
            );

            if let Some(current) = registry.progress_callback() {
                current("run_pty_cmd", "chunk");
            }
            assert_eq!(second_hits.load(Ordering::SeqCst), 1);
        }

        if let Some(current) = registry.progress_callback() {
            current("run_pty_cmd", "chunk");
        }
        assert_eq!(first_hits.load(Ordering::SeqCst), 1);
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
