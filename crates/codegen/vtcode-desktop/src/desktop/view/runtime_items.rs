//! Runtime item projection: ThreadItems into transcript rows, diff cards,
//! activity summaries, and bounded diff classification.

use super::super::output_sections::human_output_option;
use super::*;
use vtcode_commons::formatting::collapse_whitespace;
use vtcode_commons::tool_headlines::preview_full_command;

/// Retain a bounded, UTF-8-safe tail of live deltas. Completion replaces this
/// preview with the runtime's final snapshot, which can include a spool path.
pub(super) const LIVE_OUTPUT_LIMIT: usize = 64 * 1024;

pub(super) fn bounded_output_chunk(previous: &str, chunk: &str) -> String {
    let chunk_start = chunk.ceil_char_boundary(chunk.len().saturating_sub(LIVE_OUTPUT_LIMIT));
    let chunk = chunk.get(chunk_start..).unwrap_or_default();
    let previous_start = previous.ceil_char_boundary(previous.len().saturating_sub(LIVE_OUTPUT_LIMIT - chunk.len()));
    let mut output = String::with_capacity(previous.len() - previous_start + chunk.len());
    output.push_str(previous.get(previous_start..).unwrap_or_default());
    output.push_str(chunk);
    output
}

#[cfg(test)]
mod live_output_tests {
    use super::{bounded_output_chunk, describe_activity};
    use vtcode_exec_events::{
        CommandExecutionItem, CommandExecutionStatus, ThreadItemDetails, ToolCallStatus, ToolOutputItem, WebSearchItem,
    };

    #[test]
    fn live_output_chunks_keep_order_and_bound_utf8() {
        let first = bounded_output_chunk("", "first\n");
        assert_eq!(bounded_output_chunk(&first, "second"), "first\nsecond");
        assert_eq!(bounded_output_chunk("other", ""), "other");
        let large = format!("prefix{}end", "界".repeat(30_000));
        let tail = bounded_output_chunk("old", &large);
        assert!(tail.len() <= 64 * 1024);
        assert!(tail.ends_with("界end"));
        assert!(!tail.contains("prefix"));
    }

    #[test]
    fn failed_tool_output_and_command_report_exit_code_consistently() {
        let output = ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
            call_id: "call".into(),
            tool_call_id: None,
            spool_path: None,
            output: String::new(),
            is_delta: None,
            exit_code: Some(17),
            status: ToolCallStatus::Failed,
            app_previews: Vec::new(),
        }));
        let command = ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
            command: "false".into(),
            arguments: None,
            aggregated_output: String::new(),
            exit_code: Some(17),
            status: CommandExecutionStatus::Failed,
        }));
        assert_eq!(describe_activity(&output, None).1, "failed (exit 17)");
        assert_eq!(describe_activity(&command, None).1, "failed (exit 17)");
        let ThreadItemDetails::ToolOutput(mut output) = output else {
            panic!("expected tool output")
        };
        output.status = ToolCallStatus::Completed;
        output.exit_code = Some(0);
        assert_eq!(describe_activity(&ThreadItemDetails::ToolOutput(output), None).1, "completed");
    }

    #[test]
    fn blank_commands_fall_back_to_cli_placeholder_title() {
        for blank in ["", "   "] {
            let command = ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                command: blank.into(),
                arguments: None,
                aggregated_output: String::new(),
                exit_code: None,
                status: CommandExecutionStatus::Completed,
            }));
            assert_eq!(describe_activity(&command, None).0, "Ran command");
        }
    }

    #[test]
    fn multi_line_queries_collapse_to_single_line_titles() {
        // Titles render in a single-line slot: a multi-line query collapses
        // to one line instead of overflowing its row with a bare middle line
        // and ghosted neighbors (the 2026-11-09 session failure). Commands
        // preview to one line upstream (`gist_previews_multiline_commands`);
        // this pins the collapse for sources that carry raw text.
        let search = ThreadItemDetails::WebSearch(Box::new(WebSearchItem {
            query: "first line\n\tsecond line".into(),
            provider: None,
            results: None,
        }));
        assert_eq!(describe_activity(&search, None).0, "first line second line");
    }

    #[test]
    fn completed_commands_preview_like_invocation_gists() {
        // A pending row and its completed row title identically: multi-line
        // commands preview to the first line on both paths.
        let execution = ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
            command: "sed -n '1p' README.md\necho done".into(),
            arguments: None,
            aggregated_output: String::new(),
            exit_code: Some(0),
            status: CommandExecutionStatus::Completed,
        }));
        assert_eq!(describe_activity(&execution, None).0, "Ran sed -n '1p' README.md");
    }
}

