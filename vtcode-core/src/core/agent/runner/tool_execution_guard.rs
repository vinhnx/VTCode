use std::sync::Arc;

use parking_lot::Mutex;

use crate::core::agent::error_recovery::{ErrorRecoveryState, ErrorType};

pub(super) struct ToolExecutionGuard {
    tool_name: String,
    tool_call_id: String,
    error_recovery: Arc<Mutex<ErrorRecoveryState>>,
    completed: bool,
}

impl ToolExecutionGuard {
    pub(super) fn new(
        tool_name: &str,
        tool_call_id: &str,
        error_recovery: Arc<Mutex<ErrorRecoveryState>>,
    ) -> Self {
        Self {
            tool_name: tool_name.to_string(),
            tool_call_id: tool_call_id.to_string(),
            error_recovery,
            completed: false,
        }
    }

    pub(super) fn mark_completed(&mut self) {
        self.completed = true;
    }
}

impl Drop for ToolExecutionGuard {
    fn drop(&mut self) {
        if self.completed {
            return;
        }

        let error_message = format!(
            "tool execution interrupted before completion (tool_call_id={})",
            self.tool_call_id
        );

        tracing::warn!(
            tool = %self.tool_name,
            tool_call_id = %self.tool_call_id,
            "tool execution guard dropped without completion"
        );

        self.error_recovery.lock().record_error_with_category(
            &self.tool_name,
            error_message,
            ErrorType::ToolExecution,
            Some(vtcode_commons::ErrorCategory::Cancelled),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::ToolExecutionGuard;
    use crate::core::agent::error_recovery::{ErrorRecoveryState, ErrorType};
    use parking_lot::Mutex;
    use std::sync::Arc;
    use vtcode_commons::ErrorCategory;

    #[test]
    fn drop_without_completion_records_interruption() {
        let recovery = Arc::new(Mutex::new(ErrorRecoveryState::new()));

        {
            let _guard = ToolExecutionGuard::new("read_file", "call-123", recovery.clone());
        }

        let state = recovery.lock();
        let error = state.recent_errors.front().expect("interruption recorded");
        assert_eq!(error.tool_name, "read_file");
        assert_eq!(error.error_type, ErrorType::ToolExecution);
        assert_eq!(error.category, Some(ErrorCategory::Cancelled));
        assert!(error.error_message.contains("call-123"));
    }

    #[test]
    fn mark_completed_suppresses_interruption_record() {
        let recovery = Arc::new(Mutex::new(ErrorRecoveryState::new()));

        {
            let mut guard = ToolExecutionGuard::new("read_file", "call-123", recovery.clone());
            guard.mark_completed();
        }

        assert!(recovery.lock().recent_errors.is_empty());
    }
}
