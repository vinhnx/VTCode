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

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum SessionStartTrigger {
    Startup,
    Resume,
    Clear,
    Compact,
}

impl SessionStartTrigger {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Resume => "resume",
            Self::Clear => "clear",
            Self::Compact => "compact",
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum SessionEndReason {
    Completed,
    Exit,
    Cancelled,
    Error,
    NewSession,
    Other,
}

impl SessionEndReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Exit => "exit",
            Self::Cancelled => "cancelled",
            Self::Error => "error",
            Self::NewSession => "new_session",
            Self::Other => "other",
        }
    }
}