impl DesktopView {
    /// Resolve a live tool output to its invocation row when identities are unambiguous.
    ///
    /// Merges into any tool row the `call_id` names — shell, MCP, read, edit,
    /// or search — so one tool call renders as exactly one row. A row that
    /// already absorbed a different capture keeps it (the newcomer stands
    /// alone) and non-tool rows never absorb output.
    pub(super) fn runtime_output_row_id<'a>(&self, item: &'a vtcode_exec_events::ThreadItem) -> &'a str {
        let vtcode_exec_events::ThreadItemDetails::ToolOutput(output) = &item.details else {
            return &item.id;
        };
        if self.transcript.position_by_id(&item.id).is_some() {
            return &item.id;
        }
        let Some(row) = self
            .transcript
            .position_by_id(&output.call_id)
            .and_then(|index| self.transcript.get(index))
        else {
            return &item.id;
        };
        if row.is_tool() && row.output_item_id.as_deref().is_none_or(|id| id == item.id) {
            &output.call_id
        } else {
            &item.id
        }
    }

    pub(super) fn restore_review_summaries(&mut self, events: &[vtcode_exec_events::VersionedThreadEvent]) {
        let mut completions: HashMap<&str, usize> = HashMap::new();
        let mut item_turns: HashMap<&str, Option<&str>> = HashMap::new();
        for event in events {
            match event.event() {
                vtcode_exec_events::ThreadEvent::TurnCompleted(completed) => {
                    if let Some(id) = completed.final_response_item_id.as_deref() {
                        *completions.entry(id.as_str()).or_default() += 1;
                    }
                }
                vtcode_exec_events::ThreadEvent::ItemCompleted(completed)
                    if matches!(completed.item.details, vtcode_exec_events::ThreadItemDetails::AgentMessage(_)) =>
                {
                    use std::collections::hash_map::Entry;
                    let turn = completed
                        .item
                        .context
                        .as_deref()
                        .map(|context| context.turn_id.as_str())
                        .filter(|id| !id.is_empty());
                    match item_turns.entry(&completed.item.id) {
                        Entry::Vacant(entry) => {
                            let _stored = entry.insert(turn);
                        }
                        Entry::Occupied(mut entry) => {
                            let _previous = entry.insert(None);
                        }
                    }
                }
                _ => {}
            }
        }
        let mut occurrences: HashMap<&str, usize> = HashMap::new();
        for item in self.transcript.items() {
            if let Some(id) = item.canonical_item_id.as_deref() {
                *occurrences.entry(id).or_default() += 1;
            }
        }
        let matches: Vec<_> = self
            .transcript
            .items()
            .filter_map(|item| {
                let id = item.canonical_item_id.as_deref()?;
                let turn = (*item_turns.get(id)?)?;
                (occurrences.get(id) == Some(&1) && completions.get(id) == Some(&1))
                    .then(|| (item.id.clone(), id.to_owned(), turn.to_owned()))
            })
            .collect();
        for (row_id, item_id, turn_id) in matches {
            self.transcript.restore_archived_turn_files(
                &row_id,
                &item_id,
                &turn_id,
                self.restored_review_history_incomplete,
            );
        }
    }

    pub(super) fn restore_completion_times(&mut self, events: &[vtcode_exec_events::VersionedThreadEvent]) {
        let mut times: HashMap<&str, Option<String>> = HashMap::new();
        for event in events {
            if let vtcode_exec_events::ThreadEvent::TurnCompleted(completed) = event.event()
                && let Some(id) = &completed.final_response_item_id
            {
                use std::collections::hash_map::Entry;
                match times.entry(id.as_str()) {
                    Entry::Vacant(entry) => {
                        let time = completed
                            .completed_at
                            .as_deref()
                            .and_then(|timestamp| chrono::DateTime::parse_from_rfc3339(timestamp).ok())
                            .map(|timestamp| timestamp.with_timezone(&chrono::Local).format("%H:%M").to_string());
                        let _stored = entry.insert(time);
                    }
                    Entry::Occupied(mut entry) => {
                        let _previous = entry.insert(None);
                    }
                }
            }
        }
        let mut occurrences: HashMap<&str, usize> = HashMap::new();
        for item in self.transcript.items() {
            if let Some(id) = item.canonical_item_id.as_deref() {
                *occurrences.entry(id).or_default() += 1;
            }
        }
        let matches: Vec<_> = self
            .transcript
            .items()
            .filter_map(|item| {
                let id = item.canonical_item_id.as_deref()?;
                let time = times.get(id)?.as_ref()?;
                (occurrences.get(id) == Some(&1)).then(|| (item.id.clone(), id.to_owned(), time.clone()))
            })
            .collect();
        for (row_id, item_id, time) in matches {
            self.transcript.set_archived_completion_time(&row_id, &item_id, time.into());
        }
    }

    /// Seed the Tasks viewer from the newest `tracker.updated` checkpoint in
    /// the restored timeline. Later live updates replace it; sessions without
    /// tracker activity keep the empty state.
    pub(super) fn restore_tracker_snapshot(&mut self, events: &[vtcode_exec_events::VersionedThreadEvent]) {
        self.tracker_snapshot = events.iter().rev().find_map(|event| match event.event() {
            vtcode_exec_events::ThreadEvent::TrackerUpdated(snapshot) => Some(snapshot.as_ref().clone()),
            _ => None,
        });
    }

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

    /// Effective CLI communication style for diagnostic projection. Older
    /// services omit the projection; those fall back to `Balanced`, matching
    /// the CLI default. Unknown future values resolve the same way.
    fn communication_style(&self) -> vtcode_commons::ui_protocol::CommunicationStyle {
        self.configuration
            .as_ref()
            .and_then(|configuration| configuration.shared_ui.as_ref())
            .map(|shared| vtcode_commons::ui_protocol::CommunicationStyle::parse(&shared.communication_style))
            .unwrap_or_default()
            .resolve()
    }

    pub(super) fn update_runtime_item_for_turn(
        &mut self,
        item: &vtcode_exec_events::ThreadItem,
        turn_id: Option<&str>,
    ) {
        let context_turn = item
            .context
            .as_deref()
            .map(|context| context.turn_id.as_str())
            .filter(|id| !id.is_empty());
        let row_id = self.runtime_output_row_id(item).to_owned();
        if !self.transcript.accepts_item_turn(&item.id, turn_id) || !self.transcript.accepts_item_turn(&row_id, turn_id)
        {
            // Conflicting turn identity: the stale item is dropped.
            return;
        }
        self.update_runtime_item(item);
        if let Some(turn_id) = turn_id {
            self.transcript.set_item_turn(&row_id, turn_id);
        }
        if let Some(context_turn) = context_turn {
            self.transcript.set_item_canonical_turn(&row_id, context_turn);
        }
    }

    /// Drop restored projections superseded by a newer item with the same
    /// identity: its historical activity row, output cursors/pages, and any
    /// pending output page. Shared by live updates and session-load batches.
    fn drop_superseded_restored_item(&mut self, item_id: &str) {
        let capture_ids: Vec<String> = self
            .restored_activity
            .iter()
            .filter(|row| row.id == item_id || row.output_item_id.as_deref() == Some(item_id))
            .filter_map(|row| row.output_item_id.clone())
            .collect();
        self.retain_restored_activity(|restored| {
            restored.id.as_str() != item_id && restored.output_item_id.as_deref() != Some(item_id)
        });
        let _cursor = self.restored_output_cursors.remove(item_id);
        let _pages = self.restored_output_pages.remove(item_id);
        for capture_id in &capture_ids {
            let _cursor = self.restored_output_cursors.remove(capture_id);
            let _pages = self.restored_output_pages.remove(capture_id);
        }
        if self
            .output_page_pending
            .as_ref()
            .is_some_and(|pending| pending.item_id == item_id || capture_ids.contains(&pending.item_id))
        {
            self.output_page_pending = None;
            self.output_page_task = None;
            self.output_page_error = None;
        }
    }

    pub(super) fn project_runtime_item(&mut self, item: &vtcode_exec_events::ThreadItem, historical: bool) {
        use vtcode_exec_events::ThreadItemDetails;
        // Routine harness telemetry never reaches the transcript (live or
        // restored): latency records and lifecycle chatter would drown the
        // conversation. Failures and blocks stay visible; see the policy.
        if let ThreadItemDetails::Harness(event) = &item.details
            && !harness_is_user_visible(&event.event)
        {
            return;
        }
        if historical && let ThreadItemDetails::ToolOutput(output) = &item.details {
            let incoming_turn = item
                .context
                .as_deref()
                .map(|context| context.turn_id.as_str())
                .filter(|id| !id.is_empty());
            let index = self.restored_activity.iter().position(|row| {
                row.id == output.call_id
                    && row.is_tool()
                    && row.output_item_id.as_deref().is_none_or(|id| id == item.id)
                    && !matches!((row.canonical_turn_id.as_deref(), incoming_turn), (Some(a), Some(b)) if a != b)
            });
            if let Some(index) = index {
                let (_, mut status) = describe_activity(&item.details, None);
                if ActivityState::from_status(&status) == ActivityState::Running {
                    status = tool_status::INTERRUPTED.to_string();
                }
                let Some(row) = self.restored_activity.get_mut(index) else {
                    return;
                };
                let payload = activity_payload(&row.text);
                row.text = activity_text(&status, payload).into();
                // Identity implies output: `output_for` resolves a selection by
                // identity and falls back to the newest capture when the row is
                // output-less, so a blank merge must not plant an identity.
                if let Some(human) = human_output_option(&output.output) {
                    row.output = Some(human.into());
                    row.output_item_id = Some(item.id.clone());
                } else {
                    row.output = None;
                }
                row.app_previews.clone_from(&output.app_previews);
                self.bump_restored_activity_revision();
                return;
            }
        }
        if !historical && let ThreadItemDetails::ToolOutput(output) = &item.details {
            let row_id = self.runtime_output_row_id(item).to_owned();
            if row_id != item.id
                && let Some(row) = self
                    .transcript
                    .position_by_id(&row_id)
                    .and_then(|index| self.transcript.get(index))
            {
                let title = row.title.to_string();
                let payload = activity_payload(&row.text);
                let status = describe_activity(&item.details, None).1;
                let text = activity_text(&status, payload);
                self.append_or_update_item(row_id.clone(), TranscriptRole::Tool.as_str(), &title, &text, true);
                // Identity implies output (see the historical merge above): a
                // blank capture stores no output and plants no identity.
                if let Some(human) = human_output_option(&output.output) {
                    self.transcript.set_output(&row_id, Some(human.into()));
                    self.transcript.set_output_identity(&row_id, &item.id);
                } else {
                    self.transcript.set_output(&row_id, None);
                }
                if !output.app_previews.is_empty() {
                    self.transcript.set_app_previews(&row_id, output.app_previews.clone());
                }
                return;
            }
        }
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
                if let Some(diagnostic) = reasoning.diagnostic.as_deref() {
                    // Routine agent-internal failures stay out of the desktop
                    // transcript under the default style; the canonical event
                    // still carries the complete evidence for forensics. A
                    // silent update also clears a stale visible row so an
                    // incident can never linger after its class goes silent.
                    if !vtcode_commons::diagnostics::diagnostic_is_user_visible(
                        self.communication_style(),
                        &diagnostic.code,
                    ) {
                        self.transcript.remove(&item.id);
                        return;
                    }
                    let severity = match diagnostic.severity {
                        vtcode_exec_events::DiagnosticSeverity::Error => NoticeSeverity::Error,
                        vtcode_exec_events::DiagnosticSeverity::Warning => NoticeSeverity::Warning,
                        vtcode_exec_events::DiagnosticSeverity::Info => NoticeSeverity::Info,
                    };
                    self.transcript.upsert(TranscriptItem::diagnostic_notice(
                        id,
                        diagnostic.summary.as_str().into(),
                        diagnostic.agent_details.clone(),
                        severity,
                    ));
                } else {
                    let title = rust_i18n::t!("app.transcript.planning_update").to_string();
                    self.append_or_update_item(id, TranscriptRole::Assistant.as_str(), &title, &reasoning.text, false);
                }
            }
            ThreadItemDetails::Decision(decision) => {
                let title = rust_i18n::t!("app.transcript.decision_title").to_string();
                self.append_or_update_item(id, TranscriptRole::Assistant.as_str(), &title, &decision.summary, false);
            }
            ThreadItemDetails::FileChange(change) => {
                // The emitter ids this `{call_id}:changes`: stamp per-call edit
                // stats onto the sibling invocation row when counts are present.
                let stamp = match (change.additions, change.deletions) {
                    (Some(additions), Some(deletions)) => {
                        id.strip_suffix(":changes").map(|stem| (stem.to_string(), additions, deletions))
                    }
                    _ => None,
                };
                self.upsert_diff_card(id, change);
                if let Some((stem, additions, deletions)) = stamp {
                    self.transcript.stamp_edit_totals(&stem, additions, deletions);
                }
            }
            details => {
                let (title, mut status) = describe_activity(details, self.workspace.as_deref());
                if historical && ActivityState::from_status(&status) == ActivityState::Running {
                    status = tool_status::INTERRUPTED.to_string();
                }
                let payload = activity_details_summary(details, self.workspace.as_deref());
                let text = activity_text(&status, &payload);
                // Keep the human-projected output for the inspector's Output
                // tab; it updates in place as streaming events arrive.
                let output = activity_output_text(details).map(gpui_kit::SharedString::from);
                let app_previews = match details {
                    ThreadItemDetails::ToolOutput(tool_output) => tool_output.app_previews.clone(),
                    _ => Vec::new(),
                };
                if historical {
                    let turn_id = item.context.as_deref().map(|context| context.turn_id.clone().into());
                    let mut item =
                        TranscriptItem::new(id, TranscriptRole::Tool.as_str().to_string(), title.into(), text);
                    item.canonical_turn_id.clone_from(&turn_id);
                    item.turn_id = turn_id;
                    item.output = output;
                    item.app_previews = app_previews;
                    item.activity_category = ToolCategory::from_details(details);
                    item.tool_name = invocation_tool_name(details).map(Into::into);
                    self.push_restored_activity(item);
                } else {
                    // Carry stamped edit totals across re-projections: the fresh
                    // gist never carries the suffix, so suffixing here cannot
                    // double-append.
                    let carried = self
                        .transcript
                        .position_by_id(&id)
                        .and_then(|index| self.transcript.get(index))
                        .and_then(|row| row.edit_totals);
                    let title = match carried {
                        Some(EditTotals { additions, deletions }) => {
                            Transcript::title_with_edit_stats(&title, additions, deletions)
                        }
                        None => title,
                    };
                    self.append_or_update_item(id.clone(), TranscriptRole::Tool.as_str(), &title, &text, true);
                    if output.is_some() {
                        self.transcript.set_output(&id, output);
                    }
                    if !app_previews.is_empty() {
                        self.transcript.set_app_previews(&id, app_previews);
                    }
                    if let Some(category) = ToolCategory::from_details(details) {
                        self.transcript.set_activity_category(&id, category);
                    }
                    if let Some(tool_name) = invocation_tool_name(details) {
                        self.transcript.set_tool_name(&id, tool_name);
                    }
                    if let Some(EditTotals { additions, deletions }) = carried {
                        self.transcript.set_edit_totals(&id, additions, deletions);
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
            if let Some(context) = item.context.as_deref() {
                self.transcript.set_item_turn(&item.id, &context.turn_id);
                self.transcript.set_item_canonical_turn(&item.id, &context.turn_id);
            }
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
        // New cards arrive collapsed: the header (files, +/− counts, apply
        // status) summarizes the change, and the patch expands on demand.
        // `upsert` preserves the user's toggle on later updates to the card.
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
            .unwrap_or_else(|| self.preferences.diff_mode());
        self.selected_change = Some(path);
        self.change_viewer_dirty = false;
        self.change_viewer =
            Some(cx.new(|cx| diff_preferences::new_diff_state(vec![file], mode, &self.preferences.diff, cx)));
        self.open_inspector_tab(InspectorTab::Changes, cx);
    }

    /// Inspector diff layout toggle: updates the live viewer and records the
    /// shared viewer default so relaunches keep the user's choice.
    pub(super) fn set_inspector_diff_mode(&mut self, mode: DiffMode, cx: &mut Context<Self>) {
        if let Some(viewer) = self.change_viewer.as_ref() {
            viewer.update(cx, |viewer, cx| viewer.set_mode(mode, cx));
        }
        self.preferences.set_diff_mode(mode);
        self.preferences.save();
        cx.notify();
    }

    pub(super) fn select_activity_output(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let capture_id = self
            .restored_activity
            .iter()
            .chain(self.transcript.items())
            .find(|row| row.id == id)
            .and_then(|row| row.output_item_id.clone())
            .unwrap_or(id);
        self.selected_output_id = Some(capture_id);
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

/// Raw invoking tool name for rows that have one, kept verbatim for search
/// keywords. Executions and outputs carry no tool name of their own.
fn invocation_tool_name(details: &vtcode_exec_events::ThreadItemDetails) -> Option<&str> {
    match details {
        vtcode_exec_events::ThreadItemDetails::ToolInvocation(tool) => Some(tool.tool_name.as_str()),
        vtcode_exec_events::ThreadItemDetails::McpToolCall(call) => Some(call.tool_name.as_str()),
        _ => None,
    }
}

/// Human title and lifecycle summary for a non-message activity item.
///
/// Titles render in a single-line slot, but agent commands, queries, and
/// messages routinely span lines — so the title collapses here, once, rather
/// than in each reader. (A multi-line title used to overflow its row,
/// clipping to a bare middle line with ghosted neighbors and hiding the
/// `Ran` prefix on its clipped first line.)
fn describe_activity(
    details: &vtcode_exec_events::ThreadItemDetails,
    workspace_root: Option<&Path>,
) -> (String, String) {
    use vtcode_exec_events::ThreadItemDetails;
    let (title, status) = match details {
        ThreadItemDetails::ToolInvocation(tool) => {
            (tool_row_gist(tool, workspace_root), tool_status_label(&tool.status))
        }
        ThreadItemDetails::ToolOutput(output) => {
            let label = status_with_exit_code(tool_status_label(&output.status), output.exit_code);
            (rust_i18n::t!("app.transcript.tool_output_title").to_string(), label)
        }
        ThreadItemDetails::CommandExecution(command) => {
            let label = status_with_exit_code(command_status_label(&command.status), command.exit_code);
            // Blank commands are degenerate (real executions always name one);
            // fall back to the CLI's own placeholder instead of a "Ran " stub.
            // Non-blank commands preview like invocation gists so a pending
            // row and its completed row title identically.
            let title = if command.command.trim().is_empty() {
                "Ran command".to_string()
            } else {
                format!("Ran {}", preview_full_command(&command.command))
            };
            (title, label)
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
    };
    (collapse_whitespace(&title), status)
}

fn status_with_exit_code(status: String, exit_code: Option<i32>) -> String {
    if status == tool_status::FAILED
        && let Some(code) = exit_code
    {
        format!("{}{code})", tool_status::FAILED_EXIT_PREFIX)
    } else {
        status
    }
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

/// Transcript-visible lines of a unified diff: git framing (`diff --git`,
/// `---`/`+++`) is skipped because the card's file rows already name each
/// path, and only yielded lines count toward [`MAX_DIFF_LINES`].
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
        loop {
            if self.remaining == 0 {
                if !self.checked_limit {
                    // Framing past the budget must not mark a fully-rendered
                    // patch truncated: only visible lines count.
                    self.truncated = self.lines.clone().any(|text| classify_diff_line(text) != DiffLineKind::Header);
                    self.checked_limit = true;
                }
                return None;
            }

            let Some(text) = self.lines.next() else {
                self.remaining = 0;
                self.checked_limit = true;
                return None;
            };
            let kind = classify_diff_line(text);
            if kind == DiffLineKind::Header {
                continue;
            }
            self.remaining -= 1;
            return Some(DiffLine { kind, text });
        }
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
