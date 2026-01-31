//! JSON-RPC 2.0 types for ACP protocol compliance
//!
//! This module implements the JSON-RPC 2.0 specification as required by the
//! Agent Client Protocol (ACP). All ACP methods use JSON-RPC 2.0 as the
//! transport layer.
//!
//! Reference: https://agentclientprotocol.com/llms.txt

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 version string (always "2.0")
pub const JSONRPC_VERSION: &str = "2.0";

/// JSON-RPC 2.0 request object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// Protocol version (always "2.0")
    pub jsonrpc: String,

    /// Method name to invoke
    pub method: String,

    /// Method parameters (positional or named)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,

    /// Request ID for correlation (null for notifications)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<JsonRpcId>,
}

/// JSON-RPC 2.0 response object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// Protocol version (always "2.0")
    pub jsonrpc: String,

    /// Result on success (mutually exclusive with error)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,

    /// Error on failure (mutually exclusive with result)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,

    /// Request ID this response corresponds to
    pub id: Option<JsonRpcId>,
}

/// JSON-RPC 2.0 error object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// Error code (integer)
    pub code: i32,

    /// Short error description
    pub message: String,

    /// Additional error data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// JSON-RPC 2.0 request/response ID
///
/// Per spec, ID can be a string, number, or null
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum JsonRpcId {
    /// String ID
    String(String),
    /// Numeric ID
    Number(i64),
}

impl JsonRpcId {
    /// Create a new string ID
    pub fn string(s: impl Into<String>) -> Self {
        Self::String(s.into())
    }

    /// Create a new numeric ID
    pub fn number(n: i64) -> Self {
        Self::Number(n)
    }

    /// Generate a new UUID-based string ID
    pub fn new_uuid() -> Self {
        Self::String(uuid::Uuid::new_v4().to_string())
    }
}

impl std::fmt::Display for JsonRpcId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JsonRpcId::String(s) => write!(f, "{}", s),
            JsonRpcId::Number(n) => write!(f, "{}", n),
        }
    }
}

/// Standard JSON-RPC 2.0 error codes
pub mod error_codes {
    /// Parse error: Invalid JSON was received
    pub const PARSE_ERROR: i32 = -32700;

    /// Invalid request: The JSON sent is not a valid Request object
    pub const INVALID_REQUEST: i32 = -32600;

    /// Method not found: The method does not exist / is not available
    pub const METHOD_NOT_FOUND: i32 = -32601;

    /// Invalid params: Invalid method parameter(s)
    pub const INVALID_PARAMS: i32 = -32602;

    /// Internal error: Internal JSON-RPC error
    pub const INTERNAL_ERROR: i32 = -32603;

    /// Server error range: -32000 to -32099 (reserved for implementation-defined server-errors)
    pub const SERVER_ERROR_START: i32 = -32099;
    pub const SERVER_ERROR_END: i32 = -32000;

    // ACP-specific error codes (in server error range)

    /// Authentication required
    pub const AUTH_REQUIRED: i32 = -32001;

    /// Permission denied
    pub const PERMISSION_DENIED: i32 = -32002;

    /// Session not found
    pub const SESSION_NOT_FOUND: i32 = -32003;

    /// Rate limited
    pub const RATE_LIMITED: i32 = -32004;

    /// Resource not found
    pub const RESOURCE_NOT_FOUND: i32 = -32005;
}

impl JsonRpcRequest {
    /// Create a new JSON-RPC 2.0 request
    pub fn new(method: impl Into<String>, params: Option<Value>, id: Option<JsonRpcId>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: method.into(),
            params,
            id,
        }
    }

    /// Create a request with auto-generated UUID ID
    pub fn with_uuid(method: impl Into<String>, params: Option<Value>) -> Self {
        Self::new(method, params, Some(JsonRpcId::new_uuid()))
    }

    /// Create a notification (request without ID, no response expected)
    pub fn notification(method: impl Into<String>, params: Option<Value>) -> Self {
        Self::new(method, params, None)
    }

    /// Check if this is a notification (no ID means no response expected)
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

impl JsonRpcResponse {
    /// Create a successful response
    pub fn success(result: Value, id: Option<JsonRpcId>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    /// Create an error response
    pub fn error(error: JsonRpcError, id: Option<JsonRpcId>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: None,
            error: Some(error),
            id,
        }
    }

    /// Check if response is successful
    pub fn is_success(&self) -> bool {
        self.error.is_none() && self.result.is_some()
    }

    /// Check if response is an error
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }

    /// Get result, returning error if response was an error
    pub fn into_result(self) -> Result<Value, JsonRpcError> {
        if let Some(error) = self.error {
            Err(error)
        } else {
            Ok(self.result.unwrap_or(Value::Null))
        }
    }
}

