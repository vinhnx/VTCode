use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;

use anyhow::Result;
use vtcode_core::ui::tui::InlineHandle;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::hooks::lifecycle::{HookMessage, HookMessageLevel};

/// UI Redraw Batcher for optimizing UI updates
/// This struct batches multiple redraw requests into a single operation
/// to reduce UI jank and improve performance
#[allow(dead_code)]
pub struct UIRedrawBatcher {
    handle: InlineHandle,
    pending_redraws: Arc<Mutex<usize>>,
    last_redraw_time: Arc<Mutex<Instant>>,
    min_batch_interval: Duration,
    max_batch_size: usize,
    auto_flush_notify: Arc<Notify>,
    auto_flush_task: Option<JoinHandle<()>>,
    enabled: bool,
}

impl UIRedrawBatcher {
    /// Create a new UIRedrawBatcher
    pub fn new(handle: InlineHandle) -> Self {
        Self {
            handle,
            pending_redraws: Arc::new(Mutex::new(0)),
            last_redraw_time: Arc::new(Mutex::new(Instant::now())),
            min_batch_interval: Duration::from_millis(50), // Minimum 50ms between redraws
            max_batch_size: 5, // Maximum 5 pending redraws before forcing
            auto_flush_notify: Arc::new(Notify::new()),
            auto_flush_task: None,
            enabled: true,
        }
    }

    /// Create a new UIRedrawBatcher with auto-flush enabled
    pub fn with_auto_flush(handle: InlineHandle) -> Self {
        let mut batcher = Self::new(handle);
        batcher.start_auto_flush();
        batcher
    }

    /// Start the auto-flush task
    pub fn start_auto_flush(&mut self) {
        if self.auto_flush_task.is_none() {
            let pending_redraws = self.pending_redraws.clone();
            let last_redraw_time = self.last_redraw_time.clone();
            let auto_flush_notify = self.auto_flush_notify.clone();
            let handle = self.handle.clone();
            let min_batch_interval = self.min_batch_interval;

            self.auto_flush_task = Some(tokio::spawn(async move {
                loop {
                    // Wait for notification or timeout
                    tokio::select! {
                        _ = auto_flush_notify.notified() => {
                            // Check if we should flush
                            let mut pending = pending_redraws.lock().await;
                            if *pending > 0 {
                                let mut last_redraw = last_redraw_time.lock().await;
                                if last_redraw.elapsed() >= min_batch_interval {
                                    handle.force_redraw();
                                    *pending = 0;
                                    *last_redraw = Instant::now();
                                }
                            }
                        }
                        _ = tokio::time::sleep(Duration::from_millis(100)) => {
                            // Periodic check
                            let mut pending = pending_redraws.lock().await;
                            if *pending > 0 {
                                let mut last_redraw = last_redraw_time.lock().await;
                                if last_redraw.elapsed() >= min_batch_interval {
                                    handle.force_redraw();
                                    *pending = 0;
                                    *last_redraw = Instant::now();
                                }
                            }
                        }
                    }
                }
            }));
        }
    }

    /// Request a redraw (batched)
    #[allow(dead_code)]
    pub async fn request_redraw(&self) {
        if !self.enabled {
            self.handle.force_redraw();
            return;
        }

        let mut pending = self.pending_redraws.lock().await;
        *pending += 1;

        // Check if we should flush the batch
        if *pending >= self.max_batch_size {
            self.flush().await;
        } else {
            // Notify auto-flush task
            self.auto_flush_notify.notify_one();
        }
    }

    /// Flush pending redraws if enough time has passed or batch is full
    #[allow(dead_code)]
    pub async fn flush(&self) {
        let mut pending = self.pending_redraws.lock().await;
        if *pending == 0 {
            return;
        }

        let mut last_redraw = self.last_redraw_time.lock().await;
        if last_redraw.elapsed() >= self.min_batch_interval {
            self.handle.force_redraw();
            *pending = 0;
            *last_redraw = Instant::now();
        }
    }

    /// Force immediate redraw (bypasses batching)
    #[allow(dead_code)]
    pub fn force_redraw(&self) {
        self.handle.force_redraw();
        // Reset counters
        let _ = self.pending_redraws.try_lock(); // Clear pending count
        let _ = self.last_redraw_time.try_lock(); // Update last redraw time
    }

    /// Set minimum batch interval
    #[allow(dead_code)]
    pub fn set_min_batch_interval(&mut self, duration: Duration) {
        self.min_batch_interval = duration;
    }

    /// Set maximum batch size
    #[allow(dead_code)]
    pub fn set_max_batch_size(&mut self, size: usize) {
        self.max_batch_size = size;
    }
}

#[allow(dead_code)]
pub(super) fn safe_force_redraw(handle: &InlineHandle, last_forced_redraw: &mut Instant) {
    if last_forced_redraw.elapsed() > std::time::Duration::from_millis(100) {
        handle.force_redraw();
        *last_forced_redraw = Instant::now();
    }
}

pub(crate) fn render_hook_messages(
    renderer: &mut AnsiRenderer,
    messages: &[HookMessage],
) -> Result<()> {
    for message in messages {
        let text = message.text.trim();
        if text.is_empty() {
            continue;
        }

        let style = match message.level {
            HookMessageLevel::Info => MessageStyle::Info,
            HookMessageLevel::Warning => MessageStyle::Info,
            HookMessageLevel::Error => MessageStyle::Error,
        };

        renderer.line(style, text)?;
    }

    Ok(())
}
pub(crate) fn should_trigger_turn_balancer(
    step_count: usize,
    max_tool_loops: usize,
    repeated: usize,
    repeat_limit: usize,
) -> bool {
    step_count > max_tool_loops / 2 && repeated >= repeat_limit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balancer_triggers_only_after_halfway_and_repeats() {
        assert!(should_trigger_turn_balancer(11, 20, 3, 3));
        assert!(!should_trigger_turn_balancer(9, 20, 3, 3));
        assert!(!should_trigger_turn_balancer(12, 20, 2, 3));
    }
}
