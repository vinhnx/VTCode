mod turn;
use std::collections::VecDeque;
use std::ops::Range;

use gpui_kit::SharedString;
use vtcode_exec_events::AppPreviewItem;

use super::markdown::prepare_markdown;

mod activity;
mod activity_verbs;
mod output;
mod tool_summary;
mod user_attachments;
pub(super) use activity_verbs::{
    ToolCategory, ToolCategoryCounts, describe_activity_verbs, harness_is_user_visible, tool_row_gist,
};
pub(super) use tool_summary::{activity_details_summary, activity_output_text, activity_payload, activity_text};
pub(super) use user_attachments::UserAttachment;

pub(super) const MAX_TRANSCRIPT_ITEMS: usize = 2_000;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum TranscriptLayoutChange {
    Reset(usize),
    Prepend(usize),
    Splice { range: Range<usize>, count: usize },
    Remeasure,
    RemeasureItems(Range<usize>),
    PauseFollowingTail,
    FollowLatest,
}

/// Severity for `notice` rows (runtime errors, compaction/reset notices,
/// run status). Other roles leave the default untouched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum NoticeSeverity {
    #[default]
    Info,
    Warning,
    Error,
}

/// Canonical activity-status labels.
///
/// Producers (the view's `describe_activity` family) write these as the first
/// line of tool-row text; [`ActivityState::from_status`] is the single
/// parser. Keep both sides on these consts: a label renamed in one place and
/// not the other misclassifies silently.
pub(super) mod status {
    pub const RUNNING: &str = "running";
    pub const IN_PROGRESS: &str = "in progress";
    pub const STARTED: &str = "started";
    pub const COMPLETED: &str = "completed";
    pub const FAILED: &str = "failed";
    /// Prefix (with trailing space) of exit-code failures such as `failed (exit 17)`.
    pub const FAILED_EXIT_PREFIX: &str = "failed (exit ";
    pub const INTERRUPTED: &str = "interrupted";
    pub const ACTIVITY: &str = "activity";
}

/// Known transcript-row roles.
///
/// Stored as [`TranscriptItem::role`] strings because history rows arrive
/// from the service; compare via the [`TranscriptItem::is_tool`] family or
/// match on [`TranscriptRole::from_str`] instead of repeating literals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TranscriptRole {
    User,
    Assistant,
    Tool,
    Diff,
    Notice,
    System,
}

impl TranscriptRole {
    const ALL: [Self; 6] = [
        Self::User,
        Self::Assistant,
        Self::Tool,
        Self::Diff,
        Self::Notice,
        Self::System,
    ];

    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
            Self::Diff => "diff",
            Self::Notice => "notice",
            Self::System => "system",
        }
    }

    /// Parse a stored role. Case-sensitive: unknown roles stay unclassified
    /// so new service-side roles keep rendering instead of vanishing.
    pub(super) fn from_str(role: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|known| role == known.as_str())
    }

    /// Roles whose message text is searchable conversation content.
    pub(super) const fn is_searchable_conversation(self) -> bool {
        matches!(self, Self::User | Self::Assistant | Self::Tool)
    }
}

/// Lifecycle classification of a tool-row status line.
///
/// [`from_status`](Self::from_status) is the single parser shared by the
/// transcript group summaries and the inspector projections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ActivityState {
    Running,
    Completed,
    Failed,
    Interrupted,
    Other,
}