impl JsonRpcError {
    /// Create a new error
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Create error with additional data
    pub fn with_data(code: i32, message: impl Into<String>, data: Value) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data),
        }
    }

    /// Create a parse error
    pub fn parse_error(details: impl Into<String>) -> Self {
        Self::new(error_codes::PARSE_ERROR, details)
    }

    /// Create an invalid request error
    pub fn invalid_request(details: impl Into<String>) -> Self {
        Self::new(error_codes::INVALID_REQUEST, details)
    }

    /// Create a method not found error
    pub fn method_not_found(method: impl Into<String>) -> Self {
        Self::new(
            error_codes::METHOD_NOT_FOUND,
            format!("Method not found: {}", method.into()),
        )
    }

    /// Create an invalid params error
    pub fn invalid_params(details: impl Into<String>) -> Self {
        Self::new(error_codes::INVALID_PARAMS, details)
    }

    /// Create an internal error
    pub fn internal_error(details: impl Into<String>) -> Self {
        Self::new(error_codes::INTERNAL_ERROR, details)
    }

    /// Create an authentication required error with list of available methods
    /// 
    /// Per ACP spec, includes authMethods in error data to help clients
    /// present appropriate UI for authentication options.
    pub fn auth_required(auth_methods: Vec<super::AuthMethod>) -> Self {
        let data = serde_json::json!({
            "authMethods": auth_methods,
        });
        Self::with_data(
            error_codes::AUTH_REQUIRED,
            "Authentication required",
            data,
        )
    }

    /// Create a permission denied error
    pub fn permission_denied(details: impl Into<String>) -> Self {
        Self::new(error_codes::PERMISSION_DENIED, details)
    }

    /// Create a session not found error
    pub fn session_not_found(session_id: impl Into<String>) -> Self {
        Self::new(
            error_codes::SESSION_NOT_FOUND,
            format!("Session not found: {}", session_id.into()),
        )
    }

    /// Create a rate limited error
    pub fn rate_limited(details: impl Into<String>) -> Self {
        Self::new(error_codes::RATE_LIMITED, details)
    }

    /// Create a resource not found error
    pub fn resource_not_found(resource: impl Into<String>) -> Self {
        Self::new(
            error_codes::RESOURCE_NOT_FOUND,
            format!("Resource not found: {}", resource.into()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_request_serialization() {
        let req = JsonRpcRequest::new(
            "initialize",
            Some(json!({"protocolVersions": ["2025-01-01"]})),
            Some(JsonRpcId::string("req-1")),
        );

        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"method\":\"initialize\""));
        assert!(json.contains("\"id\":\"req-1\""));
    }

    #[test]
    fn test_response_success() {
        let resp = JsonRpcResponse::success(
            json!({"session_id": "sess-123"}),
            Some(JsonRpcId::string("req-1")),
        );

        assert!(resp.is_success());
        assert!(!resp.is_error());
    }

    #[test]
    fn test_response_error() {
        let resp = JsonRpcResponse::error(
            JsonRpcError::method_not_found("unknown"),
            Some(JsonRpcId::string("req-1")),
        );

        assert!(resp.is_error());
        assert!(!resp.is_success());
    }

    #[test]
    fn test_notification() {
        let notif = JsonRpcRequest::notification("session/update", Some(json!({"delta": "hello"})));

        assert!(notif.is_notification());
        assert!(notif.id.is_none());
    }

    #[test]
    fn test_id_types() {
        let string_id = JsonRpcId::string("abc");
        let number_id = JsonRpcId::number(123);

        assert_eq!(format!("{}", string_id), "abc");
        assert_eq!(format!("{}", number_id), "123");
    }

    #[test]
    fn test_auth_required_error() {
        use super::super::AuthMethod;

        let auth_methods = vec![
            AuthMethod::Agent {
                id: "agent_auth".to_string(),
                name: "Agent Auth".to_string(),
                description: None,
            },
            AuthMethod::EnvVar {
                id: "openai_key".to_string(),
                name: "OpenAI Key".to_string(),
                description: None,
                var_name: "OPENAI_API_KEY".to_string(),
                link: None,
            },
        ];

        let error = JsonRpcError::auth_required(auth_methods);
        
        assert_eq!(error.code, error_codes::AUTH_REQUIRED);
        assert_eq!(error.message, "Authentication required");
        assert!(error.data.is_some());

        let data = error.data.unwrap();
        assert!(data["authMethods"].is_array());
        assert_eq!(data["authMethods"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_auth_required_error_serialization() {
        use super::super::AuthMethod;

        let auth_methods = vec![
            AuthMethod::EnvVar {
                id: "test".to_string(),
                name: "Test".to_string(),
                description: None,
                var_name: "TEST_VAR".to_string(),
                link: Some("https://example.com".to_string()),
            },
        ];

        let error = JsonRpcError::auth_required(auth_methods);
        let json = serde_json::to_value(&error).unwrap();

        assert_eq!(json["code"], -32001);
        assert_eq!(json["message"], "Authentication required");
        assert_eq!(json["data"]["authMethods"][0]["type"], "env_var");
        assert_eq!(json["data"]["authMethods"][0]["id"], "test");
    }

    #[test]
    fn test_acp_error_helpers() {
        let err_perm = JsonRpcError::permission_denied("Not allowed");
        assert_eq!(err_perm.code, error_codes::PERMISSION_DENIED);

        let err_session = JsonRpcError::session_not_found("sess-123");
        assert_eq!(err_session.code, error_codes::SESSION_NOT_FOUND);
        assert!(err_session.message.contains("sess-123"));

        let err_rate = JsonRpcError::rate_limited("Too many requests");
        assert_eq!(err_rate.code, error_codes::RATE_LIMITED);

        let err_resource = JsonRpcError::resource_not_found("file.txt");
        assert_eq!(err_resource.code, error_codes::RESOURCE_NOT_FOUND);
        assert!(err_resource.message.contains("file.txt"));
    }
}
