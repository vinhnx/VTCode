use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedExecAction {
    Run,
    Write,
    Poll,
    Continue,
    Inspect,
    List,
    Close,
    Code,
}

impl UnifiedExecAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Write => "write",
            Self::Poll => "poll",
            Self::Continue => "continue",
            Self::Inspect => "inspect",
            Self::List => "list",
            Self::Close => "close",
            Self::Code => "code",
        }
    }

    pub const fn documented_labels() -> &'static [&'static str] {
        const ACTIONS: &[&str] = &[
            UnifiedExecAction::Run.as_str(),
            UnifiedExecAction::Write.as_str(),
            UnifiedExecAction::Poll.as_str(),
            UnifiedExecAction::Continue.as_str(),
            UnifiedExecAction::Inspect.as_str(),
            UnifiedExecAction::List.as_str(),
            UnifiedExecAction::Close.as_str(),
            UnifiedExecAction::Code.as_str(),
        ];
        ACTIONS
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedFileAction {
    Read,
    Write,
    Edit,
    Patch,
    Delete,
    Move,
    Copy,
}

impl UnifiedFileAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Edit => "edit",
            Self::Patch => "patch",
            Self::Delete => "delete",
            Self::Move => "move",
            Self::Copy => "copy",
        }
    }

    pub const fn documented_labels() -> &'static [&'static str] {
        const ACTIONS: &[&str] = &[
            UnifiedFileAction::Read.as_str(),
            UnifiedFileAction::Write.as_str(),
            UnifiedFileAction::Edit.as_str(),
            UnifiedFileAction::Patch.as_str(),
            UnifiedFileAction::Delete.as_str(),
            UnifiedFileAction::Move.as_str(),
            UnifiedFileAction::Copy.as_str(),
        ];
        ACTIONS
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedSearchAction {
    Grep,
    List,
    Structural,
    Intelligence,
    Tools,
    Errors,
    Agent,
    Web,
    Skill,
}

impl UnifiedSearchAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Grep => "grep",
            Self::List => "list",
            Self::Structural => "structural",
            Self::Intelligence => "intelligence",
            Self::Tools => "tools",
            Self::Errors => "errors",
            Self::Agent => "agent",
            Self::Web => "web",
            Self::Skill => "skill",
        }
    }

    pub const fn documented_labels() -> &'static [&'static str] {
        const ACTIONS: &[&str] = &[
            UnifiedSearchAction::Grep.as_str(),
            UnifiedSearchAction::List.as_str(),
            UnifiedSearchAction::Structural.as_str(),
            UnifiedSearchAction::Intelligence.as_str(),
            UnifiedSearchAction::Tools.as_str(),
            UnifiedSearchAction::Errors.as_str(),
            UnifiedSearchAction::Agent.as_str(),
            UnifiedSearchAction::Web.as_str(),
            UnifiedSearchAction::Skill.as_str(),
        ];
        ACTIONS
    }
}
