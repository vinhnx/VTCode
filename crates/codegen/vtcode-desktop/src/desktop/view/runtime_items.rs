//! Runtime item projection: ThreadItems into transcript rows, diff cards,
//! activity summaries, and bounded diff classification.

use super::*;

impl DesktopView {
    pub(super) fn finish_unverified_turn(&mut self, message: &str) {
        self.pending_submission = None;
        self.pending_first_submission = false;
        self.prune_attachment_thumbnails();
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
        self.drop_superseded_restored_item(&item.id);
        self.project_runtime_item(item, false);
    }

    /// Drop restored projections superseded by a newer item with the same
    /// identity: its historical activity row, output cursors/pages, and any
    /// pending output page. Shared by live updates and session-load batches.
    fn drop_superseded_restored_item(&mut self, item_id: &str) {
        self.retain_restored_activity(|restored| restored.id.as_str() != item_id);
        let _cursor = self.restored_output_cursors.remove(item_id);
        let _pages = self.restored_output_pages.remove(item_id);
        if self
            .output_page_pending
            .as_ref()
            .is_some_and(|pending| pending.item_id == item_id)
        {
            self.output_page_pending = None;
            self.output_page_task = None;
            self.output_page_error = None;
        }
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
        let previous_patch = self.selected_change_patch();
        self.replace_diff_card_row(id, change);
        self.rebuild_changes(previous_patch);
    }

    /// Project restored file-change snapshots after a session load. Rows are
    /// staged first and the inspector projection rebuilds once, instead of
    /// once per change-set (each rebuild re-parses every diff patch).
    pub(super) fn project_restored_review(&mut self, items: &[vtcode_exec_events::ThreadItem]) {
        let previous_patch = self.selected_change_patch();
        let mut projected = false;
        for item in items {
            let vtcode_exec_events::ThreadItemDetails::FileChange(change) = &item.details else {
                continue;
            };
            self.drop_superseded_restored_item(&item.id);
            self.replace_diff_card_row(item.id.clone(), change);
            projected = true;
        }
        if projected {
            self.rebuild_changes(previous_patch);
        }
    }

    /// Stage one diff card row without rebuilding the inspector projection;
    /// the caller rebuilds once after staging a batch.
    fn replace_diff_card_row(&mut self, id: String, change: &vtcode_exec_events::FileChangeItem) {
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
        // The inline diff reads open by default; `upsert` preserves the
        // user's toggle on later updates to the same card.
        item.expanded = true;
        self.transcript.upsert(item);
    }

    /// Raw patch text behind the selected change, for viewer refresh detection.
    fn selected_change_patch(&self) -> Option<String> {
        self.selected_change
            .as_deref()
            .and_then(|path| self.changes.patch_for(&self.transcript, path))
    }

    /// Rebuild the inspector projection eagerly so renders never parse
    /// patches. Change-sets are rare; session loads stage rows first and call
    /// this once for the whole batch.
    fn rebuild_changes(&mut self, previous_patch: Option<String>) {
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
            if self.workspace_review.is_none() && self.current_route() == Route::Review {
                self.teardown_current_route();
                self.route_history.dismiss_to_workspace();
            }
        }
    }

    /// Open one changed file in the inspector's syntax-highlighted diff view.
    pub(super) fn select_change(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(file) = self.changes.select(&path) else {
            return;
        };
        let filter = self.change_kind_filter;
        let position = self
            .changes
            .entries()
            .iter()
            .enumerate()
            .filter(|(_, entry)| filter.is_none_or(|kind| kind == entry.kind))
            .position(|(_, entry)| entry.path == path);
        if let Some(position) = position {
            self.change_list_scroll.scroll_to_item(position, gpui_kit::ScrollStrategy::Top);
        }
        let mode = self
            .change_viewer
            .as_ref()
            .map(|viewer| viewer.read(cx).mode())
            .unwrap_or(DiffMode::Unified);
        self.selected_change = Some(path);
        self.change_viewer_dirty = false;
        self.change_viewer = Some(cx.new(|cx| DiffState::new(vec![file], cx).with_mode(mode)));
        self.open_inspector_tab(InspectorTab::Changes, cx);
    }

    pub(super) fn select_activity_output(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_output_id = Some(id);
        self.open_inspector_tab(InspectorTab::Output, cx);
        self.inspector_focus.focus(window, cx);
    }

    pub(super) fn navigate_change_file(&mut self, next: bool, cx: &mut Context<Self>) {
        let available = self
            .changes
            .entries()
            .iter()
            .filter(|entry| self.change_kind_filter.is_none_or(|kind| kind == entry.kind))
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
