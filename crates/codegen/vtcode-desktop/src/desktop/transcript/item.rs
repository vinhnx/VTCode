//! Transcript row type: one rendered row plus its prepared render projections.

use gpui_kit::SharedString;

use super::super::markdown::prepare_markdown;
use super::activity_verbs::ToolCategory;
use super::user_attachments::UserAttachment;
use super::{
    ActivityGroupSummary, AppPreviewItem, DiffCard, NoticeSeverity, ToolActivityStatus, TranscriptRole, TurnFile,
};

/// Per-call added/deleted line counts stamped onto an edit row. `Copy` so
/// re-projections can read it through a row borrow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EditTotals {
    pub additions: u64,
    pub deletions: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct TranscriptItem {
    pub id: String,
    pub role: String,
    pub notice_severity: NoticeSeverity,
    pub title: SharedString,
    pub text: SharedString,
    pub markdown: SharedString,
    pub accessibility_text: SharedString,
    pub completed_at: Option<SharedString>,
    pub turn_id: Option<SharedString>,
    pub canonical_turn_id: Option<SharedString>,
    pub canonical_item_id: Option<SharedString>,
    pub archived_final: bool,
    pub archived_phase: Option<SharedString>,
    pub turn_group_head: bool,
    pub turn_grouped: bool,
    pub turn_group_expanded: bool,
    pub turn_files: Vec<TurnFile>,
    pub turn_file_totals: Option<(u64, u64)>,
    pub turn_files_incomplete: bool,
    pub turn_files_offset: usize,
    /// Whether the prepared Markdown contains a list. Lists need a definite
    /// bubble width (see `PreparedMarkdown::has_list`); kept beside the other
    /// render projections so every prepare/clone path carries it together.
    pub has_list: bool,
    pub expanded: bool,
    /// Whether this notice row carries collapsible operational details behind
    /// a Show/Hide toggle. Plain notices render their full text instead.
    pub collapsible_details: bool,
    /// Shared expansion state copied to each row so virtualized rendering stays constant-time.
    pub activity_expanded: bool,
    /// Whether this tool row is part of a consecutive group of at least two rows.
    pub activity_grouped: bool,
    /// Whether this row owns the visible header for its tool group.
    pub activity_group_head: bool,
    /// Status totals kept on the group head so streamed updates do not rescan every tool row.
    pub activity_group_summary: ActivityGroupSummary,
    /// Verb category for the group summary, set at projection from the live
    /// event details. `None` for rows that never pass through projection;
    /// those groups keep the legacy count label.
    pub activity_category: Option<ToolCategory>,
    /// Whether this row carries structured activity status rather than archive text.
    pub has_activity_status: bool,
    /// Per-call added/deleted line counts for an edit row, stamped from its
    /// `{call_id}:changes` sibling when the FileChange event lands. `None`
    /// until then (or when the emitter had no counts); doubles as the
    /// already-stamped guard so replays never double-suffix the title.
    pub edit_totals: Option<EditTotals>,
    /// Raw tool name for an invocation row (`exec_command`), kept verbatim so
    /// search matches snake_case queries the prettified title cannot. `None`
    /// for rows without an invoking tool (messages, executions, diffs).
    pub tool_name: Option<SharedString>,
    pub diff: Option<DiffCard>,
    /// Raw aggregated command/tool output for the Output inspector tab.
    pub output: Option<SharedString>,
    /// Canonical output item identity when capture is attached to an invocation.
    pub output_item_id: Option<String>,
    /// Earlier live output was discarded from the bounded preview.
    pub output_truncated: bool,
    /// MCP Apps referenced by this output, for preview cards. Empty for non-app results.
    pub app_previews: Vec<AppPreviewItem>,
    /// Structured attachments for user echo rows; history rows and other roles leave this empty.
    pub user_attachments: Vec<UserAttachment>,
}

impl TranscriptItem {
    fn has_role(&self, role: TranscriptRole) -> bool {
        self.role == role.as_str()
    }

    pub(crate) fn is_user(&self) -> bool {
        self.has_role(TranscriptRole::User)
    }

