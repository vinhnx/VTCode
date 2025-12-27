use crate::agent::runloop::unified::tool_output_handler::handle_pipeline_output_renderer;
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::mcp_events::McpPanelState;
use vtcode_core::tools::result_cache::{ToolResultCache, ToolCacheKey};
use vtcode_core::utils::ansi::AnsiRenderer;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::test]
async fn handle_pipeline_output_renderer_invalidates_cache_and_records_tool() {
    let mut renderer = AnsiRenderer::stdout();
    let mut session_stats = SessionStats::default();
    let mut mcp_state = McpPanelState::new(10, true);
    let cache = Arc::new(RwLock::new(ToolResultCache::new(16)));

    // Insert a cache entry for `/workspace/modified.txt` with read_file
    let key = ToolCacheKey::new("read_file", "{}", "/workspace/modified.txt");
    {
        let mut c = cache.write().await;
        c.insert(key.clone(), "old output".to_string());
        assert!(c.get(&key).is_some());
    }

    // Build an outcome that indicates the file was modified
    let outcome = vtcode_core::tools::result_cache::ToolCacheKey::new;
    let pipeline_outcome = crate::agent::runloop::unified::tool_pipeline::ToolPipelineOutcome::from_status(
        crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus::Success {
            output: json!({"stdout": "ok"}),
            stdout: Some("ok".to_string()),
            modified_files: vec!["/workspace/modified.txt".to_string()],
            command_success: true,
            has_more: false,
        },
    );

    // Call the handler
    let (any_write, mod_files, last_stdout) = handle_pipeline_output_renderer(
        &mut renderer,
        &mut session_stats,
        &mut mcp_state,
        Some(&cache),
        None,
        "read_file",
        &json!({}),
        &pipeline_outcome,
        None,
    )
    .await
    .expect("handler should succeed");

    // Cache should be invalidated
    {
        let mut c = cache.write().await;
        assert!(c.get(&key).is_none());
    }

    // Verify results
    assert_eq!(mod_files, vec![std::path::PathBuf::from("/workspace/modified.txt")]);
    assert_eq!(last_stdout.unwrap(), "ok");
    assert!(!any_write);
    assert!(session_stats.sorted_tools().contains(&"read_file".to_string()));
}
