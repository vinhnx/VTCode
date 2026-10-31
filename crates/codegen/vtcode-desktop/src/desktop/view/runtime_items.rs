//! Runtime item projection: ThreadItems into transcript rows, diff cards,
//! activity summaries, and bounded diff classification.

use super::*;

impl DesktopView {
    pub(super) fn finish_unverified_turn(&mut self, message: &str) {
        self.pending_submission = None;
        self.pending_first_submission = false;
        self.running = false;
        self.ui_progress_active = false;
        self.active_turn_id = None;
        self.cancel_requested = false;
        self.cancel_request_sent = false;
        self.status = rust_i18n::t!("app.transcript.turn_unverified").to_string();
        self.set_selected_session_completed(false);
        self.transcript.push(TranscriptItem::notice(
            format!("event-{}", uuid_like_id()),
            rust_i18n::t!("app.transcript.run_status").to_string().into(),
            message.to_string(),
            NoticeSeverity::Warning,
        ));
    }

    /// Keep the selected sidebar row current without rereading every archive.
    pub(super) fn set_selected_session_completed(&mut self, completed: bool) {
        let Some(session_id) = self.selected_session_id.as_deref() else {
            return;
        };
        if let Some(session) = self.sessions.iter_mut().find(|session| session.session_id == session_id) {
            session.completed = completed;
        }
    }

    pub(super) fn update_runtime_item(&mut self, item: &vtcode_exec_events::ThreadItem) {
        self.retain_restored_activity(|restored| restored.id != item.id);
        let _cursor = self.restored_output_cursors.remove(&item.id);
        let _pages = self.restored_output_pages.remove(&item.id);
        if self
            .output_page_pending
            .as_ref()
            .is_some_and(|pending| pending.item_id == item.id)
        {
            self.output_page_pending = None;
            self.output_page_task = None;
            self.output_page_error = None;
        }
        self.project_runtime_item(item, false);
    }

    pub(super) fn project_runtime_item(&mut self, item: &vtcode_exec_events::ThreadItem, historical: bool) {
        use vtcode_exec_events::ThreadItemDetails;
        let id = item.id.clone();
        match &item.details {
            ThreadItemDetails::AgentMessage(message) => {
                self.append_or_update_item(id, TranscriptRole::Assistant.as_str(), app::NAME, &message.text, false);
            }
            ThreadItemDetails::Plan(plan) => {
                let title = rust_i18n::t!(i18n::TRANSCRIPT_PLAN_TITLE).to_string();
                self.append_or_update_item(id, TranscriptRole::Assistant.as_str(), &title, &plan.text, false);
            }
            ThreadItemDetails::Reasoning(reasoning) => {
                let title = rust_i18n::t!("app.transcript.planning_update").to_string();
                self.append_or_update_item(id, TranscriptRole::Assistant.as_str(), &title, &reasoning.text, false);
            }
            ThreadItemDetails::Decision(decision) => {
                let title = rust_i18n::t!("app.transcript.decision_title").to_string();
                self.append_or_update_item(id, TranscriptRole::Assistant.as_str(), &title, &decision.summary, false);
            }
            ThreadItemDetails::FileChange(change) => self.upsert_diff_card(id, change),
            details => {
                let (title, mut status) = describe_activity(details);
                if historical && ActivityState::from_status(&status) == ActivityState::Running {
                    status = tool_status::INTERRUPTED.to_string();
                }
                let payload = activity_details_payload(details);
                let text = format!("{status}\n\n{payload}");
                // Keep the raw aggregated output for the inspector's Output
                // tab; it updates in place as streaming events arrive.
                let output = match details {
                    ThreadItemDetails::CommandExecution(command) => Some(command.aggregated_output.as_str().into()),
                    ThreadItemDetails::ToolOutput(tool_output) => Some(tool_output.output.as_str().into()),
                    _ => None,
                };
                let app_previews = match details {
                    ThreadItemDetails::ToolOutput(tool_output) => tool_output.app_previews.clone(),
                    _ => Vec::new(),
                };
                if historical {
                    let mut item =
                        TranscriptItem::new(id, TranscriptRole::Tool.as_str().to_string(), title.into(), text);
                    item.output = output;
                    item.app_previews = app_previews;
                    self.push_restored_activity(item);
                } else {
                    self.append_or_update_item(id.clone(), TranscriptRole::Tool.as_str(), &title, &text, true);
                    if output.is_some() {
                        self.transcript.set_output(&id, output);
                    }
                    if !app_previews.is_empty() {
                        self.transcript.set_app_previews(&id, app_previews);
                    }
                }
            }
        }
    }

