#[derive(Debug, Clone)]
pub struct HookMessage {
    pub level: HookMessageLevel,
    pub text: String,
}

impl HookMessage {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            level: HookMessageLevel::Info,
            text: text.into(),
        }
    }

    pub fn warning(text: impl Into<String>) -> Self {
        Self {
            level: HookMessageLevel::Warning,
            text: text.into(),
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            level: HookMessageLevel::Error,
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum HookMessageLevel {
    Info,
    Warning,
    Error,
}

#[derive(Default)]
pub struct SessionStartHookOutcome {
    pub messages: Vec<HookMessage>,
    pub additional_context: Vec<String>,
}

pub struct UserPromptHookOutcome {
    pub allow_prompt: bool,
    pub block_reason: Option<String>,
    pub additional_context: Vec<String>,
    pub messages: Vec<HookMessage>,
}

impl Default for UserPromptHookOutcome {
    fn default() -> Self {
        Self {
            allow_prompt: true,
            block_reason: None,
            additional_context: Vec::new(),
            messages: Vec::new(),
        }
    }
}

#[derive(Default)]
pub struct PreToolHookOutcome {
    pub decision: PreToolHookDecision,
    pub messages: Vec<HookMessage>,
}

#[derive(Default)]
pub struct PostToolHookOutcome {
    pub messages: Vec<HookMessage>,
    pub additional_context: Vec<String>,
    pub block_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum PreToolHookDecision {
    #[default]
    Continue,
    Allow,
    Deny,
    Ask,
}

#[derive(Debug, Clone, Copy)]
pub enum SessionStartTrigger {
    Startup,
    Resume,
}

impl SessionStartTrigger {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Resume => "resume",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SessionEndReason {
    Completed,
    Exit,
    Cancelled,
    Error,
    NewSession,
}

impl SessionEndReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Exit => "exit",
            Self::Cancelled => "cancelled",
            Self::Error => "error",
            Self::NewSession => "new_session",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationHookType {
    PermissionPrompt,
    IdlePrompt,
}

impl NotificationHookType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PermissionPrompt => "permission_prompt",
            Self::IdlePrompt => "idle_prompt",
        }
    }
}
