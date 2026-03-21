use std::future::Future;
use std::pin::Pin;

use serde_json::Value;

use crate::llm::provider::LLMError;

pub const COPILOT_PROVIDER_KEY: &str = "copilot";
pub const COPILOT_MODEL_ID: &str = vtcode_config::constants::models::copilot::DEFAULT_MODEL;
pub const COPILOT_AUTH_DOC_PATH: &str = "docs/providers/copilot.md";

pub type CopilotPromptSessionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<crate::copilot::PromptSession, LLMError>> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopilotAcpCompatibilityState {
    FullTools,
    PromptOnly,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopilotAuthEvent {
    VerificationCode { url: String, user_code: String },
    Progress { message: String },
    Success { account: Option<String> },
    Failure { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopilotDiscoveredModel {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopilotAuthStatusKind {
    Authenticated,
    Unauthenticated,
    ServerUnavailable,
    AuthFlowFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopilotAuthStatus {
    pub kind: CopilotAuthStatusKind,
    pub message: Option<String>,
}

impl CopilotAuthStatus {
    #[must_use]
    pub fn authenticated(message: Option<String>) -> Self {
        Self {
            kind: CopilotAuthStatusKind::Authenticated,
            message,
        }
    }

    #[must_use]
    pub fn unauthenticated(message: Option<String>) -> Self {
        Self {
            kind: CopilotAuthStatusKind::Unauthenticated,
            message,
        }
    }

    #[must_use]
    pub fn server_unavailable(message: impl Into<String>) -> Self {
        Self {
            kind: CopilotAuthStatusKind::ServerUnavailable,
            message: Some(message.into()),
        }
    }

    #[must_use]
    pub fn auth_flow_failed(message: impl Into<String>) -> Self {
        Self {
            kind: CopilotAuthStatusKind::AuthFlowFailed,
            message: Some(message.into()),
        }
    }

    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        matches!(self.kind, CopilotAuthStatusKind::Authenticated)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CopilotToolCallRequest {
    pub tool_call_id: String,
    pub tool_name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopilotToolUse {
    pub tool_call_id: String,
    pub tool_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopilotObservedToolCallStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CopilotObservedToolCall {
    pub tool_call_id: String,
    pub tool_name: String,
    pub status: CopilotObservedToolCallStatus,
    pub arguments: Option<Value>,
    pub output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopilotToolCallSuccess {
    pub text_result_for_llm: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopilotToolCallFailure {
    pub text_result_for_llm: String,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopilotToolCallResponse {
    Success(CopilotToolCallSuccess),
    Failure(CopilotToolCallFailure),
}

#[derive(Debug, Clone, PartialEq)]
pub enum CopilotPermissionRequest {
    Shell {
        tool_call_id: Option<String>,
        full_command_text: String,
        intention: String,
        commands: Vec<CopilotShellCommandSummary>,
        possible_paths: Vec<String>,
        possible_urls: Vec<String>,
        has_write_file_redirection: bool,
        can_offer_session_approval: bool,
        warning: Option<String>,
    },
    Write {
        tool_call_id: Option<String>,
        intention: String,
        file_name: String,
        diff: String,
        new_file_contents: Option<String>,
    },
    Read {
        tool_call_id: Option<String>,
        intention: String,
        path: String,
    },
    Mcp {
        tool_call_id: Option<String>,
        server_name: String,
        tool_name: String,
        tool_title: String,
        args: Option<Value>,
        read_only: bool,
    },
    Url {
        tool_call_id: Option<String>,
        intention: String,
        url: String,
    },
    Memory {
        tool_call_id: Option<String>,
        subject: String,
        fact: String,
        citations: String,
    },
    CustomTool {
        tool_call_id: Option<String>,
        tool_name: String,
        tool_description: String,
        args: Option<Value>,
    },
    Hook {
        tool_call_id: Option<String>,
        tool_name: String,
        tool_args: Option<Value>,
        hook_message: Option<String>,
    },
    Unknown {
        kind: Option<String>,
        raw: Value,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopilotShellCommandSummary {
    pub identifier: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopilotPermissionDecision {
    Approved,
    ApprovedAlways,
    DeniedByRules,
    DeniedNoApprovalRule,
    DeniedInteractivelyByUser { feedback: Option<String> },
    DeniedByContentExclusionPolicy { path: String, message: String },
}

impl CopilotPermissionDecision {
    #[must_use]
    pub fn to_rpc_result(&self) -> Value {
        match self {
            Self::Approved | Self::ApprovedAlways => serde_json::json!({
                "kind": "approved",
            }),
            Self::DeniedByRules => serde_json::json!({
                "kind": "denied-by-rules",
                "rules": [],
            }),
            Self::DeniedNoApprovalRule => serde_json::json!({
                "kind": "denied-no-approval-rule-and-could-not-request-from-user",
            }),
            Self::DeniedInteractivelyByUser { feedback } => {
                let mut result = serde_json::Map::from_iter([(
                    "kind".to_string(),
                    Value::String("denied-interactively-by-user".to_string()),
                )]);
                if let Some(feedback) = feedback.as_ref().filter(|value| !value.trim().is_empty()) {
                    result.insert("feedback".to_string(), Value::String(feedback.clone()));
                }
                Value::Object(result)
            }
            Self::DeniedByContentExclusionPolicy { path, message } => {
                serde_json::json!({
                    "kind": "denied-by-content-exclusion-policy",
                    "path": path,
                    "message": message,
                })
            }
        }
    }
}
