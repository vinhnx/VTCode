use std::fmt;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LocalAgentKind {
    Delegated,
    Background,
    ExecSession,
}

impl LocalAgentKind {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Delegated => "delegated",
            Self::Background => "background",
            Self::ExecSession => "exec-session",
        }
    }
}

/// Live/terminal status of a local-agent row, decoded once at the boundary.
///
/// Each producer (`SubagentStatus`, `BackgroundSubprocessStatus`, exec
/// lifecycle state) maps to this union in the runloop's local-agents
/// builder, so render code matches on variants instead of string literals.
/// Liveness is inherent to the variant; no kind dispatch needed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalAgentStatus {
    Queued,
    Starting,
    Running,
    Waiting,
    Completed,
    Failed,
    Closed,
    Stopped,
    Error,
    Exited { code: Option<i32> },
    Unknown,
}

impl LocalAgentStatus {
    /// Whether this status represents live background work.
    #[must_use]
    pub const fn is_live(self) -> bool {
        matches!(self, Self::Queued | Self::Starting | Self::Running | Self::Waiting)
    }
}

impl fmt::Display for LocalAgentStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Queued => write!(f, "queued"),
            Self::Starting => write!(f, "starting"),
            Self::Running => write!(f, "running"),
            Self::Waiting => write!(f, "waiting"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::Closed => write!(f, "closed"),
            Self::Stopped => write!(f, "stopped"),
            Self::Error => write!(f, "error"),
            Self::Exited { code: Some(code) } => write!(f, "exited ({code})"),
            Self::Exited { code: None } => write!(f, "exited"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Actions that the runloop performs for a raw background exec session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecSessionAction {
    Inspect,
    GracefulTerminate,
    ForceTerminateOrClose,
    Focus,
    Preview,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalAgentEntry {
    pub id: String,
    pub display_label: String,
    pub agent_name: String,
    pub color: Option<String>,
    pub kind: LocalAgentKind,
    /// Typed terminal projection, derived from runtime state and exit evidence.
    pub program_status: vtcode_commons::program_status::ProgramState,
    /// Unix milliseconds used to prioritize recently updated retained records.
    pub updated_at: i64,
    pub status: LocalAgentStatus,
    pub summary: Option<String>,
    pub preview: String,
    pub transcript_path: Option<PathBuf>,
}

impl LocalAgentEntry {
    #[must_use]
    /// Whether this entry represents live background work.
    pub fn is_loading(&self) -> bool {
        self.status.is_live()
    }

    /// Non-live history row used for expanded-window header counts.
    /// Live work is [`Self::is_loading`]; everything retained that is not live
    /// counts as finished for the `N running · M finished` summary.
    #[must_use]
    pub(crate) fn is_finished(&self) -> bool {
        !self.is_loading()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_status_has_expected_liveness_and_label() {
        let cases = [
            (LocalAgentStatus::Queued, true, "queued"),
            (LocalAgentStatus::Starting, true, "starting"),
            (LocalAgentStatus::Running, true, "running"),
            (LocalAgentStatus::Waiting, true, "waiting"),
            (LocalAgentStatus::Completed, false, "completed"),
            (LocalAgentStatus::Failed, false, "failed"),
            (LocalAgentStatus::Closed, false, "closed"),
            (LocalAgentStatus::Stopped, false, "stopped"),
            (LocalAgentStatus::Error, false, "error"),
            (LocalAgentStatus::Exited { code: None }, false, "exited"),
            (LocalAgentStatus::Exited { code: Some(0) }, false, "exited (0)"),
            (LocalAgentStatus::Exited { code: Some(1) }, false, "exited (1)"),
            (LocalAgentStatus::Unknown, false, "unknown"),
        ];
        for (status, live, label) in cases {
            assert_eq!(status.is_live(), live, "{status:?} liveness");
            assert_eq!(status.to_string(), label, "{status:?} label");
        }
    }

    #[test]
    fn entry_loading_mirrors_status_liveness() {
        fn entry_with(status: LocalAgentStatus) -> LocalAgentEntry {
            LocalAgentEntry {
                id: "agent-1".to_string(),
                display_label: "agent".to_string(),
                agent_name: "agent".to_string(),
                color: None,
                kind: LocalAgentKind::Delegated,
                program_status: vtcode_commons::program_status::ProgramState::Idle,
                updated_at: 0,
                status,
                summary: None,
                preview: String::new(),
                transcript_path: None,
            }
        }

        assert!(entry_with(LocalAgentStatus::Running).is_loading());
        assert!(!entry_with(LocalAgentStatus::Running).is_finished());
        assert!(!entry_with(LocalAgentStatus::Failed).is_loading());
        assert!(entry_with(LocalAgentStatus::Failed).is_finished());
    }
}
