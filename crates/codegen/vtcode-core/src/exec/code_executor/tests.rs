use super::*;
use crate::mcp::{McpClientStatus, McpToolExecutor, McpToolInfo, McpToolVisibility};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;

struct MockMcpToolExecutor;

#[async_trait]
impl McpToolExecutor for MockMcpToolExecutor {
    async fn execute_mcp_tool(&self, _tool_name: &str, _args: &Value) -> Result<Value> {
        Ok(json!({}))
    }

    async fn list_mcp_tools(&self) -> Result<Vec<McpToolInfo>> {
        Ok(vec![McpToolInfo {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            provider: "test".to_string(),
            input_schema: json!({}),
            output_schema: None,
            ui: None,
            visibility: McpToolVisibility::default(),
        }])
    }

    async fn has_mcp_tool(&self, _tool_name: &str) -> Result<bool> {
        Ok(true)
    }

    fn get_status(&self) -> McpClientStatus {
        McpClientStatus {
            enabled: true,
            provider_count: 1,
            active_connections: 1,
            configured_providers: vec!["test".to_string()],
        }
    }
}

fn test_executor(language: Language) -> CodeExecutor {
    CodeExecutor::new(language, Arc::new(MockMcpToolExecutor), PathBuf::from("/workspace"))
}

struct MockBuiltinExecutor;

#[async_trait]
impl BuiltinToolExecutor for MockBuiltinExecutor {
    async fn execute_builtin_tool(&self, _name: &str, _args: &Value) -> Result<Value> {
        Ok(json!({}))
    }

    fn list_builtin_tools(&self) -> Result<Vec<BuiltinToolInfo>> {
        Ok(vec![BuiltinToolInfo {
            name: "unified_file".to_string(),
            description: "Read/write files".to_string(),
        }])
    }
}

#[tokio::test]
async fn generate_sdk_includes_builtin_wrappers() {
    let executor = test_executor(Language::Python3).with_builtin_executor(Some(Arc::new(MockBuiltinExecutor)));
    let sdk = executor.generate_python_sdk().await.unwrap();
    assert!(sdk.contains("def unified_file("));
    assert!(sdk.contains("mcp._call_tool('unified_file'"));
    // MCP tools remain present alongside built-ins.
    assert!(sdk.contains("def read_file("));
}

#[test]
fn sanitize_function_name_handles_special_chars() {
    assert_eq!(sanitize_function_name("read_file"), "read_file");
    assert_eq!(sanitize_function_name("read-file"), "read_file");
    assert_eq!(sanitize_function_name("read.file"), "read_file");
    assert_eq!(sanitize_function_name("readFile123"), "readFile123");
}

#[test]
fn language_as_str() {
    assert_eq!(Language::Python3.as_str(), "python3");
    assert_eq!(Language::JavaScript.as_str(), "javascript");
}

#[test]
fn language_interpreter() {
    assert_eq!(Language::Python3.interpreter(), "python3");
    assert_eq!(Language::JavaScript.interpreter(), "node");
}

#[tokio::test]
async fn python_sdk_uses_ipc_dir_only() {
    let sdk = test_executor(Language::Python3)
        .generate_sdk()
        .await
        .expect("python sdk should generate");

    assert!(sdk.contains("VTCODE_IPC_DIR"));
    assert!(!sdk.contains("/tmp/vtcode_ipc"));
    assert!(!sdk.contains("VTCODE_WORKSPACE"));
}

#[tokio::test]
async fn javascript_sdk_uses_ipc_dir_only() {
    let sdk = test_executor(Language::JavaScript)
        .generate_sdk()
        .await
        .expect("javascript sdk should generate");

    assert!(sdk.contains("VTCODE_IPC_DIR"));
    assert!(!sdk.contains("/tmp/vtcode_ipc"));
    assert!(!sdk.contains("VTCODE_WORKSPACE"));
}
