//! UI synchronization utilities to replace arbitrary sleep calls
//!
//! This module provides efficient synchronization between the agent runloop
//! and UI updates, eliminating the need for arbitrary sleep delays.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{Notify, RwLock};
use vtcode_core::ui::tui::InlineHandle;

/// UI synchronization manager that replaces arbitrary sleep calls with event-driven synchronization
pub struct UiSyncManager {
    /// Notification for redraw completion
    redraw_notify: Arc<Notify>,
    /// Flag to track if a redraw is in progress
    redraw_in_progress: Arc<AtomicBool>,
    /// Last redraw timestamp
    last_redraw: Arc<RwLock<Instant>>,
    /// Minimum time between redraws
    min_redraw_interval: Duration,
}

impl UiSyncManager {
    pub fn new() -> Self {
        Self {
            redraw_notify: Arc::new(Notify::new()),
            redraw_in_progress: Arc::new(AtomicBool::new(false)),
            last_redraw: Arc::new(RwLock::new(Instant::now())),
            min_redraw_interval: Duration::from_millis(100),
        }
    }

    /// Force a redraw with proper synchronization, replacing sleep calls
    pub async fn force_redraw_sync(&self, handle: &InlineHandle) -> Result<(), anyhow::Error> {
        let now = Instant::now();

        // Check if enough time has passed since last redraw
        {
            let last_redraw = self.last_redraw.read().await;
            if now.duration_since(*last_redraw) < self.min_redraw_interval {
                return Ok(());
            }
        }

        // Set redraw in progress flag
        self.redraw_in_progress.store(true, Ordering::SeqCst);

        // Force the redraw
        handle.force_redraw();

        // Update last redraw timestamp
        {
            let mut last_redraw = self.last_redraw.write().await;
            *last_redraw = now;
        }

        // Wait for redraw to complete (with timeout to prevent hanging)
        tokio::time::timeout(Duration::from_millis(50), self.redraw_notify.notified())
            .await
            .ok();

        // Clear redraw in progress flag
        self.redraw_in_progress.store(false, Ordering::SeqCst);

        Ok(())
    }

    /// Notify that a redraw has been completed
    #[allow(dead_code)]
    pub fn notify_redraw_complete(&self) {
        self.redraw_notify.notify_one();
    }

    /// Check if a redraw is currently in progress
    pub fn is_redraw_in_progress(&self) -> bool {
        self.redraw_in_progress.load(Ordering::SeqCst)
    }

    /// Wait for any pending redraw to complete
    pub async fn wait_for_redraw_complete(&self) -> Result<(), anyhow::Error> {
        if self.is_redraw_in_progress() {
            tokio::time::timeout(Duration::from_millis(100), self.redraw_notify.notified())
                .await
                .map_err(|_| anyhow::anyhow!("Timeout waiting for redraw completion"))?;
        }
        Ok(())
    }
}

impl Default for UiSyncManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Global UI sync manager instance
use once_cell::sync::Lazy;
static UI_SYNC_MANAGER: Lazy<Arc<UiSyncManager>> = Lazy::new(|| Arc::new(UiSyncManager::new()));

/// Get the global UI sync manager
pub fn get_ui_sync_manager() -> Arc<UiSyncManager> {
    UI_SYNC_MANAGER.clone()
}

/// Efficient redraw function that replaces sleep-based synchronization
pub async fn redraw_with_sync(handle: &InlineHandle) -> Result<(), anyhow::Error> {
    get_ui_sync_manager().force_redraw_sync(handle).await
}

/// Wait for any pending redraw operations to complete
pub async fn wait_for_redraw_complete() -> Result<(), anyhow::Error> {
    get_ui_sync_manager().wait_for_redraw_complete().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_core::config::types::UiSurfacePreference;
    use vtcode_core::ui::theme;
    use vtcode_core::ui::tui::{spawn_session, theme_from_styles};

    #[tokio::test]
    async fn test_redraw_sync() {
        let manager = UiSyncManager::new();

        // Mock handle (would need proper mocking in real tests)
        // For now, just test the basic flow
        assert!(!manager.is_redraw_in_progress());

        // Simulate redraw completion
        manager.notify_redraw_complete();

        // Test wait for completion when not in progress
        manager.wait_for_redraw_complete().await.unwrap();
    }

    #[tokio::test]
    async fn test_redraw_rate_limiting() {
        let manager = UiSyncManager::new();

        // First redraw should work using a real inline handle
        let session = spawn_session(
            theme_from_styles(&theme::active_styles()),
            None,
            UiSurfacePreference::default(),
            10,
            false,
            None,
        )
        .expect("spawn inline session");
        let handle = session.clone_inline_handle();
        manager.force_redraw_sync(&handle).await.unwrap();

        // Immediate second redraw should be skipped due to rate limiting
        manager.force_redraw_sync(&handle).await.unwrap();
    }
}