    pub(super) fn upsert_diff_card(&mut self, id: String, change: &vtcode_exec_events::FileChangeItem) {
        let files = change
            .changes
            .iter()
            .map(|file| DiffFile {
                path: file.path.clone(),
                kind: match file.kind {
                    vtcode_exec_events::PatchChangeKind::Add => DiffFileKind::Add,
                    vtcode_exec_events::PatchChangeKind::Delete => DiffFileKind::Delete,
                    vtcode_exec_events::PatchChangeKind::Update => DiffFileKind::Update,
                },
            })
            .collect();
        let card = DiffCard {
            files,
            additions: change.additions,
            deletions: change.deletions,
            incomplete: change.diff_incomplete.unwrap_or(false),
            failed: change.status == vtcode_exec_events::PatchApplyStatus::Failed,
            patch: change.unified_diff.clone().unwrap_or_default(),
        };
        let mut item = TranscriptItem::new(
            id,
            TranscriptRole::Diff.as_str().to_string(),
            rust_i18n::t!("app.transcript.file_changes_title").to_string().into(),
            card.patch.clone(),
        );
        item.diff = Some(card);
        let previous_patch = self
            .selected_change
            .as_deref()
            .and_then(|path| self.changes.patch_for(&self.transcript, path));
        self.transcript.upsert(item);
        // Change-sets are rare; rebuild the inspector projection eagerly so
        // renders never parse patches.
        self.changes = ChangesProjection::rebuild(&self.transcript);
        self.change_viewer_dirty |= self
            .selected_change
            .as_deref()
            .is_some_and(|path| self.changes.patch_for(&self.transcript, path) != previous_patch);
    }

    /// Refresh only when the selected file's accepted patch changed, preserving its mode.
    pub(super) fn refresh_selected_change(&mut self, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.change_viewer_dirty) {
            return;
        }
        let file = self.selected_change.as_deref().and_then(|path| self.changes.select(path));
        if let (Some(viewer), Some(file)) = (self.change_viewer.as_ref(), file) {
            viewer.update(cx, |viewer, cx| viewer.set_files(vec![file], cx));
        } else {
            self.selected_change = None;
            self.change_viewer = None;
            if self.workspace_review.is_none() && self.route == Route::Review {
                self.route = Route::Workspace;
            }
        }
    }

    /// Open one changed file in the inspector's syntax-highlighted diff view.
    pub(super) fn select_change(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(file) = self.changes.select(&path) else {
            return;
        };
        if let Some(index) = self.changes.entries().iter().position(|entry| entry.path == path) {
            self.change_list_scroll.scroll_to_item(index, gpui_kit::ScrollStrategy::Top);
        }
        let mode = self
            .change_viewer
            .as_ref()
            .map(|viewer| viewer.read(cx).mode())
            .unwrap_or(DiffMode::Unified);
        self.selected_change = Some(path);
        self.change_viewer_dirty = false;
        self.change_viewer = Some(cx.new(|cx| DiffState::new(vec![file], cx).with_mode(mode)));
        self.inspector_open = true;
        self.set_inspector_tab(InspectorTab::Changes, cx);
    }

    pub(super) fn select_activity_output(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_output_id = Some(id);
        self.inspector_open = true;
        self.set_inspector_tab(InspectorTab::Output, cx);
        self.inspector_focus.focus(window, cx);
    }

    pub(super) fn navigate_change_file(&mut self, next: bool, cx: &mut Context<Self>) {
        let available = self
            .changes
            .entries()
            .iter()
            .filter(|entry| self.changes.has_preview(&entry.path))
            .collect::<Vec<_>>();
        if available.len() < 2 {
            return;
        }
        let current = self
            .selected_change
            .as_deref()
            .and_then(|path| available.iter().position(|entry| entry.path.as_str() == path));
        let index = match current {
            Some(index) if next => (index + 1) % available.len(),
            Some(0) => available.len() - 1,
            Some(index) => index - 1,
            None => 0,
        };
        if let Some(entry) = available.get(index) {
            self.select_change(entry.path.clone(), cx);
        }
    }
}

