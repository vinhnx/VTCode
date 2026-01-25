//! Tool execution helpers for turn handling.

use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Notify;
use vtcode_core::tools::ToolResultCache;

use crate::agent::runloop::unified::progress::ProgressReporter;
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::tool_pipeline::{
    ToolExecutionStatus, execute_tool_with_timeout_ref,
};
use crate::agent::runloop::unified::turn::utils::safe_force_redraw;

pub(crate) struct RunTurnExecuteToolParams<'a> {
    pub tool_registry: &'a mut vtcode_core::tools::registry::ToolRegistry,
    pub name: &'a str,
    pub args_val: &'a serde_json::Value,
    pub is_read_only_tool: bool,
    pub tool_result_cache: &'a Arc<tokio::sync::RwLock<ToolResultCache>>,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub ctrl_c_notify: &'a Arc<Notify>,
    pub progress_reporter: Option<&'a ProgressReporter>,
    pub handle: &'a vtcode_core::ui::tui::InlineHandle,
    pub last_forced_redraw: &'a mut Instant,
    pub max_tool_retries: usize,
}

#[allow(dead_code)]
pub(crate) async fn run_turn_execute_tool(
    params: RunTurnExecuteToolParams<'_>,
) -> ToolExecutionStatus {
    use vtcode_core::tools::result_cache::ToolCacheKey;

    if params.is_read_only_tool {
        let _params_str = serde_json::to_string(params.args_val).unwrap_or_default();
        let cache_key = ToolCacheKey::from_json(params.name, params.args_val, "");
        {
            let mut tool_cache = params.tool_result_cache.write().await;
            if let Some(cached_output) = tool_cache.get(&cache_key) {
                #[cfg(debug_assertions)]
                tracing::debug!("Cache hit for tool: {}", params.name);

                let cached_json: serde_json::Value =
                    serde_json::from_str(&cached_output).unwrap_or(serde_json::json!({}));
                return ToolExecutionStatus::Success {
                    output: cached_json,
                    stdout: None,
                    modified_files: vec![],
                    command_success: true,
                    has_more: false,
                };
            }
        }
        safe_force_redraw(params.handle, params.last_forced_redraw);

        let result = execute_tool_with_timeout_ref(
            params.tool_registry,
            params.name,
            params.args_val,
            params.ctrl_c_state,
            params.ctrl_c_notify,
            params.progress_reporter,
            params.max_tool_retries,
        )
        .await;

        if let ToolExecutionStatus::Success { ref output, .. } = result {
            let output_json = serde_json::to_string(output).unwrap_or_else(|_| "{}".to_string());
            let mut cache = params.tool_result_cache.write().await;
            cache.insert_arc(cache_key, Arc::new(output_json));
        }

        return result;
    }

    safe_force_redraw(params.handle, params.last_forced_redraw);

    execute_tool_with_timeout_ref(
        params.tool_registry,
        params.name,
        params.args_val,
        params.ctrl_c_state,
        params.ctrl_c_notify,
        params.progress_reporter,
        params.max_tool_retries,
    )
    .await
}
