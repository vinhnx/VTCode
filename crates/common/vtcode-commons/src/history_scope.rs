//! Cross-platform prompt-history search scope.
//!
//! Shared by the CLI history picker, the desktop history palette, and the
//! desktop service so scope labels, cycling order, and wire values cannot
//! drift between surfaces.

use serde::{Deserialize, Serialize};

/// Which sessions contribute prompt-history matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoryScope {
    /// Every session in every workspace.
    #[default]
    Everywhere,
    /// Sessions in the current workspace/project only.
    Project,
    /// The current session/thread only.
    Session,
}

impl HistoryScope {
    /// Short display label used in picker titles; also the wire value.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Everywhere => "everywhere",
            Self::Project => "project",
            Self::Session => "session",
        }
    }

    /// Title-case label for switcher buttons. English-only until
    /// native-quality translations ship (thread-menu precedent).
    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Everywhere => "Everywhere",
            Self::Project => "Project",
            Self::Session => "Session",
        }
    }

    /// Next scope in the Ctrl+S cycle order (broad to narrow).
    #[must_use]
    pub fn cycle_next(self) -> Self {
        match self {
            Self::Everywhere => Self::Project,
            Self::Project => Self::Session,
            Self::Session => Self::Everywhere,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_runs_broad_to_narrow_and_wraps() {
        assert_eq!(HistoryScope::Everywhere.cycle_next(), HistoryScope::Project);
        assert_eq!(HistoryScope::Project.cycle_next(), HistoryScope::Session);
        assert_eq!(HistoryScope::Session.cycle_next(), HistoryScope::Everywhere);
    }

    #[test]
    fn labels_match_wire_values() {
        for scope in [HistoryScope::Everywhere, HistoryScope::Project, HistoryScope::Session] {
            let encoded = serde_json::to_value(scope).expect("scope serializes");
            assert_eq!(encoded, serde_json::Value::String(scope.label().to_string()));
            let decoded: HistoryScope = serde_json::from_value(encoded).expect("scope deserializes");
            assert_eq!(decoded, scope);
        }
        assert_eq!(HistoryScope::Everywhere.title(), "Everywhere");
        assert_eq!(HistoryScope::Project.title(), "Project");
        assert_eq!(HistoryScope::Session.title(), "Session");
    }

    #[test]
    fn unknown_wire_values_reject() {
        for raw in ["", "EventEverywhere", "workspace", "galaxy"] {
            assert!(
                serde_json::from_value::<HistoryScope>(serde_json::Value::String(raw.to_string())).is_err(),
                "{raw:?} must not parse as a scope"
            );
        }
    }
}