/// Human title and lifecycle summary for a non-message activity item.
fn describe_activity(details: &vtcode_exec_events::ThreadItemDetails) -> (String, String) {
    use vtcode_exec_events::ThreadItemDetails;
    match details {
        ThreadItemDetails::ToolInvocation(tool) => (tool.tool_name.clone(), tool_status_label(&tool.status)),
        ThreadItemDetails::ToolOutput(output) => {
            (rust_i18n::t!("app.transcript.tool_output_title").to_string(), tool_status_label(&output.status))
        }
        ThreadItemDetails::CommandExecution(command) => {
            let mut label = command_status_label(&command.status);
            if let Some(code) = command.exit_code
                && matches!(command.status, vtcode_exec_events::CommandExecutionStatus::Failed)
            {
                label = format!("{}{code})", tool_status::FAILED_EXIT_PREFIX);
            }
            (command.command.clone(), label)
        }
        ThreadItemDetails::McpToolCall(call) => {
            let label = match call.status {
                Some(vtcode_exec_events::McpToolCallStatus::Started) => tool_status::STARTED,
                Some(vtcode_exec_events::McpToolCallStatus::Completed) => tool_status::COMPLETED,
                Some(vtcode_exec_events::McpToolCallStatus::Failed) => tool_status::FAILED,
                None => tool_status::ACTIVITY,
            }
            .to_string();
            (format!("MCP · {}", call.tool_name), label)
        }
        ThreadItemDetails::WebSearch(search) => {
            let label = match search.provider.as_deref() {
                Some(provider) => rust_i18n::t!("app.transcript.web_search_via", provider = provider).to_string(),
                None => rust_i18n::t!("app.transcript.web_search").to_string(),
            };
            (search.query.clone(), label)
        }
        ThreadItemDetails::Harness(event) => {
            let label = event.message.clone().unwrap_or_else(|| "harness activity".to_string());
            let title = event
                .command
                .clone()
                .or_else(|| event.path.clone())
                .unwrap_or_else(|| "Harness".to_string());
            (title, label)
        }
        ThreadItemDetails::Error(error) => {
            (rust_i18n::t!("app.transcript.runtime_error").to_string(), error.message.clone())
        }
        ThreadItemDetails::AgentMessage(_)
        | ThreadItemDetails::Plan(_)
        | ThreadItemDetails::Reasoning(_)
        | ThreadItemDetails::Decision(_)
        | ThreadItemDetails::FileChange(_) => ("Activity".to_string(), tool_status::ACTIVITY.to_string()),
    }
}

#[derive(serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ActivityDetailsMetadata<'a> {
    CommandExecution {
        command: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        arguments: Option<&'a Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<&'a i32>,
        status: &'a vtcode_exec_events::CommandExecutionStatus,
    },
    ToolOutput {
        call_id: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_call_id: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        spool_path: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<&'a i32>,
        status: &'a vtcode_exec_events::ToolCallStatus,
    },
}

/// Keep large tool output out of the metadata serialization path.
pub(super) fn activity_details_payload(details: &vtcode_exec_events::ThreadItemDetails) -> String {
    use vtcode_exec_events::ThreadItemDetails;

    let serialized = match details {
        ThreadItemDetails::CommandExecution(command) => {
            serde_json::to_string_pretty(&ActivityDetailsMetadata::CommandExecution {
                command: &command.command,
                arguments: command.arguments.as_ref(),
                exit_code: command.exit_code.as_ref(),
                status: &command.status,
            })
        }
        ThreadItemDetails::ToolOutput(output) => serde_json::to_string_pretty(&ActivityDetailsMetadata::ToolOutput {
            call_id: &output.call_id,
            tool_call_id: output.tool_call_id.as_deref(),
            spool_path: output.spool_path.as_deref(),
            exit_code: output.exit_code.as_ref(),
            status: &output.status,
        }),
        _ => serde_json::to_string_pretty(details),
    };
    serialized.unwrap_or_else(|_| "{}".to_string())
}

