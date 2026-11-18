//! Compact command-group rendering.

use super::*;

use crate::utils::transcript;

use anyhow::Result;
use vtcode_commons::formatting::{RAN_COMMAND_CONTINUATION_WIDTH, RAN_COMMAND_FIRST_WIDTH};
use vtcode_commons::tool_headlines::SessionPollKey;
use vtcode_commons::tool_types::CompactStr;
use vtcode_commons::ui_protocol::{CompactActivityMetadata, ToolOutputId};

impl AnsiRenderer {
    ///
    /// The grouping state is intentionally kept in the renderer rather than
    /// the persisted execution event stream. Callers use this before a turn
    /// ends or before rendering a non-command result.
    ///
    /// This deliberately does not end the session poll run: plain body and
    /// hint lines flush through here, and a run must survive its own bodies.
    /// Poll runs end only at explicit boundaries (other headers, turn ends,
    /// failures) via [`AnsiRenderer::end_session_poll_run`].
    pub fn flush_compact_command_group(&mut self) {
        self.compact_command_group = None;
    }

    /// Whether `key` continues the active session poll/wait run.
    pub fn session_poll_continues(&self, key: &SessionPollKey) -> bool {
        self.session_poll_run.as_ref() == Some(key)
    }

    /// Start (or restart) a session poll/wait run after rendering its header.
    pub fn start_session_poll_run(&mut self, key: SessionPollKey, waited_secs: f64) {
        self.session_poll_run = Some(key);
        self.poll_run_attempts = 1;
        self.poll_run_waited_secs = waited_secs.max(0.0);
    }

    /// Record a follow-up within the active run, returning its progress tick
    /// (`×3 · waited 12s`) for the compact continuation line.
    pub fn note_session_poll_followup(&mut self, waited_secs: f64) -> Option<String> {
        self.session_poll_run.as_ref()?;
        self.poll_run_attempts = self.poll_run_attempts.saturating_add(1);
        self.poll_run_waited_secs = (self.poll_run_waited_secs + waited_secs.max(0.0)).max(0.0);
        Some(vtcode_commons::tool_headlines::poll_run_tick(self.poll_run_attempts, self.poll_run_waited_secs))
    }

    /// End the active session poll/wait run, if any. Called at explicit
    /// boundaries only: a non-matching header, a command activity row, a
    /// failure, a turn end, or a cleared screen. Body, hint, and detail lines
    /// of the run itself must never end it.
    pub fn end_session_poll_run(&mut self) {
        self.session_poll_run = None;
        self.poll_run_attempts = 0;
        self.poll_run_waited_secs = 0.0;
        self.last_poll_hint = None;
    }

    /// Suppress an identical consecutive follow-up hint within the active run.
    ///
    /// Returns true when `hint` was already shown for this run and must be
    /// skipped; otherwise records it and returns false. Outside a run every
    /// hint renders.
    pub fn suppress_repeated_poll_hint(&mut self, hint: &str) -> bool {
        if self.session_poll_run.is_none() {
            return false;
        }
        if self.last_poll_hint.as_deref() == Some(hint) {
            return true;
        }
        self.last_poll_hint = Some(hint.to_owned());
        false
    }

    fn next_compact_activity(
        &mut self,
        command: String,
        hidden_line_count: usize,
        suffix: Option<String>,
        review_anchor: Option<ToolOutputId>,
    ) -> (CompactActivityMetadata, bool) {
        if let Some(group) = &mut self.compact_command_group {
            group.command_count = group.command_count.saturating_add(1);
            group.command = None;
            group.hidden_line_count = group.hidden_line_count.saturating_add(hidden_line_count);
            group.suffix = None;
            if let Some(review_anchor) = review_anchor {
                if group.review_anchor.is_none() {
                    group.review_anchor = Some(review_anchor);
                }
                if !group.review_anchors.contains(&review_anchor) {
                    group.review_anchors.push(review_anchor);
                }
            }
            return (group.clone(), true);
        }

        let group_id = self.next_compact_group_id;
        self.next_compact_group_id = self.next_compact_group_id.wrapping_add(1);
        let activity = CompactActivityMetadata {
            group_id,
            command_count: 1,
            command: Some(command.into()),
            hidden_line_count,
            suffix: suffix.map(CompactStr::from),
            review_anchor,
            review_anchors: review_anchor.into_iter().collect(),
        };
        self.compact_command_group = Some(activity.clone());
        (activity, false)
    }

    /// Render one compact successful command activity row, coalescing it with
    /// the immediately preceding successful command row when possible.
    pub fn render_compact_command_activity(
        &mut self,
        command: impl Into<String>,
        hidden_line_count: usize,
        suffix: Option<String>,
        review_anchor: Option<ToolOutputId>,
    ) -> Result<()> {
        // A command row is never a session follow-up: it ends any poll run.
        self.end_session_poll_run();
        let command = command.into();
        if !self.supports_inline_ui() {
            // Plain-text fallback (no inline UI): wrap long commands with
            // explicit `\` continuations and `│` gutters so a chained
            // `git add … && git commit …` stays readable instead of terminal
            // word-wrap without a continuation marker.
            let wrapped = vtcode_commons::formatting::wrap_shell_command_with_continuations(
                &command,
                RAN_COMMAND_FIRST_WIDTH,
                RAN_COMMAND_CONTINUATION_WIDTH,
            );
            if wrapped.is_empty() {
                return self.line(MessageStyle::Info, "• Ran command");
            }
            self.line(MessageStyle::Info, &format!("• Ran {}", wrapped[0]))?;
            for segment in wrapped.iter().skip(1) {
                self.line(MessageStyle::Info, &format!("  │ {segment}"))?;
            }
            return Ok(());
        }

        let (activity, replaces_previous) =
            self.next_compact_activity(command, hidden_line_count, suffix, review_anchor);
        let text = activity.display_text();
        if let Some(sink) = &self.sink {
            if replaces_previous {
                sink.handle.replace_compact_activity(activity);
                transcript::replace_last(1, std::slice::from_ref(&text));
            } else {
                sink.handle.append_compact_activity(activity);
                transcript::append(&text);
            }
        }
        self.last_line_was_empty = false;
        Ok(())
    }

    /// Collapse the live PTY preview into a compact activity row after the
    /// command completes. The complete capture is recorded separately.
    pub fn collapse_pty_block_to_compact_activity(
        &mut self,
        command: impl Into<String>,
        hidden_line_count: usize,
        suffix: Option<String>,
        review_anchor: Option<ToolOutputId>,
    ) -> Result<()> {
        // A command row is never a session follow-up: it ends any poll run.
        self.end_session_poll_run();
        if !self.supports_inline_ui() {
            return Ok(());
        }

        let (activity, replaces_previous) =
            self.next_compact_activity(command.into(), hidden_line_count, suffix, review_anchor);
        let text = activity.display_text();
        if let Some(sink) = &self.sink {
            sink.handle.collapse_pty_block(activity);
            if replaces_previous {
                transcript::replace_last(1, std::slice::from_ref(&text));
            } else {
                transcript::append(&text);
            }
        }
        self.last_line_was_empty = false;
        Ok(())
    }
}
