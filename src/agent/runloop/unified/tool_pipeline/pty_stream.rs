use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use vtcode_core::tools::registry::ToolProgressCallback;
use vtcode_core::ui::tui::{InlineHandle, InlineMessageKind, InlineSegment, InlineTextStyle};

use crate::agent::runloop::unified::progress::ProgressReporter;

pub(super) struct PtyStreamState {
    command_prompt: Option<String>,
    lines: VecDeque<String>,
    current_line: String,
    displayed_count: usize,
    total_lines: usize,
    last_pushed_line: Option<String>,
}

impl PtyStreamState {
    pub(super) fn new(command_prompt: Option<String>) -> Self {
        Self {
            command_prompt: normalize_command_prompt(command_prompt)
                .map(|command| format_command_prompt(&command)),
            lines: VecDeque::new(),
            current_line: String::new(),
            displayed_count: 0,
            total_lines: 0,
            last_pushed_line: None,
        }
    }

    pub(super) fn apply_chunk(&mut self, chunk: &str, limit: usize) {
        if limit == 0 {
            self.lines.clear();
            self.current_line.clear();
            self.displayed_count = 0;
            self.total_lines = 0;
            self.last_pushed_line = None;
            return;
        }

        let mut chars = chunk.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\r' => {
                    if matches!(chars.peek(), Some('\n')) {
                        let _ = chars.next();
                        self.push_line(limit);
                    } else {
                        self.current_line.clear();
                    }
                }
                '\n' => self.push_line(limit),
                _ => self.current_line.push(ch),
            }
        }
    }

    fn push_line(&mut self, limit: usize) {
        let trimmed = self.current_line.trim_end();
        if trimmed.trim().is_empty() {
            self.current_line.clear();
            return;
        }

        if self
            .last_pushed_line
            .as_ref()
            .is_some_and(|previous| previous == trimmed)
        {
            self.current_line.clear();
            return;
        }

        let line = trimmed.to_string();
        self.last_pushed_line = Some(line.clone());
        self.lines.push_back(line);
        self.total_lines += 1;
        self.current_line.clear();
        while self.lines.len() > limit {
            let _ = self.lines.pop_front();
        }
    }

    fn render_lines(&self, limit: usize) -> Vec<String> {
        let mut rendered = Vec::new();
        if let Some(prompt) = self.command_prompt.as_ref() {
            rendered.push(prompt.clone());
        }
        if limit == 0 {
            return rendered;
        }

        let has_current = !self.current_line.trim().is_empty();
        let total = self.total_lines + usize::from(has_current);
        if total == 0 {
            return rendered;
        }

        let mut truncated = false;
        let mut tail_limit = limit;
        let mut hidden_lines = 0usize;
        if total > limit {
            truncated = true;
            tail_limit = tail_limit.saturating_sub(1);
            hidden_lines = total.saturating_sub(tail_limit);
        }

        let start = total.saturating_sub(tail_limit);
        let base_index = self.total_lines.saturating_sub(self.lines.len());
        if truncated {
            rendered.push(format_hidden_lines_summary(hidden_lines));
        }

        for (idx, line) in self.lines.iter().enumerate() {
            let absolute = base_index + idx;
            if absolute >= start {
                rendered.push(line.clone());
            }
        }

        let current_index = base_index + self.lines.len();
        if has_current && current_index >= start {
            rendered.push(self.current_line.trim_end().to_string());
        }

        rendered
    }

    fn last_display_line(&self) -> Option<String> {
        if !self.current_line.trim().is_empty() {
            return Some(self.current_line.trim_end().to_string());
        }
        self.lines.back().cloned()
    }

    pub(super) fn render_segments(
        &mut self,
        chunk: &str,
        tail_limit: usize,
    ) -> (usize, Vec<Vec<InlineSegment>>, Option<String>) {
        self.apply_chunk(chunk, tail_limit);
        let rendered = self.render_lines(tail_limit);
        let style = Arc::new(InlineTextStyle::default());
        let segments = rendered
            .into_iter()
            .map(|line| {
                vec![InlineSegment {
                    text: line,
                    style: Arc::clone(&style),
                }]
            })
            .collect::<Vec<_>>();
        let replace_count = self.displayed_count;
        self.displayed_count = segments.len();
        let last_line = self.last_display_line();
        (replace_count, segments, last_line)
    }
}

fn format_hidden_lines_summary(hidden: usize) -> String {
    if hidden == 1 {
        "… +1 line".to_string()
    } else {
        format!("… +{} lines", hidden)
    }
}

fn normalize_command_prompt(command_prompt: Option<String>) -> Option<String> {
    command_prompt.and_then(|value| {
        let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed.is_empty() {
            None
        } else {
            Some(collapsed)
        }
    })
}