fn tool_status_label(status: &vtcode_exec_events::ToolCallStatus) -> String {
    match status {
        vtcode_exec_events::ToolCallStatus::Completed => tool_status::COMPLETED,
        vtcode_exec_events::ToolCallStatus::Failed => tool_status::FAILED,
        vtcode_exec_events::ToolCallStatus::InProgress => tool_status::IN_PROGRESS,
    }
    .to_string()
}

fn command_status_label(status: &vtcode_exec_events::CommandExecutionStatus) -> String {
    match status {
        vtcode_exec_events::CommandExecutionStatus::Completed => tool_status::COMPLETED,
        vtcode_exec_events::CommandExecutionStatus::Failed => tool_status::FAILED,
        vtcode_exec_events::CommandExecutionStatus::InProgress => tool_status::RUNNING,
    }
    .to_string()
}

/// One classified line of a unified diff.
pub(super) struct DiffLine<'a> {
    pub(super) kind: DiffLineKind,
    pub(super) text: &'a str,
}

pub(super) const MAX_DIFF_LINES: usize = 2_000;

pub(super) struct ClassifiedDiffLines<'a> {
    lines: std::str::Lines<'a>,
    remaining: usize,
    checked_limit: bool,
    pub(super) truncated: bool,
}

impl<'a> ClassifiedDiffLines<'a> {
    pub(super) fn new(patch: &'a str) -> Self {
        Self {
            lines: patch.lines(),
            remaining: MAX_DIFF_LINES,
            checked_limit: false,
            truncated: false,
        }
    }
}

impl<'a> Iterator for ClassifiedDiffLines<'a> {
    type Item = DiffLine<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            if !self.checked_limit {
                self.truncated = self.lines.next().is_some();
                self.checked_limit = true;
            }
            return None;
        }

        let Some(text) = self.lines.next() else {
            self.remaining = 0;
            self.checked_limit = true;
            return None;
        };
        self.remaining -= 1;
        Some(DiffLine { kind: classify_diff_line(text), text })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DiffLineKind {
    Header,
    Hunk,
    Added,
    Removed,
    Context,
}

fn classify_diff_line(line: &str) -> DiffLineKind {
    if line.starts_with("+++") || line.starts_with("---") || line.starts_with("diff ") {
        DiffLineKind::Header
    } else if line.starts_with("@@") {
        DiffLineKind::Hunk
    } else if line.starts_with('+') {
        DiffLineKind::Added
    } else if line.starts_with('-') {
        DiffLineKind::Removed
    } else {
        DiffLineKind::Context
    }
}

/// Raw unified-diff segment for one file of a change-set, for per-file copy.
///
/// Sections split on `diff --git ` / `--- ` boundaries outside hunks,
/// mirroring Kit's `DiffFile::parse`: hunk bodies are consumed by the `@@`
/// counts, so a removed `-- x` comment never splits a file, and the first
/// `--- ` belongs to the section its `diff --git ` header opened. A section
/// matches when a `diff --git ` / `--- ` / `+++ ` entry names the path
/// exactly (quotes and `a/` / `b/` prefixes stripped), so `src/app.rs` never
/// matches `src/app.rs.bak`. A patch without any boundary is returned whole
/// only for single-file change-sets; anything unattributable yields `None`
/// so callers hide per-file actions instead of copying wrong content.
pub(super) fn file_patch_segment<'a>(patch: &'a str, path: &str, file_count: usize) -> Option<&'a str> {
    if patch.is_empty() || path.is_empty() {
        return None;
    }
    let mut starts = vec![0];
    let mut offset = 0;
    let mut saw_boundary = false;
    let mut hunk_remaining: Option<HunkCounts> = None;
    let mut section_has_old_header = false;
    for line in patch.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        if let Some(counts) = hunk_remaining.as_mut() {
            if counts.consume(body) {
                offset += line.len();
                continue;
            }
            hunk_remaining = None;
        }
        if let Some(counts) = parse_hunk_header(body) {
            hunk_remaining = Some(counts);
        } else if body.starts_with("diff --git ") {
            saw_boundary = true;
            if offset != 0 {
                starts.push(offset);
            }
            section_has_old_header = false;
        } else if body.starts_with("--- ") {
            saw_boundary = true;
            if section_has_old_header {
                starts.push(offset);
                section_has_old_header = false;
            } else {
                section_has_old_header = true;
            }
        }
        offset += line.len();
    }
    if starts.len() == 1 && !saw_boundary {
        return (file_count == 1).then_some(patch);
    }
    starts.push(patch.len());
    starts.array_windows::<2>().find_map(|[from, to]| {
        let section = patch.get(*from..*to)?;
        section_names_path(section, path).then_some(section)
    })
}

