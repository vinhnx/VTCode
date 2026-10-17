use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use anstyle::Color;
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};
use vtcode_core::config::PtyConfig;
use vtcode_core::tools::registry::ToolProgressCallback;
use vtcode_ui::tui::app::{InlineHandle, InlineMessageKind};

use crate::agent::runloop::unified::progress::ProgressReporter;

use super::state::PtyStreamState;

enum PtyStreamMessage {
    Output(String),
}

const MAX_COALESCED_LIVE_PREVIEW_BYTES: usize = 32 * 1024;

/// Records output that could not enter the bounded live-preview queue. The
/// complete PTY transcript is captured separately; this small coalescing
/// buffer keeps the visible preview honest about pressure without allowing a
/// fast producer to grow memory without bound.
struct LivePreviewDropState {
    pending: StdMutex<String>,
    dropped_chunks: AtomicU64,
    dropped_bytes: AtomicU64,
}

impl LivePreviewDropState {
    fn new() -> Self {
        Self {
            pending: StdMutex::new(String::new()),
            dropped_chunks: AtomicU64::new(0),
            dropped_bytes: AtomicU64::new(0),
        }
    }

    fn record(&self, output: &str) {
        self.dropped_chunks.fetch_add(1, Ordering::Relaxed);
        self.dropped_bytes
            .fetch_add(u64::try_from(output.len()).unwrap_or(u64::MAX), Ordering::Relaxed);

        let Ok(mut pending) = self.pending.lock() else {
            return;
        };
        let remaining = MAX_COALESCED_LIVE_PREVIEW_BYTES.saturating_sub(pending.len());
        if remaining == 0 {
            return;
        }
        pending.push_str(&truncate_to_byte_limit(output, remaining));
    }

    fn take_pending(&self) -> Option<String> {
        let chunks = self.dropped_chunks.swap(0, Ordering::AcqRel);
        let bytes = self.dropped_bytes.swap(0, Ordering::AcqRel);
        let Ok(mut pending) = self.pending.lock() else {
            return (chunks > 0).then(|| format!("\n[live preview coalesced {chunks} chunks / {bytes} bytes]\n"));
        };
        let output = std::mem::take(&mut *pending);
        if chunks == 0 {
            return (!output.is_empty()).then_some(output);
        }
        Some(format!("\n[live preview coalesced {chunks} chunks / {bytes} bytes]\n{output}"))
    }
}

fn truncate_to_byte_limit(text: &str, limit: usize) -> String {
    vtcode_commons::formatting::truncate_utf8_prefix(text, limit).to_string()
}

pub(crate) struct PtyStreamRuntime {
    sender: Option<mpsc::Sender<PtyStreamMessage>>,
    finish_sender: Option<oneshot::Sender<Color>>,
    task: Option<JoinHandle<()>>,
    active: Arc<AtomicBool>,
}

async fn update_status_line(progress_reporter: &ProgressReporter, line: &str) {
    let cleaned = vtcode_core::utils::ansi_parser::strip_ansi(line);
    if !cleaned.trim().is_empty() {
        progress_reporter.set_message(cleaned).await;
    }
}

async fn process_output(
    handle: &InlineHandle,
    progress_reporter: &ProgressReporter,
    state: &mut PtyStreamState,
    output: String,
    tail_limit: usize,
    show_live_preview: bool,
) {
    if output.is_empty() {
        return;
    }

    state.apply_chunk(&output, tail_limit);

    // Compact mode suppresses the multi-line transcript block to avoid
    // jump-through replacements, but still streams the latest line through
    // the spinner/status line so `cargo run/test` style long commands stay
    // visibly live. This keeps the complete output in the spool/`Ctrl+T`
    // viewer while giving real-time feedback in the default display mode.
    if !show_live_preview {
        if let Some(last_line) = state.last_display_line(tail_limit) {
            update_status_line(progress_reporter, &last_line).await;
        }
        return;
    }

    let visible_output = vtcode_core::utils::ansi_parser::strip_ansi(&output);
    if visible_output.trim().is_empty() {
        return;
    }

    let (replace_count, segments, link_ranges, last_line) = state.render_current_segments(tail_limit);
    if !segments.is_empty() {
        handle.replace_last_with_links(replace_count, InlineMessageKind::Pty, segments, link_ranges);
    }

    if let Some(last_line) = last_line {
        update_status_line(progress_reporter, &last_line).await;
    }
}

impl PtyStreamRuntime {
    /// Live rows kept visible while a command streams. Capped at 10 so live
    /// previews stay within the same display budget as the 10-row
    /// exec-session completed bodies; the full capture stays in `Ctrl+T`.
    const MAX_LIVE_STREAM_LINES: usize = 10;

