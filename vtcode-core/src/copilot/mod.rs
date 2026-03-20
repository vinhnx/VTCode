mod acp_client;
mod auth;
mod command;
mod types;

pub use acp_client::{CopilotAcpClient, PromptCompletion, PromptSession, PromptUpdate};
pub use auth::{login, logout, probe_auth_status};
pub use types::{COPILOT_MODEL_ID, COPILOT_PROVIDER_KEY, CopilotAuthStatus, CopilotAuthStatusKind};
