//! Data exchanged by compact per-call tool summaries.

use crate::tool_types::CompactStr;

/// Session-local key for a complete tool-output capture.
///
/// This is deliberately a UI protocol identifier. It is not persisted in
/// `ThreadEvent` data and must not be treated as a tool-call identity outside
/// the live terminal session.
pub type ToolOutputId = u64;

/// Presentation metadata for a compact command activity row.
///
/// The complete command output is sent through `RecordToolOutput` separately.
/// Keeping this small metadata object independent means compact rendering can
/// replace a row without truncating, reordering, or otherwise changing the
/// captured stdout, stderr, PTY, or spool-backed transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactActivityMetadata {
    /// Identifier shared by all rows in one contiguous successful-command group.
    pub group_id: u64,
    /// Number of successful commands represented by the row.
    pub command_count: usize,
    /// First-command headline, retained for compatibility. Rows render the
    /// count form for one or many commands.
    pub command: Option<CompactStr>,
    /// Number of complete output lines hidden behind the review affordance.
    pub hidden_line_count: usize,
    /// Optional status or artifact text that remains visible in the row.
    pub suffix: Option<CompactStr>,
    /// First complete capture represented by the row, used as the review focus.
    pub review_anchor: Option<ToolOutputId>,
    /// All complete captures represented by the row, in render order.
    ///
    /// `review_anchor` remains the first capture for compatibility with the
    /// click protocol; this list lets the UI re-anchor every member of a
    /// grouped row without guessing from transcript text.
    pub review_anchors: Vec<ToolOutputId>,
}

impl CompactActivityMetadata {
    /// Return the compact row text without the UI-only review affordance.
    ///
    /// One count form for single and grouped rows alike; the full command
    /// stays in the review capture behind the row.
    pub fn display_text(&self) -> String {
        let noun = if self.command_count == 1 { "command" } else { "commands" };
        let mut text = format!("• Ran {} {noun}", self.command_count);

        if self.hidden_line_count > 0 {
            text.push_str(&format!(" · … +{} lines", self.hidden_line_count));
        }
        if let Some(suffix) = self.suffix.as_deref().filter(|suffix| !suffix.is_empty()) {
            text.push_str(" · ");
            text.push_str(suffix);
        }
        text
    }
}

/// UI-only payload that reopens a completed-edit diff as full-viewport review.
///
/// Not part of `ThreadEvent`. The transcript notice text is the activation
/// target; `unified` is the retained preview used when before/after are absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffReviewAnchor {
    /// Workspace-visible file path for the review header.
    pub file_path: String,
    /// Retained unified preview body.
    pub unified: String,
    /// Rows omitted from the compact transcript body, when known.
    pub omitted_lines: u64,
    /// Exact notice text that should activate this review.
    pub notice: String,
}

/// Generic fallback labels that must never win path-based notice matching.
#[must_use]
pub fn is_generic_diff_review_path(path: &str) -> bool {
    path.is_empty() || path == "diff" || path == "file" || path.starts_with("diff.")
}

/// Shared expand-notice copy for clipped completed-edit diff bodies.
///
/// `safety_capped` is retained so callers can distinguish a width-cap clip
/// from vertical omission without changing the user-facing wording.
#[must_use]
pub fn diff_review_notice(file_path: &str, omitted_lines: u64, safety_capped: bool) -> String {
    let _ = safety_capped;
    if omitted_lines > 0 {
        format!("… +{omitted_lines} lines — review full diff for {file_path}")
    } else {
        format!("… diff truncated — review full diff for {file_path}")
    }
}

#[cfg(test)]
mod tests {
    use super::CompactActivityMetadata;

    #[test]
    fn compact_activity_display_uses_count_form_for_single_command() {
        let activity = CompactActivityMetadata {
            group_id: 1,
            command_count: 1,
            command: Some("cargo check".into()),
            hidden_line_count: 3,
            suffix: None,
            review_anchor: Some(7),
            review_anchors: vec![7],
        };

        assert_eq!(activity.display_text(), "• Ran 1 command · … +3 lines");
    }

    #[test]
    fn compact_activity_display_collapses_grouped_commands() {
        let activity = CompactActivityMetadata {
            group_id: 2,
            command_count: 4,
            command: None,
            hidden_line_count: 12,
            suffix: Some("output retained".into()),
            review_anchor: Some(9),
            review_anchors: vec![9],
        };

        assert_eq!(activity.display_text(), "• Ran 4 commands · … +12 lines · output retained");
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactToolSummaryLine {
    pub kind: CompactToolSummaryLineKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactToolSummaryLineKind {
    Info,
    Detail,
}
