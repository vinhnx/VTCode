use serde_json::json;

use super::ToolRegistry;

/// `mcp_executor` must dispatch `action='connect'`/`'disconnect'` to
/// `mcp_connect_server_executor`/`mcp_disconnect_server_executor` rather
/// than falling through to the unknown-action branch. A bare
/// `ToolRegistry::new` has no MCP client configured, so both calls fail
/// at the `mcp_client()` lookup inside the delegated executor -- but the
/// error text proves the dispatch reached the right function.
#[tokio::test]
async fn mcp_executor_dispatches_connect_and_disconnect_actions() {
    let temp = tempfile::tempdir().expect("tempdir");
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;

    let connect_err = registry
        .mcp_executor(json!({"action": "connect", "name": "example"}))
        .await
        .expect_err("connect without an active mcp client should fail");
    let connect_text = connect_err.to_string();
    assert!(!connect_text.contains("unknown action"));
    assert!(connect_text.contains("MCP client not available"));

    let disconnect_err = registry
        .mcp_executor(json!({"action": "disconnect", "name": "example"}))
        .await
        .expect_err("disconnect without an active mcp client should fail");
    let disconnect_text = disconnect_err.to_string();
    assert!(!disconnect_text.contains("unknown action"));
    assert!(disconnect_text.contains("MCP client not available"));
}

#[tokio::test]
async fn mcp_executor_rejects_unknown_action_with_guidance() {
    let temp = tempfile::tempdir().expect("tempdir");
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;

    let err = registry
        .mcp_executor(json!({"action": "bogus"}))
        .await
        .expect_err("unknown mcp action should error");
    let text = err.to_string();
    assert!(text.contains("unknown action 'bogus'"));
    assert!(text.contains("connect"));
    assert!(text.contains("disconnect"));
}
