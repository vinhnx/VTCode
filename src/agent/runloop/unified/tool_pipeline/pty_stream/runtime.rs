use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::{sync::mpsc, task::JoinHandle};
use vtcode_core::tools::registry::ToolProgressCallback;
use vtcode_tui::{InlineHandle, InlineMessageKind};

use crate::agent::runloop::unified::progress::ProgressReporter;

use super::state::PtyStreamState;

pub(crate) struct PtyStreamRuntime {
    pub(crate) sender: Option<mpsc::UnboundedSender<String>>,
    pub(crate) task: Option<JoinHandle<()>>,
    pub(crate) active: Arc<AtomicBool>,
}

impl PtyStreamRuntime {
    const MAX_LIVE_STREAM_LINES: usize = 12;

    pub(crate) fn start(
        handle: InlineHandle,
        progress_reporter: ProgressReporter,
        tail_limit: usize,
        command_prompt: Option<String>,
    ) -> (Self, ToolProgressCallback) {
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        let active = Arc::new(AtomicBool::new(true));
        let worker_active = Arc::clone(&active);
        let effective_tail_limit = tail_limit.clamp(1, Self::MAX_LIVE_STREAM_LINES);

        let task = tokio::spawn(async move {
            let mut state = PtyStreamState::new(command_prompt);
            let (replace_count, segments, link_ranges, _) =
                state.render_segments("", effective_tail_limit);
            if !segments.is_empty() && worker_active.load(Ordering::Relaxed) {
                handle.replace_last_with_links(
                    replace_count,
                    InlineMessageKind::Pty,
                    segments,
                    link_ranges,
                );
            }

            while let Some(output) = rx.recv().await {
                if !worker_active.load(Ordering::Relaxed) {
                    break;
                }
                if output.is_empty() {
                    continue;
                }

                state.apply_chunk(&output, effective_tail_limit);
                let visible_output = vtcode_core::utils::ansi_parser::strip_ansi(&output);
                if visible_output.trim().is_empty() {
                    continue;
                }

                let (replace_count, segments, link_ranges, last_line) =
                    state.render_current_segments(effective_tail_limit);
                if !segments.is_empty() && worker_active.load(Ordering::Relaxed) {
                    handle.replace_last_with_links(
                        replace_count,
                        InlineMessageKind::Pty,
                        segments,
                        link_ranges,
                    );
                }

                if let Some(last_line) = last_line {
                    let cleaned_last_line = vtcode_core::utils::ansi_parser::strip_ansi(&last_line);
                    if !cleaned_last_line.trim().is_empty() {
                        progress_reporter.set_message(cleaned_last_line).await;
                    }
                }
            }
        });

        let callback_active = Arc::clone(&active);
        let callback_tx = tx.clone();
        let callback: ToolProgressCallback = Arc::new(move |_name: &str, output: &str| {
            if !callback_active.load(Ordering::Relaxed) || output.is_empty() {
                return;
            }
            let _ = callback_tx.send(output.to_string());
        });

        (
            Self {
                sender: Some(tx),
                task: Some(task),
                active,
            },
            callback,
        )
    }

    pub(crate) async fn shutdown(mut self) {
        self.active.store(false, Ordering::Relaxed);
        let _ = self.sender.take();
        if let Some(task) = self.task.take() {
            let _ = tokio::time::timeout(Duration::from_millis(250), task).await;
        }
    }
}

impl Drop for PtyStreamRuntime {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Relaxed);
        let _ = self.sender.take();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
