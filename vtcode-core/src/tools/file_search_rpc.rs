//! RPC endpoint for file search operations
//!
//! Provides JSON-RPC interface to file_search_bridge for remote clients (VS Code extension).
//! Handles request/response serialization and validation.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;

use super::file_search_bridge::{self, FileSearchConfig};

/// JSON-RPC request for searching files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchFilesRequest {
    /// Fuzzy search pattern (e.g., "main", "test.rs")
    pub pattern: String,
    /// Root directory to search
    pub workspace_root: PathBuf,
    /// Maximum number of results to return
    pub max_results: usize,
    /// Patterns to exclude from results (glob-style)
    pub exclude_patterns: Vec<String>,
    /// Whether to respect .gitignore files
    pub respect_gitignore: bool,
}

/// JSON-RPC request for listing files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListFilesRequest {
    /// Root directory to list files from
    pub workspace_root: PathBuf,
    /// Patterns to exclude from results
    pub exclude_patterns: Vec<String>,
    /// Whether to respect .gitignore files
    pub respect_gitignore: bool,
    /// Maximum number of files to return
    pub max_results: usize,
}

/// File match in RPC response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMatchRpc {
    /// Path relative to workspace root
    pub path: String,
    /// Fuzzy match score (higher = better match)
    pub score: u32,
    /// Character indices for match highlighting (optional)
    pub indices: Option<Vec<u32>>,
}

/// JSON-RPC response for search_files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchFilesResponse {
    /// Matched files
    pub matches: Vec<FileMatchRpc>,
    /// Total number of matches found
    pub total_match_count: usize,
    /// Whether result was truncated
    pub truncated: bool,
}

/// JSON-RPC response for list_files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListFilesResponse {
    /// All discovered files
    pub files: Vec<String>,
    /// Total files found
    pub total: usize,
}

/// Error response for RPC calls
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    /// Error code
    pub code: i32,
    /// Error message
    pub message: String,
    /// Additional error data
    pub data: Option<Value>,
}

impl RpcError {
    /// Create a new RPC error
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Invalid request error (-32600)
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(-32600, message)
    }

    /// Method not found error (-32601)
    pub fn method_not_found() -> Self {
        Self::new(-32601, "Method not found")
    }

    /// Invalid params error (-32602)
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(-32602, message)
    }

    /// Internal error (-32603)
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new(-32603, message)
    }

    /// Custom error code
    pub fn custom(code: i32, message: impl Into<String>) -> Self {
        Self::new(code, message)
    }
}

/// RPC request envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    /// JSON-RPC version (always "2.0")
    pub jsonrpc: String,
    /// RPC method name
    pub method: String,
    /// RPC method parameters (varies by method)
    pub params: Value,
    /// Request ID (for responses)
    pub id: Option<Value>,
}

/// RPC response envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    /// JSON-RPC version
    pub jsonrpc: String,
    /// Response ID (matches request ID)
    pub id: Option<Value>,
    /// Success result (if successful)
    pub result: Option<Value>,
    /// Error response (if failed)
    pub error: Option<RpcError>,
}