impl ActivityState {
    pub(super) fn from_status(label: &str) -> Self {
        match label {
            status::RUNNING | status::IN_PROGRESS | status::STARTED => Self::Running,
            status::COMPLETED => Self::Completed,
            status::INTERRUPTED => Self::Interrupted,
            other if other == status::FAILED || other.starts_with(status::FAILED_EXIT_PREFIX) => Self::Failed,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct TranscriptItem {
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ActivityGroupSummary {
    pub total: u16,
    pub running: u16,
    pub failed: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ToolActivityStatus {
    running: bool,
    failed: bool,
}

impl ActivityGroupSummary {
    fn for_status(status: ToolActivityStatus) -> Self {
        Self {
            total: 1,
            running: u16::from(status.running),
            failed: u16::from(status.failed),
        }
    }

    fn add_status(&mut self, status: ToolActivityStatus) {
        self.total += 1;
        self.running += u16::from(status.running);
        self.failed += u16::from(status.failed);
    }

    fn remove_status(&mut self, status: ToolActivityStatus) {
        debug_assert!(self.total > 0, "a group summary cannot remove a missing tool row");
        debug_assert!(!status.running || self.running > 0, "running group count must include the removed row");
        debug_assert!(!status.failed || self.failed > 0, "failed group count must include the removed row");
        self.total -= 1;
        self.running -= u16::from(status.running);
        self.failed -= u16::from(status.failed);
    }

    fn replace_status(&mut self, previous: ToolActivityStatus, next: ToolActivityStatus) {
        debug_assert!(!previous.running || self.running > 0, "running group count must include the replaced row");
        debug_assert!(!previous.failed || self.failed > 0, "failed group count must include the replaced row");
        self.running = self.running - u16::from(previous.running) + u16::from(next.running);
        self.failed = self.failed - u16::from(previous.failed) + u16::from(next.failed);
    }
}

impl ToolActivityStatus {
    fn from_text(text: &str) -> Self {
        let state = ActivityState::from_status(text.lines().next().unwrap_or_default());
        Self {
            running: state == ActivityState::Running,
            failed: state == ActivityState::Failed,
        }
    }
}

/// Structured payload for a file-change activity row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DiffCard {
    pub files: Vec<DiffFile>,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub incomplete: bool,
    pub failed: bool,
    pub patch: String,
}

/// One file inside a [`DiffCard`], with its change kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DiffFile {
    pub path: String,
    pub kind: DiffFileKind,
}

#[derive(Clone, Debug)]
pub(super) struct TurnFile {
    pub file: DiffFile,
    pub counts: Option<(u64, u64)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DiffFileKind {
    Add,
    Delete,
    Update,
}

impl TranscriptItem {
    fn has_role(&self, role: TranscriptRole) -> bool {
        self.role == role.as_str()
    }

    pub(super) fn is_user(&self) -> bool {
        self.has_role(TranscriptRole::User)
    }

    pub(super) fn is_assistant(&self) -> bool {
        self.has_role(TranscriptRole::Assistant)
    }

    pub(super) fn is_tool(&self) -> bool {
        self.has_role(TranscriptRole::Tool)
    }

    pub(super) fn is_diff(&self) -> bool {
        self.has_role(TranscriptRole::Diff)
    }

    pub(super) fn is_notice(&self) -> bool {
        self.has_role(TranscriptRole::Notice)
    }

    /// Whether this row offers searchable conversation content: a searchable
    /// role with non-empty body. Shared by the palette search list, its
    /// availability check, and the filter-match snapshot.
    pub(super) fn is_conversation_searchable(&self) -> bool {
        TranscriptRole::from_str(&self.role).is_some_and(TranscriptRole::is_searchable_conversation)
            && !self.text.trim().is_empty()
    }

    /// Whether this row carries captured output worth jumping to. Shared by
    /// the activity projection, the transcript output CTA, and conversation
    /// search so the affordance can never disagree across surfaces.
    pub(super) fn has_output(&self) -> bool {
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
            diff: None,
            output: None,
            output_item_id: None,
            output_truncated: false,
            app_previews: Vec::new(),
            user_attachments: Vec::new(),
        }
    }

    pub(super) fn prepare_render_content(&mut self) {
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

/// Owns transcript rows and queues the minimal layout changes for the view.
#[derive(Default)]
pub(super) struct Transcript {
    items: VecDeque<TranscriptItem>,
    layout_changes: Vec<TranscriptLayoutChange>,
    layout_initialized: bool,
    /// Head index of the trailing consecutive tool group, including single-row groups.
    trailing_tool_group_start: Option<usize>,
    /// Bumped on every mutation of `items` so derived projections (inspector
    /// activity/output, palette search) can memoize per revision instead of
    /// rescanning per frame. Layout-queue-only changes (`take_layout_changes`,
    /// `follow_latest`, `remeasure`) do not bump: they never alter rows.
    /// A spurious bump only costs one recompute; a missing bump is a
    /// stale-projection bug, so new `&mut self` methods that touch `items`
    /// must call `bump_revision`.
    revision: u64,
}

impl Transcript {
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn remaining_capacity(&self) -> usize {
        MAX_TRANSCRIPT_ITEMS.saturating_sub(self.items.len())
    }

    /// Current mutation generation of `items`; see the `revision` field docs.
    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    fn bump_revision(&mut self) {
        // Wrapping is deliberate: the counter is a cache key, not a count.
        // A collision needs 2^64 mutations without an intervening cache read.
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn get(&self, index: usize) -> Option<&TranscriptItem> {
        self.items.get(index)
    }

    pub(super) fn position_by_id(&self, id: &str) -> Option<usize> {
        let last_index = self.items.len().checked_sub(1)?;
        if self.items.back().is_some_and(|item| item.id == id) {
            return Some(last_index);
        }
        self.items.iter().rposition(|item| item.id == id)
    }

    /// Rows in chronological order.
    pub fn items(&self) -> impl DoubleEndedIterator<Item = &TranscriptItem> {
        self.items.iter()
    }

    /// Rows newest first, for reverse projections such as the inspector.
    pub fn items_newest_first(&self) -> impl Iterator<Item = &TranscriptItem> {
        self.items().rev()
    }

    pub fn take_layout_changes(&mut self) -> Vec<TranscriptLayoutChange> {
        if !self.layout_initialized {
            self.layout_initialized = true;
            self.layout_changes.clear();
            return vec![TranscriptLayoutChange::Reset(self.items.len())];
        }
        if self
            .layout_changes
            .iter()
            .any(|change| matches!(change, TranscriptLayoutChange::Reset(_)))
        {
            self.layout_changes.clear();
            return vec![TranscriptLayoutChange::Reset(self.items.len())];
        }
        std::mem::take(&mut self.layout_changes)
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.layout_changes.clear();
        self.trailing_tool_group_start = None;
        self.layout_changes.push(TranscriptLayoutChange::Reset(0));
        self.bump_revision();
    }

    pub fn push(&mut self, item: TranscriptItem) {
        self.push_inner(item, true);
    }

    /// Replace the transcript with items whose render projections were prepared off the UI thread.
    pub(super) fn replace_prepared(&mut self, items: Vec<TranscriptItem>) {
        self.clear();
        for item in items {
            self.push_inner(item, false);
        }
    }

    /// Prepend prepared archive rows without resetting the reader's scroll anchor.
    pub(super) fn prepend_prepared(&mut self, items: Vec<TranscriptItem>) -> usize {
        let count = items.len();
        if count == 0 || count > self.remaining_capacity() {
            return 0;
        }

        let previous_first_tool_expanded = self
            .items
            .front()
            .filter(|item| item.is_tool())
            .map(|item| item.activity_expanded);
        let mut prefix = items;
        if let Some(expanded) = previous_first_tool_expanded {
            for item in prefix.iter_mut().rev().take_while(|item| item.is_tool()) {
                item.activity_expanded = expanded;
            }
        }

        let mut merged = VecDeque::with_capacity(self.items.len() + count);
        merged.extend(prefix);
        merged.append(&mut self.items);
        self.items = merged;
        self.bump_revision();
        self.rebuild_activity_group_flags();
        self.refresh_trailing_tool_group_start();
        self.layout_changes.push(TranscriptLayoutChange::Prepend(count));
        let affected_end = count.saturating_add(1).min(self.items.len());
        if affected_end > 0 {
            self.layout_changes
                .push(TranscriptLayoutChange::RemeasureItems(0..affected_end));
        }
        count
    }

    fn push_inner(&mut self, mut item: TranscriptItem, prepare_render_content: bool) {
        if prepare_render_content {
            item.prepare_render_content();
        }
        let item_is_tool = item.is_tool();
        let item_status = ToolActivityStatus::from_text(&item.text);
        if item_is_tool {
            item.activity_group_summary = ActivityGroupSummary::for_status(item_status);
        } else {
            item.activity_group_summary = ActivityGroupSummary::default();
        }
        let extends_tool_group = item_is_tool && self.items.back().is_some_and(|row| row.is_tool());
        let previous_group_start = self.trailing_tool_group_start;
        let evict_oldest = self.items.len() == MAX_TRANSCRIPT_ITEMS;
        let mut promoted_group_head = false;
        if evict_oldest {
            if let Some(removed) = self.items.pop_front() {
                if removed.turn_group_head {
                    self.promote_turn_group_head(removed.turn_id.as_deref());
                }
                if removed.activity_group_head {
                    let mut summary = removed.activity_group_summary;
                    summary.remove_status(ToolActivityStatus::from_text(&removed.text));
                    if let Some(head) = self.items.front_mut().filter(|item| item.is_tool()) {
                        let grouped = summary.total > 1;
                        head.activity_group_summary = summary;
                        head.activity_grouped = grouped;
                        head.activity_group_head = grouped;
                        head.activity_expanded = grouped && removed.activity_expanded;
                        promoted_group_head = true;
                    }
                }
            }
            self.layout_changes
                .push(TranscriptLayoutChange::Splice { range: 0..1, count: 0 });
            if let Some(start) = self.trailing_tool_group_start {
                self.trailing_tool_group_start = Some(start.saturating_sub(1));
            }
        }
        let index = self.items.len();
        debug_assert!(
            !extends_tool_group || previous_group_start.is_some(),
            "the trailing tool-group head stays in sync with transcript rows"
        );
        let group_head = extends_tool_group.then(|| {
            previous_group_start
                .unwrap_or_else(|| index.saturating_sub(1))
                .saturating_sub(usize::from(evict_oldest))
        });
        if let Some(head_index) = group_head
            && let Some(head) = self.items.get_mut(head_index)
        {
            head.activity_group_summary.add_status(item_status);
            head.activity_grouped = true;
            head.activity_group_head = true;
            item.activity_grouped = true;
            item.activity_expanded = head.activity_expanded;
            item.activity_group_summary = ActivityGroupSummary::default();
        }
        self.trailing_tool_group_start = if item_is_tool {
            Some(group_head.unwrap_or(index))
        } else {
            None
        };
        self.items.push_back(item);
        self.bump_revision();
        self.layout_changes
            .push(TranscriptLayoutChange::Splice { range: index..index, count: 1 });
        if promoted_group_head {
            self.layout_changes.push(TranscriptLayoutChange::RemeasureItems(0..1));
        }
        if let Some(head) = group_head {
            // A new tool row changes the shared group header and inserts only
            // one new row. Existing tool rows keep their measured geometry.
            if !promoted_group_head || head != 0 {
                self.layout_changes.push(TranscriptLayoutChange::RemeasureItems(head..head + 1));
            }
        }
    }

    /// Remove a local unsent echo together with its measured row.
    pub fn remove(&mut self, id: &str) {
        if let Some(index) = self.position_by_id(id) {
            let removed = self.items.remove(index);
            let removed_was_tool = removed.as_ref().is_some_and(|item| item.is_tool());
            if let Some(removed) = removed.as_ref().filter(|item| item.turn_group_head) {
                self.promote_turn_group_head(removed.turn_id.as_deref());
            }
            self.bump_revision();
            self.rebuild_activity_group_flags();
            self.refresh_trailing_tool_group_start();
            self.layout_changes
                .push(TranscriptLayoutChange::Splice { range: index..index + 1, count: 0 });
            let affected = index.saturating_sub(1)..(index + 1).min(self.items.len());
            if !affected.is_empty() {
                self.layout_changes.push(TranscriptLayoutChange::RemeasureItems(affected));
            }
            if removed_was_tool
                && let Some(group) = [index.saturating_sub(1), index]
                    .into_iter()
                    .find_map(|probe| self.activity_group(probe))
            {
                self.layout_changes
                    .push(TranscriptLayoutChange::RemeasureItems(group.start..group.start + 1));
            }
        }
    }

    fn promote_turn_group_head(&mut self, turn_id: Option<&str>) {
        let Some(turn_id) = turn_id else { return };
        if let Some(item) = self
            .items
            .iter_mut()
            .find(|item| item.turn_grouped && item.turn_id.as_deref() == Some(turn_id))
        {
            item.turn_group_head = true;
            self.layout_changes.push(TranscriptLayoutChange::Reset(self.items.len()));
        }
    }

    /// Append an incremental text chunk to the row with this stable identity.
    pub fn append_text(&mut self, id: String, role: &str, title: SharedString, delta: &str) {
        if delta.is_empty() {
            return;
        }
        // Fast path for streaming: when the row already exists with the same
        // role and title, extend it in place instead of rebuilding a full item
        // and round-tripping through the upsert comparison. Tool rows keep the
        // general path so group summaries still update; new rows and role or
        // title changes do the same.
        let existing = self.position_by_id(&id);
        let same_row = existing
            .and_then(|index| self.items.get(index).map(|row| (index, row)))
            .filter(|(_, row)| !row.is_tool() && row.role == role && row.title == title)
            .map(|(index, _)| index);
        if let Some(index) = same_row {
            let Some(row) = self.items.get_mut(index) else {
                return;
            };
            let mut text = row.text.to_string();
            text.push_str(delta);
            row.text = text.into();
            row.prepare_render_content();
            self.bump_revision();
            self.layout_changes
                .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
            return;
        }
        let mut text = existing
            .and_then(|index| self.items.get(index))
            .map_or_else(String::new, |item| item.text.to_string());
        text.push_str(delta);
        self.upsert(TranscriptItem::new(id, role.to_string(), title, text));
    }

    pub fn upsert(&mut self, mut item: TranscriptItem) {
        // Unconditional: every path below either mutates a row or pushes, and
        // a spurious bump on a pure no-op only costs one projection recompute.
        self.bump_revision();
        if let Some(index) = self.position_by_id(&item.id) {
            let Some(existing) = self.items.get(index) else {
                return;
            };
            let role_changed = existing.role != item.role;
            let updated_is_tool = item.is_tool();
            let title_changed = existing.title != item.title;
            let text_changed = existing.text != item.text;
            let is_tool_update = updated_is_tool && existing.is_tool();
            let activity_status_changed = is_tool_update && existing.text.lines().next() != item.text.lines().next();
            let previous_status = if is_tool_update {
                ToolActivityStatus::from_text(&existing.text)
            } else {
                ToolActivityStatus::default()
            };
            let next_status = if is_tool_update {
                ToolActivityStatus::from_text(&item.text)
            } else {
                ToolActivityStatus::default()
            };
            let group_status_changed = is_tool_update && previous_status != next_status;
            let has_activity_status_changed = existing.has_activity_status != item.has_activity_status;
            let diff_changed = existing.diff != item.diff;
            let severity_changed = existing.notice_severity != item.notice_severity;
            let content_changed = role_changed
                || title_changed
                || text_changed
                || has_activity_status_changed
                || diff_changed
                || severity_changed;
            if !content_changed {
                let Some(existing) = self.items.get_mut(index) else {
                    return;
                };
                if item.output.is_some() {
                    existing.output = item.output;
                }
                return;
            }

            let group_head = group_status_changed.then(|| self.activity_group_start_for(index)).flatten();
            if text_changed || role_changed {
                item.prepare_render_content();
            } else if !role_changed {
                item.markdown.clone_from(&existing.markdown);
                item.accessibility_text.clone_from(&existing.accessibility_text);
                item.has_list = existing.has_list;
            }
            item.expanded = existing.expanded;
            item.completed_at.clone_from(&existing.completed_at);
            item.turn_id.clone_from(&existing.turn_id);
            item.canonical_turn_id.clone_from(&existing.canonical_turn_id);
            item.canonical_item_id.clone_from(&existing.canonical_item_id);
            item.archived_final = existing.archived_final;
            item.archived_phase.clone_from(&existing.archived_phase);
            item.turn_group_head = existing.turn_group_head;
            item.turn_grouped = existing.turn_grouped;
            item.turn_group_expanded = existing.turn_group_expanded;
            item.turn_files.clone_from(&existing.turn_files);
            item.turn_file_totals = existing.turn_file_totals;
            item.turn_files_incomplete = existing.turn_files_incomplete;
            item.turn_files_offset = existing.turn_files_offset;
            item.activity_expanded = existing.activity_expanded;
            item.activity_grouped = existing.activity_grouped;
            item.activity_group_head = existing.activity_group_head;
            item.activity_group_summary = existing.activity_group_summary;
            item.output_truncated = existing.output_truncated;
            item.output_item_id.clone_from(&existing.output_item_id);
            if item.output.is_none() {
                item.output.clone_from(&existing.output);
            }
            if item.activity_category.is_none() {
                item.activity_category = existing.activity_category;
            }
            let remeasure_row = role_changed
                || title_changed
                || has_activity_status_changed
                || diff_changed
                || severity_changed
                || if updated_is_tool {
                    activity_status_changed || (existing.expanded && text_changed)
                } else {
                    text_changed
                };
            if let Some(existing) = self.items.get_mut(index) {
                *existing = item;
            }
            if group_status_changed
                && let Some(head_index) = group_head
                && let Some(head) = self.items.get_mut(head_index)
            {
                head.activity_group_summary.replace_status(previous_status, next_status);
            }
            if role_changed {
                self.rebuild_activity_group_flags();
                self.refresh_trailing_tool_group_start();
                self.layout_changes.push(TranscriptLayoutChange::Remeasure);
            } else {
                if remeasure_row {
                    self.layout_changes
                        .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
                }
                if group_status_changed
                    && let Some(head_index) = group_head
                    && head_index != index
                    && self.items.get(head_index).is_some_and(|head| head.activity_grouped)
                {
                    self.layout_changes
                        .push(TranscriptLayoutChange::RemeasureItems(head_index..head_index + 1));
                }
            }
        } else {
            self.push(item);
        }
    }

    /// Record the verb category projected from a live event, mirroring
    /// [`Transcript::set_output`]: find by id, set on change, no layout
    /// remeasure (the category lands in the same cycle as the row push or
    /// upsert that already remeasures the affected rows).
    pub fn set_activity_category(&mut self, id: &str, category: ToolCategory) {
        if let Some(index) = self.position_by_id(id) {
            if let Some(item) = self.items.get_mut(index) {
                if item.activity_category != Some(category) {
                    item.activity_category = Some(category);
                    self.bump_revision();
                }
            }
        }
    }

    pub fn set_app_previews(&mut self, id: &str, app_previews: Vec<AppPreviewItem>) {
        if let Some(index) = self.position_by_id(id) {
            if let Some(item) = self.items.get_mut(index) {
                if item.app_previews != app_previews {
                    item.app_previews = app_previews;
                    self.bump_revision();
                }
            }
        }
    }

    /// Consecutive tool rows form a presentation group without changing event identities or ordering.
    pub fn activity_group(&self, index: usize) -> Option<Range<usize>> {
        if !self.items.get(index).is_some_and(|item| item.activity_grouped) {
            return None;
        }
        let start = self.activity_group_start_for(index)?;
        let total = self.items.get(start)?.activity_group_summary.total;
        Some(start..start.saturating_add(usize::from(total)).min(self.items.len()))
    }

    fn activity_group_start_for(&self, index: usize) -> Option<usize> {
        if !self.items.get(index).is_some_and(|item| item.is_tool()) {
            return None;
        }
        if let Some(start) = self.trailing_tool_group_start
            && index >= start
        {
            return Some(start);
        }
        let mut start = index;
        while start > 0 && self.items.get(start - 1).is_some_and(|item| item.is_tool()) {
            start -= 1;
        }
        Some(start)
    }

    fn rebuild_activity_group_flags(&mut self) {
        let mut start = 0;
        while start < self.items.len() {
            if !self.items.get(start).is_some_and(|item| item.is_tool()) {
                if let Some(item) = self.items.get_mut(start) {
                    item.activity_grouped = false;
                    item.activity_group_head = false;
                    item.activity_expanded = false;
                    item.activity_group_summary = ActivityGroupSummary::default();
                }
                start += 1;
                continue;
            }

            let mut end = start + 1;
            while self.items.get(end).is_some_and(|item| item.is_tool()) {
                end += 1;
            }
            let expanded = self.items.get(start).is_some_and(|item| item.activity_expanded);
            let mut summary = ActivityGroupSummary::default();
            for item in self.items.range(start..end) {
                summary.add_status(ToolActivityStatus::from_text(&item.text));
            }
            for index in start..end {
                if let Some(item) = self.items.get_mut(index) {
                    item.activity_grouped = end - start > 1;
                    item.activity_group_head = end - start > 1 && index == start;
                    item.activity_expanded = end - start > 1 && expanded;
                    item.activity_group_summary = if index == start {
                        summary
                    } else {
                        ActivityGroupSummary::default()
                    };
                }
            }
            start = end;
        }
    }

    fn refresh_trailing_tool_group_start(&mut self) {
        if !self.items.back().is_some_and(|item| item.is_tool()) {
            self.trailing_tool_group_start = None;
            return;
        }
        let mut start = self.items.len().saturating_sub(1);
        while start > 0 && self.items.get(start - 1).is_some_and(|item| item.is_tool()) {
            start -= 1;
        }
        self.trailing_tool_group_start = Some(start);
    }

    pub fn toggle_tool(&mut self, id: &str) {
        let last_index = self.items.len().checked_sub(1);
        let index = last_index
            .filter(|index| self.items.get(*index).is_some_and(|item| item.id == id && item.is_tool()))
            .or_else(|| self.items.iter().rposition(|item| item.id == id && item.is_tool()));
        if let Some(index) = index {
            if let Some(item) = self.items.get_mut(index) {
                item.expanded = !item.expanded;
            }
            self.bump_revision();
            self.layout_changes.push(TranscriptLayoutChange::PauseFollowingTail);
            self.layout_changes
                .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
        }
    }

    pub fn toggle_diff(&mut self, id: &str) {
        let last_index = self.items.len().checked_sub(1);
        let index = last_index
            .filter(|index| self.items.get(*index).is_some_and(|item| item.id == id && item.is_diff()))
            .or_else(|| self.items.iter().rposition(|item| item.id == id && item.is_diff()));
        if let Some(index) = index {
            if let Some(item) = self.items.get_mut(index) {
                item.expanded = !item.expanded;
            }
            self.bump_revision();
            self.layout_changes.push(TranscriptLayoutChange::PauseFollowingTail);
            self.layout_changes
                .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
        }
    }

    pub fn toggle_notice(&mut self, id: &str) {
        let last_index = self.items.len().checked_sub(1);
        let index = last_index
            .filter(|index| {
                self.items
                    .get(*index)
                    .is_some_and(|item| item.id == id && item.is_notice() && item.collapsible_details)
            })
            .or_else(|| {
                self.items
                    .iter()
                    .rposition(|item| item.id == id && item.is_notice() && item.collapsible_details)
            });
        if let Some(index) = index {
            if let Some(item) = self.items.get_mut(index) {
                item.expanded = !item.expanded;
            }
            self.bump_revision();
            self.layout_changes.push(TranscriptLayoutChange::PauseFollowingTail);
            self.layout_changes
                .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
        }
    }

    pub fn follow_latest(&mut self) {
        self.layout_changes.push(TranscriptLayoutChange::FollowLatest);
    }

    pub fn remeasure(&mut self) {
        // Consecutive full remeasures are redundant by construction: nothing
        // changed between them, so measuring twice yields the same layout.
        // Theme browsing, contrast drags, and font stepping enqueue bursts
        // while the transcript is unmounted (routes), and the consumer
        // applies every entry; collapsing the burst here keeps one probe per
        // drain instead of one per click.
        if !matches!(self.layout_changes.last(), Some(TranscriptLayoutChange::Remeasure)) {
            self.layout_changes.push(TranscriptLayoutChange::Remeasure);
        }
    }
}

#[cfg(test)]
mod tests;
