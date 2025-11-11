/// Approval Decision Recording and Learning
///
/// Records user approval decisions for high-risk tools and enables pattern learning
/// to reduce approval friction over time.
use super::justification::{ApprovalPattern, JustificationManager};
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Records tool approval decisions for learning
pub struct ApprovalRecorder {
    manager: Arc<RwLock<JustificationManager>>,
}

impl ApprovalRecorder {
    /// Create a new approval recorder
    pub fn new(cache_dir: PathBuf) -> Self {
        let manager = JustificationManager::new(cache_dir);
        Self {
            manager: Arc::new(RwLock::new(manager)),
        }
    }

    /// Record a user's approval decision for a tool
    pub async fn record_approval(
        &self,
        tool_name: &str,
        approved: bool,
        reason: Option<String>,
    ) -> Result<()> {
        let mut manager = self.manager.write().await;
        manager.record_decision(tool_name, approved, reason);
        Ok(())
    }

    /// Get the approval pattern for a tool
    pub async fn get_pattern(&self, tool_name: &str) -> Option<ApprovalPattern> {
        let manager = self.manager.read().await;
        manager.get_pattern(tool_name)
    }

    /// Check if a tool has high approval rate from history
    pub async fn has_high_approval_rate(&self, tool_name: &str) -> bool {
        let manager = self.manager.read().await;
        if let Some(pattern) = manager.get_pattern(tool_name) {
            pattern.has_high_approval_rate()
        } else {
            false
        }
    }

    /// Get learning summary for a tool
    pub async fn get_learning_summary(&self, tool_name: &str) -> Option<String> {
        let manager = self.manager.read().await;
        manager.get_learning_summary(tool_name)
    }

    /// Get approval count for a tool
    pub async fn get_approval_count(&self, tool_name: &str) -> u32 {
        let manager = self.manager.read().await;
        if let Some(pattern) = manager.get_pattern(tool_name) {
            pattern.approval_count()
        } else {
            0
        }
    }

    /// Should auto-approve based on approval pattern
    /// Rules:
    /// - At least 3 approvals
    /// - Approval rate > 80%
    pub async fn should_auto_approve(&self, tool_name: &str) -> bool {
        let manager = self.manager.read().await;
        if let Some(pattern) = manager.get_pattern(tool_name) {
            pattern.has_high_approval_rate()
        } else {
            false
        }
    }

    /// Suggest auto-approval message if user has approved this tool many times
    pub async fn get_auto_approval_suggestion(&self, tool_name: &str) -> Option<String> {
        let manager = self.manager.read().await;
        if let Some(pattern) = manager.get_pattern(tool_name) {
            let rate = pattern.approval_rate();
            if pattern.approval_count() >= 5 {
                return Some(format!(
                    "You've approved this tool {} times ({:.0}% approval rate)",
                    pattern.approval_count(),
                    rate * 100.0
                ));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_approval_recording() {
        let temp_dir = std::env::temp_dir().join(format!("vtcode_test_{}", std::process::id()));
        let recorder = ApprovalRecorder::new(temp_dir.clone());

        // Record some approvals
        assert!(
            recorder
                .record_approval("read_file", true, None)
                .await
                .is_ok()
        );
        assert!(
            recorder
                .record_approval("read_file", true, None)
                .await
                .is_ok()
        );
        assert!(
            recorder
                .record_approval("read_file", false, None)
                .await
                .is_ok()
        );

        // Check pattern
        let pattern = recorder.get_pattern("read_file").await;
        assert!(pattern.is_some());
        assert_eq!(pattern.unwrap().approval_count(), 2);

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_auto_approval_suggestion() {
        let temp_dir = std::env::temp_dir().join(format!("vtcode_test_{}", std::process::id()));
        let recorder = ApprovalRecorder::new(temp_dir.clone());

        // Not enough approvals initially
        assert!(
            recorder
                .get_auto_approval_suggestion("read_file")
                .await
                .is_none()
        );

        // Add 5 approvals
        for _ in 0..5 {
            let _ = recorder.record_approval("read_file", true, None).await;
        }

        // Now we should get a suggestion
        let suggestion = recorder.get_auto_approval_suggestion("read_file").await;
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("100%"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_should_auto_approve() {
        let temp_dir = std::env::temp_dir().join(format!("vtcode_test_{}", std::process::id()));
        let recorder = ApprovalRecorder::new(temp_dir.clone());

        // Not approved initially
        assert!(!recorder.should_auto_approve("run_command").await);

        // Add 3 approvals (minimum threshold)
        for _ in 0..3 {
            let _ = recorder.record_approval("run_command", true, None).await;
        }

        // Now should auto-approve
        assert!(recorder.should_auto_approve("run_command").await);

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
