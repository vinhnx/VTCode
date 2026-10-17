//! Compact command-group rendering.

use super::*;

use crate::utils::transcript;

use anyhow::Result;
use vtcode_commons::formatting::{RAN_COMMAND_CONTINUATION_WIDTH, RAN_COMMAND_FIRST_WIDTH};
use vtcode_commons::tool_types::CompactStr;
use vtcode_commons::ui_protocol::{CompactActivityMetadata, ToolOutputId};

impl AnsiRenderer {
    ///
    /// The grouping state is intentionally kept in the renderer rather than
    /// the persisted execution event stream. Callers use this before a turn
    /// ends or before rendering a non-command result.
    pub fn flush_compact_command_group(&mut self) {
        self.compact_command_group = None;
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
