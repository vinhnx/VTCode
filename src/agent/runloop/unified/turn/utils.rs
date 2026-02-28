use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;

use anyhow::Result;
use vtcode_core::config::constants::output_limits;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_tui::InlineHandle;

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

pub(crate) fn truncate_message_content(content: &str) -> String {
    let mut result =
        String::with_capacity(content.len().min(output_limits::MAX_AGENT_MESSAGES_SIZE));
    let mut truncated = false;

    for line in content.lines() {
        let mut line_bytes = 0;
        let mut end = 0;
        for (idx, ch) in line.char_indices() {
            let ch_len = ch.len_utf8();
            if line_bytes + ch_len > output_limits::MAX_LINE_LENGTH {
                truncated = true;
                break;
            }
            line_bytes += ch_len;
            end = idx + ch_len;
        }
        let trimmed_line = &line[..end];
        if result.len() + trimmed_line.len() + 1 > output_limits::MAX_AGENT_MESSAGES_SIZE {
            truncated = true;
            break;
        }
        result.push_str(trimmed_line);
        result.push('\n');
    }

    if truncated {
        result.push_str("[... content truncated due to size limit ...]");
    }

    result
}

pub(crate) fn enforce_history_limits(history: &mut Vec<uni::Message>) {
    let max_messages = output_limits::DEFAULT_MESSAGE_LIMIT.min(output_limits::MAX_MESSAGE_LIMIT);
    while history.len() > max_messages {
        if !remove_oldest_non_system(history) {
            break;
        }
    }

    loop {
        let total_bytes: usize = history.iter().map(|msg| msg.content.as_text().len()).sum();
        if total_bytes <= output_limits::MAX_ALL_MESSAGES_SIZE {
            break;
        }
        if !remove_oldest_non_system(history) {
            break;
        }
    }
}

fn remove_oldest_non_system(history: &mut Vec<uni::Message>) -> bool {
    if history.is_empty() {
        return false;
    }
    if history[0].role != uni::MessageRole::System {
        history.remove(0);
        return true;
    }
    if history.len() > 1 {
        history.remove(1);
        return true;
    }
    false
}
pub(crate) fn should_trigger_turn_balancer(
    step_count: usize,
    max_tool_loops: usize,
    repeated: usize,
    repeat_limit: usize,
) -> bool {
    let step_threshold = max_tool_loops.saturating_mul(3) / 4;
    let effective_repeat_limit = repeat_limit.max(3);
    step_count > step_threshold && repeated >= effective_repeat_limit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balancer_triggers_after_three_quarters_and_effective_repeat_limit() {
        assert!(should_trigger_turn_balancer(16, 20, 3, 3));
        assert!(!should_trigger_turn_balancer(15, 20, 3, 3));
        assert!(!should_trigger_turn_balancer(16, 20, 2, 3));
        assert!(!should_trigger_turn_balancer(16, 20, 2, 2));
    }

    #[test]
    fn truncate_message_content_limits_lines_and_size() {
        let long_line = "a".repeat(output_limits::MAX_LINE_LENGTH + 16);
        let truncated = truncate_message_content(&long_line);

        assert!(truncated.contains("content truncated"));
        assert!(truncated.len() <= output_limits::MAX_AGENT_MESSAGES_SIZE);
    }

    #[test]
    fn enforce_history_limits_caps_message_count_and_keeps_system() {
        let mut history = Vec::new();
        history.push(uni::Message::system("system".to_string()));
        for idx in 0..(output_limits::DEFAULT_MESSAGE_LIMIT + 1) {
            history.push(uni::Message::assistant(format!("msg {}", idx)));
        }

        enforce_history_limits(&mut history);

        assert!(history.len() <= output_limits::DEFAULT_MESSAGE_LIMIT);
        assert_eq!(
            history.first().map(|m| m.role.clone()),
            Some(uni::MessageRole::System)
        );
    }
}