    pub(crate) fn start(
        handle: InlineHandle,
        progress_reporter: ProgressReporter,
        tail_limit: usize,
        command_prompt: Option<String>,
        pty_config: PtyConfig,
        workspace_root: Option<&Path>,
        show_live_preview: bool,
    ) -> (Self, ToolProgressCallback) {
        let owned_root = workspace_root.map(Path::to_path_buf);
        let (tx, mut rx) = mpsc::channel::<PtyStreamMessage>(256);
        let (finish_tx, mut finish_rx) = oneshot::channel::<Color>();
        let active = Arc::new(AtomicBool::new(true));
        let worker_active = Arc::clone(&active);
        let drop_state = Arc::new(LivePreviewDropState::new());
        let worker_drop_state = Arc::clone(&drop_state);
        let effective_tail_limit = tail_limit.clamp(1, Self::MAX_LIVE_STREAM_LINES);

        let task = tokio::spawn(async move {
            let mut state = PtyStreamState::new(command_prompt, pty_config, owned_root.as_deref());
            if show_live_preview {
                let (replace_count, segments, link_ranges, _) = state.render_segments("", effective_tail_limit);
                if !segments.is_empty() && worker_active.load(Ordering::Relaxed) {
                    handle.replace_last_with_links(replace_count, InlineMessageKind::Pty, segments, link_ranges);
                }
            }

            let mut finish_requested = None;
            loop {
                if !worker_active.load(Ordering::Relaxed) {
                    break;
                }

                if let Some(final_color) = finish_requested.take() {
                    // The status channel is deliberately separate from output,
                    // so a busy stream cannot strand the final status behind a
                    // full output queue. Drain output accepted before shutdown
                    // before applying the final color to the complete block.
                    while let Ok(message) = rx.try_recv() {
                        let PtyStreamMessage::Output(output) = message;
                        if let Some(coalesced) = worker_drop_state.take_pending() {
                            process_output(
                                &handle,
                                &progress_reporter,
                                &mut state,
                                coalesced,
                                effective_tail_limit,
                                show_live_preview,
                            )
                            .await;
                        }
                        process_output(
                            &handle,
                            &progress_reporter,
                            &mut state,
                            output,
                            effective_tail_limit,
                            show_live_preview,
                        )
                        .await;
                    }
                    if let Some(coalesced) = worker_drop_state.take_pending() {
                        process_output(
                            &handle,
                            &progress_reporter,
                            &mut state,
                            coalesced,
                            effective_tail_limit,
                            show_live_preview,
                        )
                        .await;
                    }

                    if show_live_preview {
                        state.set_header_color(final_color);
                        let (replace_count, segments, link_ranges, _) =
                            state.render_current_segments(effective_tail_limit);
                        if !segments.is_empty() {
                            handle.replace_last_with_links(
                                replace_count,
                                InlineMessageKind::Pty,
                                segments,
                                link_ranges,
                            );
                        }
                    }
                    break;
                }

                tokio::select! {
                    biased;
                    result = &mut finish_rx => {
                        let Ok(color) = result else {
                            break;
                        };
                        finish_requested = Some(color);
                    }
                    message = rx.recv() => {
                        match message {
                            Some(PtyStreamMessage::Output(output)) => {
                                if let Some(coalesced) = worker_drop_state.take_pending() {
                                    process_output(
                                        &handle,
                                        &progress_reporter,
                                        &mut state,
                                        coalesced,
                                        effective_tail_limit,
                                        show_live_preview,
                                    )
                                    .await;
                                }
                                process_output(
                                    &handle,
                                    &progress_reporter,
                                    &mut state,
                                    output,
                                    effective_tail_limit,
                                    show_live_preview,
                                )
                                .await;
                            }
                            None => break,
                        }
                    }
                }
            }
        });

        let callback_active = Arc::clone(&active);
        let callback_tx = tx.clone();
        let callback_drop_state = Arc::clone(&drop_state);
        let callback: ToolProgressCallback = Arc::new(move |_name: &str, output: &str| {
            if !callback_active.load(Ordering::Relaxed) || output.is_empty() {
                return;
            }
            match callback_tx.try_send(PtyStreamMessage::Output(output.to_string())) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(PtyStreamMessage::Output(output))) => {
                    callback_drop_state.record(&output);
                }
                Err(mpsc::error::TrySendError::Closed(PtyStreamMessage::Output(output))) => {
                    callback_drop_state.record(&output);
                }
            }
        });

        (
            Self {
                sender: Some(tx),
                finish_sender: Some(finish_tx),
                task: Some(task),
                active,
            },
            callback,
        )
    }

    pub(crate) async fn shutdown(mut self, header_color: Color) {
        let _ = self.sender.take();
        if let Some(finish_sender) = self.finish_sender.take() {
            let _ = finish_sender.send(header_color);
        }
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
        self.active.store(false, Ordering::Relaxed);
    }

    #[cfg(test)]
    pub(crate) fn for_test(task: JoinHandle<()>, active: Arc<AtomicBool>) -> Self {
        Self {
            sender: None,
            finish_sender: None,
            task: Some(task),
            active,
        }
    }
}

impl Drop for PtyStreamRuntime {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Relaxed);
        let _ = self.sender.take();
        let _ = self.finish_sender.take();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LivePreviewDropState;

    #[test]
    fn live_preview_drop_state_coalesces_and_reports_pressure() {
        let state = LivePreviewDropState::new();
        state.record("first\n");
        state.record("second\n");

        let pending = state.take_pending().expect("coalesced preview");
        assert!(pending.contains("live preview coalesced 2 chunks / 13 bytes"));
        assert!(pending.contains("first\nsecond\n"));
        assert!(state.take_pending().is_none());
    }
}