/// Remaining body lines of one hunk, from its `@@` counts.
struct HunkCounts {
    old_remaining: usize,
    new_remaining: usize,
}

impl HunkCounts {
    /// Consumes a hunk body line. Context lines count on both sides;
    /// no-newline markers consume nothing. Returns false once the counts
    /// are exhausted or the line cannot belong to the hunk. A `-` / `+`
    /// line that exhausts the counts but opens a `--- ` / `+++ ` header is
    /// released, so skewed counts still split files instead of swallowing
    /// the next header as body.
    fn consume(&mut self, line: &str) -> bool {
        if self.old_remaining == 0 && self.new_remaining == 0 {
            return false;
        }
        match line.chars().next() {
            Some(' ') => {
                self.old_remaining = self.old_remaining.saturating_sub(1);
                self.new_remaining = self.new_remaining.saturating_sub(1);
                true
            }
            Some('-') => {
                self.old_remaining = self.old_remaining.saturating_sub(1);
                self.old_remaining > 0 || self.new_remaining > 0 || !line.starts_with("--- ")
            }
            Some('+') => {
                self.new_remaining = self.new_remaining.saturating_sub(1);
                self.old_remaining > 0 || self.new_remaining > 0 || !line.starts_with("+++ ")
            }
            // `\ No newline at end of file` attaches to the previous line.
            Some('\\') => true,
            _ => false,
        }
    }
}

/// Parses `@@ -old[,count] +new[,count] @@`; a missing count means one line.
fn parse_hunk_header(line: &str) -> Option<HunkCounts> {
    let (ranges, _) = line.strip_prefix("@@ ")?.split_once("@@")?;
    let mut parts = ranges.split_whitespace();
    let old = parse_hunk_range(parts.next()?, '-')?;
    let new = parse_hunk_range(parts.next()?, '+')?;
    Some(HunkCounts { old_remaining: old, new_remaining: new })
}

fn parse_hunk_range(range: &str, sign: char) -> Option<usize> {
    let range = range.strip_prefix(sign)?;
    match range.split_once(',') {
        Some((_, count)) => count.parse().ok(),
        None => Some(1),
    }
}

/// Whether any `diff --git ` / `--- ` / `+++ ` entry of a section names
/// `path` exactly.
fn section_names_path(section: &str, path: &str) -> bool {
    section.lines().any(|line| {
        if let Some(header) = line.strip_prefix("diff --git ") {
            split_git_header_entries(header)
                .iter()
                .any(|entry| diff_path_entry_matches(entry, path))
        } else {
            line.strip_prefix("--- ")
                .or_else(|| line.strip_prefix("+++ "))
                .is_some_and(|entry| diff_path_entry_matches(entry, path))
        }
    })
}

/// Splits a `diff --git ` header into path entries, keeping quoted paths
/// with spaces intact.
fn split_git_header_entries(header: &str) -> Vec<&str> {
    let mut entries = Vec::new();
    let mut start = None;
    let mut in_quotes = false;
    for (index, ch) in header.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ' ' | '\t' if !in_quotes => {
                if let Some(from) = start.take()
                    && let Some(entry) = header.get(from..index)
                {
                    entries.push(entry);
                }
            }
            _ => {
                if start.is_none() {
                    start = Some(index);
                }
            }
        }
    }
    if let Some(from) = start
        && let Some(entry) = header.get(from..)
    {
        entries.push(entry);
    }
    entries
}

