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
#[derive(Clone)]
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
}

impl Default for ApprovalRecorder {
    fn default() -> Self {
        // This default implementation creates a temporary directory for the cache.
        // In a real application, you might want a more robust default path or
        // to make `new` take an optional `cache_dir`.
        let temp_dir =
            std::env::temp_dir().join(format!("approval_recorder_default_{}", std::process::id()));
        Self::new(temp_dir)
    }
}

impl ApprovalRecorder {
    /// Record a user's approval decision for a learned approval key
    pub async fn record_approval(
        &self,
        approval_key: &str,
        display_name: Option<&str>,
        approved: bool,
        reason: Option<String>,
    ) -> Result<()> {
        let manager = self.manager.write().await;
        manager.record_decision(approval_key, display_name, approved, reason);
        Ok(())
    }

    /// Get the approval pattern for a learned approval key
    pub async fn get_pattern(&self, approval_key: &str) -> Option<ApprovalPattern> {
        let manager = self.manager.read().await;
        manager.get_pattern(approval_key)
    }

    /// Check if a key has high approval rate from history
    pub async fn has_high_approval_rate(&self, approval_key: &str) -> bool {
        let manager = self.manager.read().await;
        if let Some(pattern) = manager.get_pattern(approval_key) {
            pattern.has_high_approval_rate()
        } else {
            false
        }
    }

    /// Get learning summary for a learned approval key
    pub async fn get_learning_summary(&self, approval_key: &str) -> Option<String> {
        let manager = self.manager.read().await;
        manager.get_learning_summary(approval_key)
    }

    /// Get approval count for a learned approval key
    pub async fn get_approval_count(&self, approval_key: &str) -> u32 {
        let manager = self.manager.read().await;
        if let Some(pattern) = manager.get_pattern(approval_key) {
            pattern.approval_count()
        } else {
            0
        }
    }

    /// Should auto-approve based on approval pattern
    /// Rules:
    /// - At least 3 approvals
    /// - Approval rate > 80%
    ///
    /// Refreshes the in-memory pattern map from disk first so we observe
    /// approvals recorded by concurrent sessions (e.g. another running vtcode
    /// instance sharing the same `~/.vtcode/cache/approval_patterns.json`).
    pub async fn should_auto_approve(&self, approval_key: &str) -> bool {
        let manager = self.manager.write().await;
        if let Err(err) = manager.refresh_patterns() {
            tracing::debug!(
                approval_key = %approval_key,
                error = %err,
                "Failed to refresh approval patterns before auto-approve check"
            );
        }
        if let Some(pattern) = manager.get_pattern(approval_key) {
            pattern.has_high_approval_rate()
        } else {
            false
        }
    }

    /// Suggest auto-approval message if user has approved this target many times
    pub async fn get_auto_approval_suggestion(
        &self,
        approval_key: &str,
        fallback_display_name: &str,
    ) -> Option<String> {
        let manager = self.manager.read().await;
        if let Some(pattern) = manager.get_pattern(approval_key) {
            let rate = pattern.approval_rate();
            if pattern.approval_count() >= 5 {
                let display_name = pattern.display_name(fallback_display_name);
                return Some(format!(
                    "You've approved {} {} times ({:.0}% approval rate)",
                    display_name,
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
        recorder
            .record_approval("read_file", Some("Read File"), true, None)
            .await
            .unwrap();
        recorder
            .record_approval("read_file", Some("Read File"), true, None)
            .await
            .unwrap();
        recorder
            .record_approval("read_file", Some("Read File"), false, None)
            .await
            .unwrap();

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
                .get_auto_approval_suggestion("read_file", "Read File")
                .await
                .is_none()
        );

        // Add 5 approvals
        for _ in 0..5 {
            let _ = recorder
                .record_approval("read_file", Some("Read File"), true, None)
                .await;
        }

        // Now we should get a suggestion
        let suggestion = recorder
            .get_auto_approval_suggestion("read_file", "Read File")
            .await;
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
            let _ = recorder
                .record_approval("run_command", Some("Run Command"), true, None)
                .await;
        }

        // Now should auto-approve
        assert!(recorder.should_auto_approve("run_command").await);

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_auto_approval_suggestion_uses_display_name() {
        let temp_dir = std::env::temp_dir().join(format!("vtcode_test_{}", std::process::id()));
        let recorder = ApprovalRecorder::new(temp_dir.clone());

        for _ in 0..5 {
            let _ = recorder
                .record_approval(
                    "cargo test|sandbox_permissions=\"require_escalated\"|additional_permissions=null",
                    Some("commands starting with `cargo test`"),
                    true,
                    None,
                )
                .await;
        }

        let suggestion = recorder
            .get_auto_approval_suggestion(
                "cargo test|sandbox_permissions=\"require_escalated\"|additional_permissions=null",
                "fallback label",
            )
            .await
            .expect("suggestion");
        assert!(suggestion.contains("commands starting with `cargo test`"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_should_auto_approve_refreshes_patterns_from_disk() {
        // Simulates a second vtcode session: one ApprovalRecorder records
        // approvals to disk, then a separately constructed recorder must
        // observe them on the next auto-approve check without restart.
        let temp_dir = std::env::temp_dir().join(format!(
            "vtcode_test_refresh_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);

        let key = "find src -type f -name '*.rs' '|' sort|sandbox_permissions=\"use_default\"|additional_permissions=null";

        let reader = ApprovalRecorder::new(temp_dir.clone());
        assert!(!reader.should_auto_approve(key).await);

        let writer = ApprovalRecorder::new(temp_dir.clone());
        for _ in 0..3 {
            writer
                .record_approval(key, Some("find src"), true, None)
                .await
                .unwrap();
        }

        // Without the disk refresh in should_auto_approve, the reader's
        // in-memory map would still be empty and this assertion would fail.
        assert!(reader.should_auto_approve(key).await);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_shell_scoped_history_does_not_reuse_tool_level_key() {
        let temp_dir = std::env::temp_dir().join(format!("vtcode_test_{}", std::process::id()));
        let recorder = ApprovalRecorder::new(temp_dir.clone());

        for _ in 0..5 {
            let _ = recorder
                .record_approval("unified_exec", Some("Unified Exec"), true, None)
                .await;
        }

        assert_eq!(
            recorder
                .get_approval_count(
                    "cargo test|sandbox_permissions=\"require_escalated\"|additional_permissions=null"
                )
                .await,
            0
        );
        assert!(
            recorder
                .get_auto_approval_suggestion(
                    "cargo test|sandbox_permissions=\"require_escalated\"|additional_permissions=null",
                    "commands starting with `cargo test`",
                )
                .await
                .is_none()
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
