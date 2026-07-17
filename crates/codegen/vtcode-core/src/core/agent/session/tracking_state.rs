//! Side-effect and progress tracking for agent sessions.
//!
//! Extracts the tracking fields from `AgentSessionState` into a focused,
//! independently testable unit. Tracks files modified, commands executed,
//! contexts created, and warnings emitted during a session.

/// Tracks side-effects and progress during an agent session.
///
/// This struct is self-contained: it can record file modifications,
/// command executions, and warnings without access to the rest of the
/// session state.
#[derive(Debug, Clone, Default)]
pub struct TrackingState {
    /// Contexts created during the session.
    pub created_contexts: Vec<String>,
    /// Files modified during the session.
    pub modified_files: Vec<String>,
    /// Commands executed during the session.
    pub executed_commands: Vec<String>,
    /// Warnings emitted during the session.
    pub warnings: Vec<String>,
    /// Last file path accessed.
    pub last_file_path: Option<String>,
    /// Last directory path accessed.
    pub last_dir_path: Option<String>,
}

impl TrackingState {
    /// Record a file modification.
    pub fn record_file_modified(&mut self, path: impl Into<String>) {
        let path = path.into();
        self.last_file_path = Some(path.clone());
        if !self.modified_files.contains(&path) {
            self.modified_files.push(path);
        }
    }

    /// Record a command execution.
    pub fn record_command_executed(&mut self, command: impl Into<String>) {
        let command = command.into();
        if !self.executed_commands.contains(&command) {
            self.executed_commands.push(command);
        }
    }

    /// Record a context creation.
    pub fn record_context_created(&mut self, context: impl Into<String>) {
        let context = context.into();
        if !self.created_contexts.contains(&context) {
            self.created_contexts.push(context);
        }
    }

    /// Record a warning.
    pub fn record_warning(&mut self, warning: impl Into<String>) {
        self.warnings.push(warning.into());
    }

    /// Set the last accessed directory.
    pub fn set_last_dir(&mut self, dir: impl Into<String>) {
        self.last_dir_path = Some(dir.into());
    }

    /// Check if any files were modified.
    pub fn has_modifications(&self) -> bool {
        !self.modified_files.is_empty()
    }

    /// Get the total number of side effects recorded.
    pub fn total_side_effects(&self) -> usize {
        self.modified_files.len()
            + self.executed_commands.len()
            + self.created_contexts.len()
            + self.warnings.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_tracking_is_empty() {
        let tracking = TrackingState::default();
        assert!(tracking.modified_files.is_empty());
        assert!(tracking.executed_commands.is_empty());
        assert!(!tracking.has_modifications());
        assert_eq!(tracking.total_side_effects(), 0);
    }

    #[test]
    fn record_file_modified_deduplicates() {
        let mut tracking = TrackingState::default();
        tracking.record_file_modified("src/main.rs");
        tracking.record_file_modified("src/main.rs");
        assert_eq!(tracking.modified_files.len(), 1);
        assert_eq!(tracking.last_file_path.as_deref(), Some("src/main.rs"));
    }

    #[test]
    fn record_command_executed_deduplicates() {
        let mut tracking = TrackingState::default();
        tracking.record_command_executed("cargo test");
        tracking.record_command_executed("cargo test");
        assert_eq!(tracking.executed_commands.len(), 1);
    }

    #[test]
    fn record_context_created_deduplicates() {
        let mut tracking = TrackingState::default();
        tracking.record_context_created("ctx-1");
        tracking.record_context_created("ctx-1");
        assert_eq!(tracking.created_contexts.len(), 1);
    }

    #[test]
    fn record_warning_allows_duplicates() {
        let mut tracking = TrackingState::default();
        tracking.record_warning("low memory");
        tracking.record_warning("low memory");
        assert_eq!(tracking.warnings.len(), 2);
    }

    #[test]
    fn has_modifications_returns_true_after_file() {
        let mut tracking = TrackingState::default();
        assert!(!tracking.has_modifications());
        tracking.record_file_modified("test.txt");
        assert!(tracking.has_modifications());
    }

    #[test]
    fn total_side_effects_counts_all() {
        let mut tracking = TrackingState::default();
        tracking.record_file_modified("a.txt");
        tracking.record_command_executed("ls");
        tracking.record_context_created("ctx");
        tracking.record_warning("warn");
        assert_eq!(tracking.total_side_effects(), 4);
    }

    #[test]
    fn set_last_dir_updates() {
        let mut tracking = TrackingState::default();
        tracking.set_last_dir("/tmp");
        assert_eq!(tracking.last_dir_path.as_deref(), Some("/tmp"));
    }
}