/// Exact entry match after stripping one layer of quotes and `a/` / `b/`
/// prefixes, so prefix paths never match longer siblings.
fn diff_path_entry_matches(entry: &str, path: &str) -> bool {
    let entry = entry
        .strip_prefix('"')
        .and_then(|quoted| quoted.strip_suffix('"'))
        .unwrap_or(entry);
    let bare = entry.strip_prefix("a/").or_else(|| entry.strip_prefix("b/")).unwrap_or(entry);
    bare == path
}

impl DesktopView {
    pub(super) fn render_diff_item(&self, item: &TranscriptItem, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let diff_foreground =
            themes::diff_text_foreground(colors.background, colors.foreground, colors.success, colors.danger);
        let mono_font = cx.theme().mono_font_family.clone();
        let Some(diff) = item.diff.as_ref() else {
            return div().into_any_element();
        };
        let id = item.id.clone();
        let status_label = if diff.failed {
            rust_i18n::t!("app.transcript.patch_failed").to_string()
        } else if diff.incomplete {
            rust_i18n::t!("app.transcript.preview_incomplete").to_string()
        } else {
            rust_i18n::t!("app.transcript.applied").to_string()
        };
        let toggle_id = id.clone();
        let mut stats = div().flex().items_center().gap_2().text_sm();
        if let Some(additions) = diff.additions {
            stats = stats.child(div().text_color(colors.success).child(format!("+{additions}")));
        }
        if let Some(deletions) = diff.deletions {
            stats = stats.child(div().text_color(colors.danger).child(format!("−{deletions}")));
        }

        // The toggle and copy actions share one trailing lane so the toggle
        // never floats mid-row when the patch is present.
        let mut actions = div().flex().flex_none().items_center().child(
            desktop_button(format!("diff-{}", item.id))
                .label(if item.expanded {
                    rust_i18n::t!("app.transcript.hide_diff").to_string()
                } else {
                    rust_i18n::t!("app.transcript.show_diff").to_string()
                })
                .ghost()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.transcript.toggle_diff(&toggle_id);
                    cx.notify();
                })),
        );
        if !diff.patch.is_empty() {
            let copy_id = id.clone();
            actions = actions.child(
                desktop_button(format!("copy-patch-{}", item.id))
                    .label(rust_i18n::t!("app.transcript.copy_patch").to_string())
                    .ghost()
                    .on_click(cx.listener(move |view, _, window, cx| {
                        let Some(patch) = view
                            .transcript
                            .items_newest_first()
                            .find(|candidate| candidate.id == copy_id)
                            .and_then(|candidate| candidate.diff.as_ref())
                            .map(|diff| diff.patch.clone())
                            .filter(|patch| !patch.is_empty())
                        else {
                            return;
                        };
                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(patch));
                        super::super::notifications::copied_to_clipboard(window, cx);
                    })),
            );
        }

        let header = div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .px_3()
            .py_2()
            .bg(colors.secondary)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().font_medium().child(item.title.clone()))
                    .child(stats)
                    .child(
                        div()
                            .text_sm()
                            .text_color(if diff.failed {
                                colors.danger
                            } else {
                                themes::muted_text(&colors)
                            })
                            .child(status_label),
                    ),
            )
            .child(actions);

        let mut panel = div()
            .relative()
            .w_full()
            .max_w(px(self.content_width()))
            .rounded_md()
            .overflow_hidden()
            .child(header);

        let open_label = rust_i18n::t!("app.transcript.open_file").to_string();
        let copy_path_label = rust_i18n::t!("app.transcript.copy_path").to_string();
        let copy_diff_label = rust_i18n::t!("app.transcript.copy_diff").to_string();
        let mut body = div().w_full().flex().flex_col().px_3().py_2().gap_1();
        for file in &diff.files {
            let (kind_label, kind_color) = match file.kind {
                DiffFileKind::Add => ("added", colors.success),
                DiffFileKind::Delete => ("deleted", colors.danger),
                DiffFileKind::Update => ("modified", colors.muted_foreground),
            };
            let open_path = file.path.clone();
            let path_copy = file.path.clone();
            let mut actions = div()
                .flex()
                .flex_none()
                .items_center()
                .child(
                    desktop_button(opaque_path_element_id(&format!("diff-open-{}", item.id), &file.path))
                        .icon(IconName::ExternalLink)
                        .accessibility_label(open_label.clone())
                        .tooltip(open_label.clone())
                        .ghost()
                        .xsmall()
                        .tab_stop(false)
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.open_file_preview(open_path.clone(), window, cx);
                        })),
                )
                .child(
                    desktop_button(opaque_path_element_id(&format!("diff-copy-path-{}", item.id), &file.path))
                        .icon(IconName::Link)
                        .accessibility_label(copy_path_label.clone())
                        .tooltip(copy_path_label.clone())
                        .ghost()
                        .xsmall()
                        .tab_stop(false)
                        .on_click(move |_, window, cx| {
                            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(path_copy.clone()));
                            super::super::notifications::copied_to_clipboard(window, cx);
                        }),
                );
            if file_patch_segment(&diff.patch, &file.path, diff.files.len()).is_some() {
                let card_id = item.id.clone();
                let segment_path = file.path.clone();
                actions = actions.child(
                    desktop_button(opaque_path_element_id(&format!("diff-copy-diff-{}", item.id), &file.path))
                        .icon(IconName::Copy)
                        .accessibility_label(copy_diff_label.clone())
                        .tooltip(copy_diff_label.clone())
                        .ghost()
                        .xsmall()
                        .tab_stop(false)
                        .on_click(cx.listener(move |view, _, window, cx| {
                            let Some(segment) = view
                                .transcript
                                .items_newest_first()
                                .find(|candidate| candidate.id == card_id)
                                .and_then(|candidate| candidate.diff.as_ref())
                                .and_then(|diff| file_patch_segment(&diff.patch, &segment_path, diff.files.len()))
                                .map(str::to_string)
                            else {
                                return;
                            };
                            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(segment));
                            super::super::notifications::copied_to_clipboard(window, cx);
                        })),
                );
            }
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .w_full()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .font_family(mono_font.clone())
                                    .child(file.path.clone()),
                            )
                            .child(div().flex_none().text_xs().text_color(kind_color).child(kind_label)),
                    )
                    .child(actions),
            );
        }
        if diff.incomplete {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(rust_i18n::t!("app.transcript.previews_unavailable").to_string()),
            );
        }

        if item.expanded && !diff.patch.is_empty() {
            let mut lines = ClassifiedDiffLines::new(&diff.patch);
            let mut diff_view = div()
                .id(format!("diff-lines-{}", item.id))
                .test_support()
                .w_full()
                .flex()
                .flex_col()
                .mt_2()
                .rounded_md()
                .overflow_hidden()
                .overflow_x_scroll()
                .bg(colors.background);
            for line in &mut lines {
                let (line_color, line_bg, marker_color) = match line.kind {
                    DiffLineKind::Added => (
                        diff_foreground,
                        themes::diff_line_background(colors.background, diff_foreground, colors.success),
                        Some(("+", "+", colors.success)),
                    ),
                    DiffLineKind::Removed => (
                        diff_foreground,
                        themes::diff_line_background(colors.background, diff_foreground, colors.danger),
                        Some(("-", "−", colors.danger)),
                    ),
                    DiffLineKind::Hunk => (colors.muted_foreground, colors.secondary, None),
                    DiffLineKind::Header | DiffLineKind::Context => (colors.foreground, colors.background, None),
                };
                let mut row = div()
                    .w_full()
                    .px_2()
                    .text_xs()
                    .font_family(mono_font.clone())
                    .whitespace_nowrap()
                    .bg(line_bg);
                if let Some((source_marker, marker, marker_color)) = marker_color {
                    let content = line.text.strip_prefix(source_marker).unwrap_or_default();
                    row = row
                        .flex()
                        .child(
                            div()
                                .w_4()
                                .flex_shrink_0()
                                .font_semibold()
                                .text_color(marker_color)
                                .child(marker),
                        )
                        .child(div().text_color(line_color).child(content.to_string()));
                } else {
                    row = row.text_color(line_color).child(line.text.to_string());
                }
                diff_view = diff_view.child(row);
            }
            if lines.truncated {
                diff_view = diff_view.child(
                    div()
                        .w_full()
                        .px_2()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(rust_i18n::t!("app.transcript.diff_truncated").to_string()),
                );
            }
            body = body.child(diff_view);
        }
        panel = panel.child(body);
        panel.into_any_element()
    }
}
