mod acp_client;
mod auth;
mod command;
mod server_client;
mod types;

pub use acp_client::{
    CopilotAcpClient, CopilotRuntimeRequest, PendingPermissionRequest, PendingToolCallRequest,
    PromptCompletion, PromptSession, PromptSessionCancelHandle, PromptUpdate,
};
pub use auth::{login, login_with_events, logout, logout_with_events, probe_auth_status};
pub use server_client::list_available_models;
pub use types::{
    COPILOT_AUTH_DOC_PATH, COPILOT_MODEL_ID, COPILOT_PROVIDER_KEY, CopilotAcpCompatibilityState,
    CopilotAuthEvent, CopilotAuthStatus, CopilotAuthStatusKind, CopilotDiscoveredModel,
    CopilotObservedToolCall, CopilotObservedToolCallStatus, CopilotPermissionDecision,
    CopilotPermissionRequest, CopilotPromptSessionFuture, CopilotShellCommandSummary,
    CopilotToolCallFailure, CopilotToolCallRequest, CopilotToolCallResponse,
    CopilotToolCallSuccess, CopilotToolUse,
};
