pub const COPILOT_PROVIDER_KEY: &str = "copilot";
pub const COPILOT_MODEL_ID: &str = vtcode_config::constants::models::copilot::DEFAULT_MODEL;

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
