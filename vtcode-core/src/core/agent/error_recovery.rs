use std::collections::VecDeque;
use std::time::Instant;

pub const DEFAULT_MAX_RECENT_ERRORS: usize = 10;
pub const DEFAULT_MAX_OPEN_CIRCUITS: usize = 3;

#[derive(Debug, Clone)]
pub struct ErrorRecoveryState {
    pub recent_errors: VecDeque<RecentError>,
    pub circuit_events: Vec<CircuitEvent>,
    pub pause_threshold: usize,
    pub last_recovery_prompt: Option<Instant>,
    pub recovery_cooldown: std::time::Duration,
}

#[derive(Debug, Clone)]
pub struct RecentError {
    pub tool_name: String,
    pub timestamp: Instant,
    pub error_message: String,
    pub error_type: ErrorType,
}

// Re-export the canonical ErrorType from core::error_recovery
pub use crate::core::error_recovery::ErrorType;

#[derive(Debug, Clone)]
pub struct CircuitEvent {
    pub tool_name: String,
    pub timestamp: Instant,
    pub state: String,
    pub failure_count: u32,
    pub backoff_duration: std::time::Duration,
}

#[derive(Debug, Clone)]
pub struct RecoveryDiagnostics {
    pub open_circuits: Vec<String>,
    pub recent_errors: Vec<RecentError>,
    pub error_patterns: Vec<ErrorPattern>,
    pub should_pause: bool,
    pub pause_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ErrorPattern {
    pub tool_name: String,
    pub error_count: usize,
    pub common_error: String,
    pub error_types: Vec<ErrorType>,
}

impl Default for ErrorRecoveryState {
    fn default() -> Self {
        Self {
            recent_errors: VecDeque::with_capacity(DEFAULT_MAX_RECENT_ERRORS),
            circuit_events: Vec::new(),
            pause_threshold: DEFAULT_MAX_OPEN_CIRCUITS,
            last_recovery_prompt: None,
            recovery_cooldown: std::time::Duration::from_secs(60),
        }
    }
}

impl ErrorRecoveryState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_threshold(max_open_circuits: usize) -> Self {
        Self {
            recent_errors: VecDeque::with_capacity(DEFAULT_MAX_RECENT_ERRORS),
            circuit_events: Vec::new(),
            pause_threshold: max_open_circuits,
            last_recovery_prompt: None,
            recovery_cooldown: std::time::Duration::from_secs(60),
        }
    }

    pub fn record_error(&mut self, tool_name: &str, error_message: String, error_type: ErrorType) {
        self.recent_errors.push_front(RecentError {
            tool_name: tool_name.to_string(),
            timestamp: Instant::now(),
            error_message,
            error_type,
        });

        if self.recent_errors.len() > DEFAULT_MAX_RECENT_ERRORS {
            self.recent_errors.pop_back();
        }
    }

    pub fn record_circuit_event(
        &mut self,
        tool_name: &str,
        state: &str,
        failure_count: u32,
        backoff_duration: std::time::Duration,
    ) {
        self.circuit_events.push(CircuitEvent {
            tool_name: tool_name.to_string(),
            timestamp: Instant::now(),
            state: state.to_string(),
            failure_count,
            backoff_duration,
        });

        if self.circuit_events.len() > DEFAULT_MAX_RECENT_ERRORS {
            self.circuit_events.remove(0);
        }
    }

    pub fn get_diagnostics(
        &self,
        open_circuits: &[String],
        max_recent_errors: usize,
    ) -> RecoveryDiagnostics {
        let recent_errors: Vec<RecentError> = self
            .recent_errors
            .iter()
            .take(max_recent_errors)
            .cloned()
            .collect();

        let error_patterns = self.detect_error_patterns();

        let should_pause = open_circuits.len() >= self.pause_threshold;
        let pause_reason = if should_pause {
            Some(format!(
                "{} circuit(s) open: {}. Consider pausing for user guidance.",
                open_circuits.len(),
                open_circuits.join(", ")
            ))
        } else {
            None
        };

        RecoveryDiagnostics {
            open_circuits: open_circuits.to_vec(),
            recent_errors,
            error_patterns,
            should_pause,
            pause_reason,
        }
    }

