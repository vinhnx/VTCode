use serde_json::Value;

use crate::config::PermissionMode;
use crate::exec::events::ThreadCompletionSubtype;

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

#[derive(Default)]
pub struct PreCompactHookOutcome {
    pub messages: Vec<HookMessage>,
}

#[derive(Default)]
pub struct PermissionRequestHookOutcome {
    pub decision: Option<PermissionRequestHookDecision>,
    pub messages: Vec<HookMessage>,
}

#[derive(Debug, Clone)]
pub struct PermissionRequestHookDecision {
    pub behavior: PermissionDecisionBehavior,
    pub scope: PermissionDecisionScope,
    pub updated_input: Option<Value>,
    pub permission_updates: Vec<PermissionUpdateRequest>,
    pub interrupt: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecisionBehavior {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecisionScope {
    Once,
    Session,
    Permanent,
}

#[derive(Debug, Clone)]
pub struct PermissionUpdateRequest {
    pub destination: PermissionUpdateDestination,
    pub kind: PermissionUpdateKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionUpdateDestination {
    Session,
    ProjectSettings,
    Unsupported(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionUpdateKind {
    AddRules(Vec<String>),
    ReplaceRules(Vec<String>),
    RemoveRules(Vec<String>),
    SetMode(PermissionMode),
    Unsupported(String),
}

#[derive(Default)]
pub struct StopHookOutcome {
    pub messages: Vec<HookMessage>,
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
    NewSession,
    Compact,
}

impl SessionStartTrigger {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Resume => "resume",
            Self::NewSession => "new_session",
            Self::Compact => "compact",
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

    pub fn thread_completion_status(
        self,
        budget_limit_reached: bool,
    ) -> (&'static str, ThreadCompletionSubtype) {
        if budget_limit_reached {
            return (
                "budget_limit_reached",
                ThreadCompletionSubtype::ErrorMaxBudgetUsd,
            );
        }

        match self {
            Self::Completed => ("completed", ThreadCompletionSubtype::Success),
            Self::Exit => ("exit", ThreadCompletionSubtype::Cancelled),
            Self::Cancelled => ("cancelled", ThreadCompletionSubtype::Cancelled),
            Self::Error => ("error", ThreadCompletionSubtype::ErrorDuringExecution),
            Self::NewSession => ("new_session", ThreadCompletionSubtype::Success),
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