impl RpcResponse {
    /// Create a successful response
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Create an error response
    pub fn error(id: Option<Value>, error: RpcError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// Handler for file search RPC requests
pub struct FileSearchRpcHandler;

impl FileSearchRpcHandler {
    /// Handle an incoming RPC request
    ///
    /// # Arguments
    ///
    /// * `request` - Parsed JSON-RPC request
    ///
    /// # Returns
    ///
    /// JSON-RPC response with result or error
    pub async fn handle_request(request: RpcRequest) -> RpcResponse {
        let id = request.id.clone();

        // Validate JSON-RPC version
        if request.jsonrpc != "2.0" {
            return RpcResponse::error(id, RpcError::invalid_request("Invalid JSON-RPC version"));
        }

        // Dispatch to appropriate handler
        let result = match request.method.as_str() {
            "search_files" => Self::handle_search_files(&request.params, id.clone()).await,
            "list_files" => Self::handle_list_files(&request.params).await,
            "find_references" => Self::handle_find_references(&request.params).await,
            _ => return RpcResponse::error(id, RpcError::method_not_found()),
        };

        match result {
            Ok(response) => RpcResponse::success(id, response),
            Err(error) => RpcResponse::error(id, RpcError::internal_error(error.to_string())),
        }
    }

    /// Handle search_files RPC method
    ///
    /// Performs fuzzy file search with the given pattern.
    async fn handle_search_files(params: &Value, _id: Option<Value>) -> Result<Value> {
        let request: SearchFilesRequest = serde_json::from_value(params.clone())
            .context("Failed to parse search_files parameters")?;

        // Validate workspace root
        if !request.workspace_root.exists() {
            return Err(anyhow::anyhow!(
                "Workspace root does not exist: {}",
                request.workspace_root.display()
            ));
        }

        // Build configuration
        let config = FileSearchConfig::new(request.pattern, request.workspace_root)
            .with_limit(request.max_results)
            .respect_gitignore(request.respect_gitignore);

        // Perform search
        let results = file_search_bridge::search_files(config, None)?;

        // Convert to RPC response format
        let matches: Vec<FileMatchRpc> = results
            .matches
            .into_iter()
            .map(|m| FileMatchRpc {
                path: m.path,
                score: m.score,
                indices: m.indices,
            })
            .collect();

        Ok(json!({
            "matches": matches,
            "total_match_count": results.total_match_count,
            "truncated": matches.len() >= request.max_results,
        }))
    }

    /// Handle list_files RPC method
    ///
    /// Lists all files in the workspace with optional exclusions.
    async fn handle_list_files(params: &Value) -> Result<Value> {
        let request: ListFilesRequest = serde_json::from_value(params.clone())
            .context("Failed to parse list_files parameters")?;

        // Validate workspace root
        if !request.workspace_root.exists() {
            return Err(anyhow::anyhow!(
                "Workspace root does not exist: {}",
                request.workspace_root.display()
            ));
        }

        // Build configuration (empty pattern lists all files)
        let mut config = FileSearchConfig::new(String::new(), request.workspace_root)
            .with_limit(request.max_results)
            .respect_gitignore(request.respect_gitignore);

        for pattern in request.exclude_patterns {
            config = config.exclude(pattern);
        }

        // Perform search
        let results = file_search_bridge::search_files(config, None)?;

        // Extract file paths
        let files: Vec<String> = results.matches.into_iter().map(|m| m.path).collect();
        let total = files.len();

        Ok(json!({
            "files": files,
            "total": total,
        }))
    }

    /// Handle find_references RPC method (stub for future implementation)
    ///
    /// Finds all files containing a symbol reference.
    async fn handle_find_references(params: &Value) -> Result<Value> {
        // This would require more sophisticated symbol analysis
        // For now, return a placeholder that could be implemented later
        let _symbol: String = serde_json::from_value(params.clone())
            .context("Failed to parse find_references parameters")?;

        Ok(json!({
            "matches": [],
            "message": "find_references not yet implemented",
        }))
    }
}

/// Parse JSON-RPC request from raw JSON string
///
/// # Arguments
///
/// * `json_string` - Raw JSON request string
///
/// # Returns
///
/// Parsed RPC request or error response
pub fn parse_rpc_request(json_string: &str) -> Result<RpcRequest, RpcResponse> {
    match serde_json::from_str::<RpcRequest>(json_string) {
        Ok(request) => Ok(request),
        Err(err) => {
            let error_response =
                RpcResponse::error(None, RpcError::invalid_request(err.to_string()));
            Err(error_response)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpc_error_codes() {
        assert_eq!(RpcError::invalid_request("test").code, -32600);
        assert_eq!(RpcError::method_not_found().code, -32601);
        assert_eq!(RpcError::invalid_params("test").code, -32602);
        assert_eq!(RpcError::internal_error("test").code, -32603);
    }

    #[test]
    fn test_rpc_response_success() {
        let response = RpcResponse::success(Some(json!(1)), json!({"ok": true}));
        assert_eq!(response.jsonrpc, "2.0");
        assert_eq!(response.id, Some(json!(1)));
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[test]
    fn test_rpc_response_error() {
        let error = RpcError::internal_error("test error");
        let response = RpcResponse::error(Some(json!(1)), error);
        assert_eq!(response.jsonrpc, "2.0");
        assert_eq!(response.id, Some(json!(1)));
        assert!(response.result.is_none());
        assert!(response.error.is_some());
    }

    #[test]
    fn test_search_files_request_parsing() {
        let json = r#"{
            "pattern": "main",
            "workspace_root": "/workspace",
            "max_results": 100,
            "exclude_patterns": [],
            "respect_gitignore": true
        }"#;

        let value: Value = serde_json::from_str(json).unwrap();
        let request: SearchFilesRequest = serde_json::from_value(value).unwrap();

        assert_eq!(request.pattern, "main");
        assert_eq!(request.max_results, 100);
        assert!(request.respect_gitignore);
    }

    #[test]
    fn test_list_files_request_parsing() {
        let json = r#"{
            "workspace_root": "/workspace",
            "exclude_patterns": ["**/node_modules/**"],
            "respect_gitignore": true,
            "max_results": 1000
        }"#;

        let value: Value = serde_json::from_str(json).unwrap();
        let request: ListFilesRequest = serde_json::from_value(value).unwrap();

        assert_eq!(request.exclude_patterns.len(), 1);
        assert_eq!(request.max_results, 1000);
    }

    #[test]
    fn test_parse_invalid_rpc_request() {
        let invalid_json = "not valid json";
        let result = parse_rpc_request(invalid_json);
        assert!(result.is_err());
    }

    #[test]
    fn test_file_match_rpc_serialization() {
        let file_match = FileMatchRpc {
            path: "src/main.rs".to_string(),
            score: 100,
            indices: Some(vec![4, 5]),
        };

        let json = serde_json::to_string(&file_match).unwrap();
        let deserialized: FileMatchRpc = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.path, "src/main.rs");
        assert_eq!(deserialized.score, 100u32);
        assert_eq!(deserialized.indices, Some(vec![4u32, 5u32]));
    }
}