    fn detect_error_patterns(&self) -> Vec<ErrorPattern> {
        let mut tool_errors: std::collections::HashMap<
            String,
            (usize, String, std::collections::HashSet<ErrorType>),
        > = std::collections::HashMap::new();

        for error in &self.recent_errors {
            let entry = tool_errors.entry(error.tool_name.clone()).or_insert((
                0,
                error.error_message.clone(),
                std::collections::HashSet::new(),
            ));
            entry.0 += 1;
            entry.2.insert(error.error_type);
        }

        tool_errors
            .into_iter()
            .filter(|(_, (count, _, _))| *count >= 2)
            .map(
                |(tool_name, (count, common_error, error_types))| ErrorPattern {
                    tool_name,
                    error_count: count,
                    common_error,
                    error_types: error_types.into_iter().collect(),
                },
            )
            .collect()
    }

    pub fn can_prompt_user(&self) -> bool {
        if let Some(last_prompt) = self.last_recovery_prompt {
            last_prompt.elapsed() >= self.recovery_cooldown
        } else {
            true
        }
    }

    pub fn mark_prompt_shown(&mut self) {
        self.last_recovery_prompt = Some(Instant::now());
    }

    pub fn clear_recent_errors(&mut self) {
        self.recent_errors.clear();
    }

    pub fn clear_circuit_events(&mut self) {
        self.circuit_events.clear();
    }

    pub fn reset(&mut self) {
        self.recent_errors.clear();
        self.circuit_events.clear();
        self.last_recovery_prompt = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_error() {
        let mut state = ErrorRecoveryState::new();

        state.record_error(
            "grep_file",
            "Pattern not found".to_string(),
            ErrorType::ToolExecution,
        );

        assert_eq!(state.recent_errors.len(), 1);
        assert_eq!(state.recent_errors[0].tool_name, "grep_file");
    }

    #[test]
    fn test_error_limit() {
        let mut state = ErrorRecoveryState::new();

        for i in 0..15 {
            state.record_error(
                &format!("tool_{}", i % 3),
                format!("error {}", i),
                ErrorType::ToolExecution,
            );
        }

        assert_eq!(state.recent_errors.len(), DEFAULT_MAX_RECENT_ERRORS);
    }

    #[test]
    fn test_error_pattern_detection() {
        let mut state = ErrorRecoveryState::new();

        for _i in 0..3 {
            state.record_error(
                "grep_file",
                "Pattern not found".to_string(),
                ErrorType::ToolExecution,
            );
        }

        state.record_error(
            "read_file",
            "File not found".to_string(),
            ErrorType::ResourceNotFound,
        );

        let diagnostics = state.get_diagnostics(&[], 10);
        assert_eq!(diagnostics.error_patterns.len(), 1);
        assert_eq!(diagnostics.error_patterns[0].tool_name, "grep_file");
        assert_eq!(diagnostics.error_patterns[0].error_count, 3);
    }

    #[test]
    fn test_pause_threshold() {
        let state = ErrorRecoveryState::with_threshold(3);

        let open_circuits = vec!["tool1".to_string(), "tool2".to_string()];
        let diagnostics = state.get_diagnostics(&open_circuits, 10);
        assert!(!diagnostics.should_pause);

        let open_circuits = vec![
            "tool1".to_string(),
            "tool2".to_string(),
            "tool3".to_string(),
        ];
        let diagnostics = state.get_diagnostics(&open_circuits, 10);
        assert!(diagnostics.should_pause);
    }

    #[test]
    fn test_cooldown() {
        let mut state = ErrorRecoveryState::new();

        assert!(state.can_prompt_user());

        state.mark_prompt_shown();
        assert!(!state.can_prompt_user());
    }
}
