use std::collections::VecDeque;
use std::ops::Range;

use gpui_kit::SharedString;
use vtcode_exec_events::AppPreviewItem;

use super::markdown::prepare_markdown;

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
    /// Whether the prepared Markdown contains a list. Lists need a definite
    /// bubble width (see `PreparedMarkdown::has_list`); kept beside the other
    /// render projections so every prepare/clone path carries it together.
    pub has_list: bool,
    pub expanded: bool,
    /// Shared expansion state copied to each row so virtualized rendering stays constant-time.
    pub activity_expanded: bool,
    /// Whether this tool row is part of a consecutive group of at least two rows.
    pub activity_grouped: bool,
    /// Whether this row owns the visible header for its tool group.
    pub activity_group_head: bool,
    /// Status totals kept on the group head so streamed updates do not rescan every tool row.
    pub activity_group_summary: ActivityGroupSummary,
    /// Whether this row carries structured activity status rather than archive text.
    pub has_activity_status: bool,
    pub diff: Option<DiffCard>,
    /// Raw aggregated command/tool output for the Output inspector tab.
    pub output: Option<SharedString>,
    /// MCP Apps referenced by this output, for preview cards. Empty for non-app results.
    pub app_previews: Vec<AppPreviewItem>,
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

    /// Notice row (runtime errors, compaction/reset notices, run status) with
    /// its severity set at construction.
    pub fn notice(id: String, title: SharedString, text: String, severity: NoticeSeverity) -> Self {
        let mut item = Self::new(id, TranscriptRole::Notice.as_str().to_string(), title, text);
        item.notice_severity = severity;
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
            has_list: false,
            expanded: false,
            activity_expanded: false,
            activity_grouped: false,
            activity_group_head: false,
            activity_group_summary,
            diff: None,
            output: None,
            app_previews: Vec::new(),
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
            if let Some(removed) = self.items.pop_front()
                && removed.activity_group_head
            {
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
            let removed_was_tool = self.items.remove(index).is_some_and(|item| item.is_tool());
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

    /// Append an incremental text chunk to the row with this stable identity.
    pub fn append_text(&mut self, id: String, role: &str, title: SharedString, delta: &str) {
        if delta.is_empty() {
            return;
        }
        let mut text = self
            .position_by_id(&id)
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
            item.activity_expanded = existing.activity_expanded;
            item.activity_grouped = existing.activity_grouped;
            item.activity_group_head = existing.activity_group_head;
            item.activity_group_summary = existing.activity_group_summary;
            if item.output.is_none() {
                item.output.clone_from(&existing.output);
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

    /// Attach or replace raw command output, which is presented outside the transcript row.
    pub fn set_output(&mut self, id: &str, output: Option<SharedString>) {
        if let Some(index) = self.position_by_id(id) {
            if let Some(item) = self.items.get_mut(index) {
                if item.output != output {
                    item.output = output;
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

    pub fn toggle_activity_group(&mut self, id: &str) {
        if let Some(index) = self.position_by_id(id)
            && let Some(range) = self.activity_group(index)
        {
            let expanded = !self.items.get(range.start).is_some_and(|head| head.activity_expanded);
            for index in range.clone() {
                if let Some(item) = self.items.get_mut(index) {
                    item.activity_expanded = expanded;
                }
            }
            self.bump_revision();
            self.layout_changes.push(TranscriptLayoutChange::PauseFollowingTail);
            self.layout_changes.push(TranscriptLayoutChange::RemeasureItems(range));
        }
    }

    /// Expand a tool row and its containing activity group so a search result is visible.
    pub(super) fn reveal_tool_item(&mut self, id: &str) -> Option<usize> {
        let index = self.position_by_id(id)?;
        if !self.items.get(index).is_some_and(|item| item.is_tool()) {
            return None;
        }

        let group = self.activity_group(index);
        let expand_group = group
            .as_ref()
            .is_some_and(|range| !self.items.get(range.start).is_some_and(|item| item.activity_expanded));
        let expand_item = self.items.get(index).is_some_and(|item| !item.expanded);
        if !expand_group && !expand_item {
            return Some(index);
        }

        self.bump_revision();
        self.layout_changes.push(TranscriptLayoutChange::PauseFollowingTail);
        if expand_group && let Some(range) = group {
            for group_index in range.clone() {
                if let Some(item) = self.items.get_mut(group_index) {
                    item.activity_expanded = true;
                }
            }
            self.layout_changes.push(TranscriptLayoutChange::RemeasureItems(range));
        }
        if expand_item {
            if let Some(item) = self.items.get_mut(index) {
                item.expanded = true;
            }
            self.layout_changes
                .push(TranscriptLayoutChange::RemeasureItems(index..index + 1));
        }
        Some(index)
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

    pub fn follow_latest(&mut self) {
        self.layout_changes.push(TranscriptLayoutChange::FollowLatest);
    }

    pub fn remeasure(&mut self) {
        self.layout_changes.push(TranscriptLayoutChange::Remeasure);
    }
}

#[cfg(test)]
mod tests;