fn format_command_prompt(command: &str) -> String {
    let trimmed = command.trim_start();
    if trimmed.starts_with('$') {
        trimmed.to_string()
    } else {
        format!("$ {}", command)
    }
}

pub(super) struct PtyStreamRuntime {
    sender: Option<mpsc::UnboundedSender<String>>,
    task: Option<JoinHandle<()>>,
    active: Arc<AtomicBool>,
}

impl PtyStreamRuntime {
    const MAX_LIVE_STREAM_LINES: usize = 12;

    pub(super) fn start(
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
            let (replace_count, segments, _) = state.render_segments("", effective_tail_limit);
            if !segments.is_empty() && worker_active.load(Ordering::Relaxed) {
                handle.replace_last(replace_count, InlineMessageKind::Pty, segments);
            }

            while let Some(output) = rx.recv().await {
                if !worker_active.load(Ordering::Relaxed) {
                    break;
                }
                if output.is_empty() {
                    continue;
                }

                let cleaned_output = vtcode_core::utils::ansi_parser::strip_ansi(&output);
                if cleaned_output.is_empty() {
                    continue;
                }

                let (replace_count, segments, last_line) =
                    state.render_segments(&cleaned_output, effective_tail_limit);
                if !segments.is_empty() && worker_active.load(Ordering::Relaxed) {
                    handle.replace_last(replace_count, InlineMessageKind::Pty, segments);
                }

                if let Some(last_line) = last_line {
                    progress_reporter.set_message(last_line).await;
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

    pub(super) async fn shutdown(mut self) {
        self.active.store(false, Ordering::Relaxed);
        let _ = self.sender.take();
        if let Some(task) = self.task.take() {
            let _ = tokio::time::timeout(Duration::from_millis(250), task).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pty_stream_state_streams_incremental_chunks() {
        let mut state = PtyStreamState::new(None);
        state.apply_chunk("line1\nline2", 5);
        let rendered = state.render_lines(5);
        assert_eq!(rendered, vec!["line1".to_string(), "line2".to_string()]);
        assert_eq!(
            state.last_display_line(),
            Some("line2".to_string()),
            "expected partial line to be tracked"
        );
    }

    #[test]
    fn pty_stream_state_handles_carriage_return_overwrite() {
        let mut state = PtyStreamState::new(None);
        state.apply_chunk("start\rreplace\n", 5);
        let rendered = state.render_lines(5);
        assert_eq!(rendered, vec!["replace".to_string()]);
        assert_eq!(
            state.last_display_line(),
            Some("replace".to_string()),
            "expected overwritten line to be retained"
        );
    }

    #[test]
    fn pty_stream_state_applies_tail_truncation() {
        let mut state = PtyStreamState::new(None);
        state.apply_chunk("a\nb\nc\nd\n", 3);
        let rendered = state.render_lines(3);
        assert_eq!(
            rendered,
            vec!["… +2 lines".to_string(), "c".to_string(), "d".to_string()]
        );
    }

    #[test]
    fn pty_stream_state_formats_hidden_line_summary() {
        let mut state = PtyStreamState::new(None);
        state.apply_chunk("a\nb\nc\n", 2);
        let rendered = state.render_lines(2);
        assert_eq!(rendered, vec!["… +2 lines".to_string(), "c".to_string()]);
    }

    #[test]
    fn pty_stream_state_deduplicates_consecutive_lines() {
        let mut state = PtyStreamState::new(None);
        state.apply_chunk("same\nsame\nnext\n", 5);
        let rendered = state.render_lines(5);
        assert_eq!(rendered, vec!["same".to_string(), "next".to_string()]);
    }

    #[test]
    fn pty_stream_state_renders_command_prompt_without_output() {
        let state = PtyStreamState::new(Some("cargo check".to_string()));
        let rendered = state.render_lines(5);
        assert_eq!(rendered, vec!["$ cargo check".to_string()]);
    }

    #[test]
    fn pty_stream_state_keeps_command_prompt_with_truncated_tail() {
        let mut state = PtyStreamState::new(Some("cargo check".to_string()));
        state.apply_chunk("a\nb\nc\n", 2);
        let rendered = state.render_lines(2);
        assert_eq!(
            rendered,
            vec![
                "$ cargo check".to_string(),
                "… +2 lines".to_string(),
                "c".to_string()
            ]
        );
    }

    #[test]
    fn normalizes_command_prompt_whitespace() {
        let state = PtyStreamState::new(Some("  cargo   check \n -p  vtcode  ".to_string()));
        let rendered = state.render_lines(5);
        assert_eq!(rendered, vec!["$ cargo check -p vtcode".to_string()]);
    }
}
