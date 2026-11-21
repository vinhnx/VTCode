mod edit_metadata;
mod turn;
use std::collections::{HashMap, VecDeque};
use std::ops::Range;

use gpui_kit::SharedString;
use vtcode_exec_events::AppPreviewItem;

mod activity;
mod activity_verbs;
mod interaction;
mod item;
mod output;
mod session_poll;
mod tool_summary;
mod user_attachments;
pub(super) use activity_verbs::{
    ToolCategory, ToolCategoryCounts, describe_activity_verbs, split_tool_title_verb, tool_row_gist,
};
pub(super) use item::{EditTotals, SessionPollRun, TranscriptItem};
pub(super) use session_poll::{SessionPollKey, session_poll_key, title_with_poll_count};
pub(super) use tool_summary::{activity_details_summary, activity_output_text, activity_payload, activity_text};
pub(super) use turn::humanize_elapsed;
pub(super) use user_attachments::UserAttachment;
pub(super) use vtcode_commons::tool_headlines::harness_is_user_visible;

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
/// run status). Other roles leave the default untouched. Declaration order is
/// ascending severity, so a folded stack can take the loudest level with `max`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum NoticeSeverity {
    #[default]
    Info,
    Warning,
    Error,
}

/// An ephemeral notice folded into a consecutive notice stack. Only plain
/// notices (which are appended with a fresh id and never updated in place) are
/// foldable, so a payload never needs id-based lookup after it is absorbed.
#[derive(Clone, Debug)]
pub(super) struct NoticePayload {
    pub(super) id: String,
    pub(super) title: SharedString,
    pub(super) text: SharedString,
    pub(super) severity: NoticeSeverity,
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

/// Service-supplied phase of an archived assistant row.
///
/// `TranscriptItem::archived_phase` keeps the raw value from history so an
/// unknown future phase still renders; these are the two phases the desktop
/// grouping needs to distinguish. Parse once here instead of repeating the
/// wire spellings at each grouping site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ArchivedPhase {
    Commentary,
    FinalAnswer,
}

impl ArchivedPhase {
    const COMMENTARY: &str = "commentary";
    const FINAL_ANSWER: &str = "final_answer";

    pub(super) fn from_phase(phase: Option<&str>) -> Option<Self> {
        match phase {
            Some(Self::COMMENTARY) => Some(Self::Commentary),
            Some(Self::FINAL_ANSWER) => Some(Self::FinalAnswer),
            _ => None,
        }
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ActivityGroupSummary {
    pub total: u16,
    pub running: u16,
    pub failed: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ToolActivityStatus {
    running: bool,
    failed: bool,
}

impl ActivityGroupSummary {
    pub(crate) fn for_status(status: ToolActivityStatus) -> Self {
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
    pub(crate) fn from_text(text: &str) -> Self {
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

impl DiffFileKind {
    /// Stable kind slug shared by change-filter ids and diff cards.
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Add => "added",
            Self::Delete => "deleted",
            Self::Update => "modified",
        }
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
    /// Coarser sibling of `revision` that only changes when a row's activity
    /// meaning changes (title, status, category, output presence). Pure output
    /// text deltas bump `revision` but not this counter, so the Activity
    /// projection is not rebuilt for every PTY chunk of a streaming command.
    activity_revision: u64,
    /// Absorbed session-poll call ids → surviving row id, so a merged wait's
    /// later output still lands on the survivor.
    coalesced_call_rows: HashMap<String, String>,
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

    /// Generation for projections that depend on row identity/status but not
    /// on captured output text (notably the Activity tab).
    pub(super) fn activity_revision(&self) -> u64 {
        self.activity_revision
    }

    fn bump_revision(&mut self) {
        // Wrapping is deliberate: the counter is a cache key, not a count.
        // A collision needs 2^64 mutations without an intervening cache read.
        self.revision = self.revision.wrapping_add(1);
        self.activity_revision = self.activity_revision.wrapping_add(1);
    }

    /// Bump only the general revision for output-text changes. Activity rows
    /// read output *presence*, which this mutation does not change; callers
    /// that cross the none↔some boundary must use `bump_revision` instead.
    fn bump_output_revision(&mut self) {
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
        self.coalesced_call_rows.clear();
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
        // Consecutive ephemeral notices collapse into one announcement. The
        // head keeps its own body behind a Details CTA; folded entries surface
        // through a count CTA, so a blocked turn reads as one callout instead
        // of a wall of near-identical rows.
        if item.can_fold_notice() {
            let head_index = self.items.len().checked_sub(1);
            let foldable = head_index
                .and_then(|index| self.items.get(index))
                .is_some_and(TranscriptItem::can_fold_notice);
            if let Some(index) = head_index.filter(|_| foldable)
                && let Some(head) = self.items.get_mut(index)
            {
                head.notice_stack.push(NoticePayload {
                    id: item.id,
                    title: item.title.clone(),
                    text: item.text.clone(),
                    severity: item.notice_severity,
                });
                head.notice_severity = head.notice_severity.max(item.notice_severity);
                self.bump_revision();
                self.layout_changes
                    .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
                return;
            }
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
                if removed.session_poll.is_some() {
                    self.coalesced_call_rows.retain(|_, survivor| survivor != &removed.id);
                }
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
            if removed.as_ref().is_some_and(|row| row.session_poll.is_some()) {
                self.coalesced_call_rows.retain(|_, survivor| survivor != id);
            }
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
                if item.output.is_some() && existing.output != item.output {
                    existing.output = item.output;
                    self.bump_revision();
                }
                return;
            }

            self.bump_revision();
            let Some(existing) = self.items.get(index) else {
                return;
            };
            let group_head = group_status_changed.then(|| self.activity_group_start_for(index)).flatten();
            if text_changed || role_changed {
                item.prepare_render_content();
            } else if !role_changed {
                item.markdown.clone_from(&existing.markdown);
                item.accessibility_text.clone_from(&existing.accessibility_text);
                item.has_list = existing.has_list;
            }
            item.expanded = existing.expanded;
            if item.is_notice() && existing.is_notice() {
                item.notice_is_diagnostic = existing.notice_is_diagnostic;
                item.notice_stack.clone_from(&existing.notice_stack);
                item.notice_stack_expanded = existing.notice_stack_expanded;
            }
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
            if item.session_poll.is_none() {
                item.session_poll.clone_from(&existing.session_poll);
            }
            if item.session_poll.is_some() && item.app_previews.is_empty() {
                item.app_previews.clone_from(&existing.app_previews);
            }
            if item.tool_name.is_none() {
                item.tool_name.clone_from(&existing.tool_name);
            }
            if item.command.is_none() {
                item.command.clone_from(&existing.command);
            }
            if item.edit_path.is_none() {
                item.edit_path.clone_from(&existing.edit_path);
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
}

#[cfg(test)]
mod tests;