    pub(crate) fn is_assistant(&self) -> bool {
        self.has_role(TranscriptRole::Assistant)
    }

    pub(crate) fn is_tool(&self) -> bool {
        self.has_role(TranscriptRole::Tool)
    }

    pub(crate) fn is_diff(&self) -> bool {
        self.has_role(TranscriptRole::Diff)
    }

    pub(crate) fn is_notice(&self) -> bool {
        self.has_role(TranscriptRole::Notice)
    }

    /// Whether this row offers searchable conversation content: a searchable
    /// role with non-empty body. Shared by the palette search list, its
    /// availability check, and the filter-match snapshot.
    pub(crate) fn is_conversation_searchable(&self) -> bool {
        TranscriptRole::from_str(&self.role).is_some_and(TranscriptRole::is_searchable_conversation)
            && !self.text.trim().is_empty()
    }

    /// Whether this row carries captured output worth jumping to. Shared by
    /// the activity projection, the transcript output CTA, and conversation
    /// search so the affordance can never disagree across surfaces.
    pub(crate) fn has_output(&self) -> bool {
        self.output.is_some()
    }

    /// Notice row (runtime errors, compaction/reset notices, run status) with
    /// its severity set at construction.
    pub fn notice(id: String, title: SharedString, text: String, severity: NoticeSeverity) -> Self {
        let mut item = Self::new(id, TranscriptRole::Notice.as_str().to_string(), title, text);
        item.notice_severity = severity;
        item
    }

    /// Diagnostic notice from a `ThreadEvent` reasoning payload: a
    /// severity-styled summary with collapsible operational details. Empty
    /// evidence offers no toggle, mirroring the terminal transcript.
    pub fn diagnostic_notice(id: String, title: SharedString, details: String, severity: NoticeSeverity) -> Self {
        let collapsible = !details.trim().is_empty();
        let mut item = Self::notice(id, title, details, severity);
        item.collapsible_details = collapsible;
        item
    }

    pub fn new(id: String, role: String, title: SharedString, text: String) -> Self {
        let text = SharedString::from(text);
        let is_tool = role == TranscriptRole::Tool.as_str();
        let activity_group_summary = if is_tool {
            ActivityGroupSummary::for_status(ToolActivityStatus::from_text(&text))
        } else {
            ActivityGroupSummary::default()
        };
        Self {
            id,
            has_activity_status: is_tool,
            role,
            notice_severity: NoticeSeverity::Info,
            title,
            text,
            markdown: SharedString::default(),
            accessibility_text: SharedString::default(),
            completed_at: None,
            turn_id: None,
            canonical_turn_id: None,
            canonical_item_id: None,
            archived_final: false,
            archived_phase: None,
            turn_group_head: false,
            turn_grouped: false,
            turn_group_expanded: false,
            turn_files: Vec::new(),
            turn_file_totals: None,
            turn_files_incomplete: false,
            turn_files_offset: 0,
            has_list: false,
            expanded: false,
            collapsible_details: false,
            activity_expanded: false,
            activity_grouped: false,
            activity_group_head: false,
            activity_group_summary,
            activity_category: None,
            edit_totals: None,
            tool_name: None,
            diff: None,
            output: None,
            output_item_id: None,
            output_truncated: false,
            app_previews: Vec::new(),
            user_attachments: Vec::new(),
        }
    }

    pub(crate) fn prepare_render_content(&mut self) {
        if self.is_tool() || self.is_diff() {
            self.markdown = SharedString::default();
            self.accessibility_text = SharedString::default();
            self.has_list = false;
            return;
        }
        // Notices render as plain severity text: error bodies must not
        // reinterpret backticks or brackets as Markdown.
        if self.is_notice() {
            self.markdown = SharedString::default();
            self.accessibility_text.clone_from(&self.text);
            self.has_list = false;
            return;
        }

        let prepared = prepare_markdown(&self.text);
        self.markdown = prepared.markdown;
        self.accessibility_text = prepared.accessible_text;
        self.append_attachment_accessibility_suffix();
        self.has_list = prepared.has_list;
    }
}
