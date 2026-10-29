//! Session lifecycle: change guards, load/restore, history and output paging.

use super::*;

/// Retain the exact submitted content until the service accepts it.
pub(super) struct SubmissionDraft {
    pub(super) prompt: String,
    pub(super) attachments: Vec<protocol::TurnAttachment>,
    pub(super) echo_id: String,
    pub(super) delivery_uncertain: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PendingOutputPage {
    pub(super) request_id: String,
    pub(super) session_id: String,
    pub(super) item_id: String,
    pub(super) offset: u64,
}

pub(super) fn valid_session_output_page(
    page: &SessionOutputPage,
    pending: &PendingOutputPage,
    cursor: &SessionOutputCursor,
) -> bool {
    let Some(byte_delta) = page.next_offset.checked_sub(page.offset) else {
        return false;
    };
    page.session_id == pending.session_id
        && page.request_id == pending.request_id
        && page.item_id == pending.item_id
        && page.offset == pending.offset
        && page.total_bytes == cursor.total_bytes
        && page.offset == cursor.next_offset
        && page.next_offset > page.offset
        && page.next_offset <= page.total_bytes
        && byte_delta <= protocol::MAX_SESSION_OUTPUT_PAGE_BYTES as u64
        && page.text.len() <= protocol::MAX_SESSION_OUTPUT_PAGE_BYTES
        && page.has_more == (page.next_offset < page.total_bytes)
        && byte_delta == page.text.len() as u64
}

pub(super) fn append_restored_output_page(
    item: &mut TranscriptItem,
    pages_by_item: &mut HashMap<String, Vec<gpui_kit::SharedString>>,
    page: &mut SessionOutputPage,
) -> bool {
    let Some(current_output) = item.output.as_ref() else {
        return false;
    };
    let current_preview = current_output
        .strip_suffix(protocol::RESTORED_OUTPUT_TRUNCATION_SUFFIX)
        .unwrap_or(current_output.as_ref());
    let pages = pages_by_item
        .entry(item.id.clone())
        .or_insert_with(|| vec![current_preview.into()]);

    let mut next_preview = String::with_capacity(
        current_preview
            .len()
            .saturating_add(page.text.len())
            .saturating_add(protocol::RESTORED_OUTPUT_TRUNCATION_SUFFIX.len()),
    );
    next_preview.push_str(current_preview);
    next_preview.push_str(&page.text);
    let mut next_preview = inspector::output_tail(&next_preview.into()).to_string();
    if page.has_more {
        next_preview.push_str(protocol::RESTORED_OUTPUT_TRUNCATION_SUFFIX);
    }
    item.output = Some(next_preview.into());
    pages.push(std::mem::take(&mut page.text).into());
    true
}

pub(super) fn restored_output_for_copy(
    item_id: &str,
    preview: &gpui_kit::SharedString,
    pages_by_item: &HashMap<String, Vec<gpui_kit::SharedString>>,
    has_more: bool,
) -> String {
    let Some(pages) = pages_by_item.get(item_id) else {
        return preview.to_string();
    };
    let capacity = pages.iter().fold(0usize, |length, page| length.saturating_add(page.len()));
    let mut output = String::with_capacity(capacity);
    for page in pages {
        output.push_str(page);
    }
    if has_more {
        output.push_str(protocol::RESTORED_OUTPUT_TRUNCATION_SUFFIX);
    }
    output
}

pub(super) struct ActivePrimaryAgentDisplay {
    pub(super) session_id: String,
    pub(super) display_name: String,
}

impl DesktopView {
    pub(super) fn session_change_idle(&self) -> bool {
        self.workspace_switch_idle() && self.backend.is_some() && self.interaction.is_none()
    }

    pub(super) fn session_change_allowed(&self, reload_session_id: Option<&str>) -> bool {
        self.session_change_idle()
            && (self.retained_submission.is_none()
                || reload_session_id.is_some_and(|id| self.selected_session_id.as_deref() == Some(id)))
    }

    pub(super) fn request_session_change(
        &mut self,
        method: &'static str,
        params: Value,
        cx: &mut Context<Self>,
    ) -> bool {
        let reload_session_id = (method == protocol::method::SESSION_LOAD)
            .then(|| params.get(keys::SESSION_ID).and_then(Value::as_str))
            .flatten();
        if !self.session_change_allowed(reload_session_id) {
            if self.retained_submission.is_some() && self.session_change_idle() {
                self.error = Some(UNSENT_MESSAGE_BLOCKS_SESSION_CHANGE.to_string());
                cx.notify();
            }
            return false;
        }
        self.clear_error();
        let sent = self.request(method, params);
        if sent {
            self.session_request = Some(method);
            self.status = "Updating session…".to_string();
        } else {
            self.error = Some(REQUEST_QUEUE_FULL_MESSAGE.to_string());
        }
        cx.notify();
        sent
    }

    pub(super) fn begin_new_thread(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.session_change_allowed(None) {
            if self.retained_submission.is_some() && self.session_change_idle() {
                self.error = Some(UNSENT_MESSAGE_BLOCKS_SESSION_CHANGE.into());
                cx.notify();
            }
            return false;
        }
        if let Some(session_id) = self.selected_session_id.clone() {
            if !self.request_session_change(
                protocol::method::SESSION_CLOSE,
                json!({ keys::SESSION_ID: session_id }),
                cx,
            ) {
                return false;
            }
            self.pending_new_thread = true;
        } else {
            let attachments = std::mem::take(&mut self.attachments);
            self.reset_session_state();
            self.attachments = attachments;
            self.clear_error();
            self.record_current_navigation();
        }
        self.status = if self.pending_new_thread {
            rust_i18n::t!("app.session.preparing_new_thread").to_string()
        } else {
            "New thread ready. Submit a prompt to begin.".into()
        };
        cx.notify();
        true
    }

    pub(super) fn create_session(&mut self, cx: &mut Context<Self>) {
        let _started = self.begin_new_thread(cx);
    }

    pub(super) fn load_session(&mut self, session_id: String, cx: &mut Context<Self>) {
        let _sent =
            self.request_session_change(protocol::method::SESSION_LOAD, json!({ keys::SESSION_ID: session_id }), cx);
    }

    pub(super) fn request_earlier_history(&mut self, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected_session_id.clone() else {
            return;
        };
        if !self.history_has_earlier || self.running || self.history_page_loading {
            return;
        }
        let limit = self
            .transcript
            .remaining_capacity()
            .min(protocol::MAX_SESSION_HISTORY_PAGE_MESSAGES);
        if limit == 0 {
            return;
        }
        let request_id = uuid_like_id();
        if self.request(
            protocol::method::SESSION_HISTORY,
            json!({
                keys::SESSION_ID: session_id,
                keys::REQUEST_ID: request_id,
                keys::BEFORE_INDEX: self.history_start_index,
                keys::LIMIT: limit,
            }),
        ) {
            self.history_page_generation = self.history_page_generation.wrapping_add(1);
            self.history_page_request_id = Some(request_id);
            self.history_page_limit = limit;
            self.history_page_loading = true;
            self.history_page_error = None;
        } else {
            self.history_page_error = Some(REQUEST_QUEUE_FULL_MESSAGE.into());
        }
        cx.notify();
    }

    pub(super) fn fail_history_page(&mut self, error: String, cx: &mut Context<Self>) {
        self.history_page_generation = self.history_page_generation.wrapping_add(1);
        self.history_page_request_id = None;
        self.history_page_limit = 0;
        self.history_page_task = None;
        self.history_page_loading = false;
        self.history_page_error = Some(error);
        cx.notify();
    }

    pub(super) fn request_more_captured_output(&mut self, item_id: &str, cx: &mut Context<Self>) {
        if self.output_page_pending.is_some() {
            return;
        }
        let Some(session_id) = self.selected_session_id.clone() else {
            return;
        };
        let Some(cursor) = self.restored_output_cursors.get(item_id) else {
            return;
        };
        let pending = PendingOutputPage {
            request_id: uuid_like_id(),
            session_id,
            item_id: item_id.to_string(),
            offset: cursor.next_offset,
        };
        if self.request(
            protocol::method::SESSION_OUTPUT_READ,
            json!({
                keys::SESSION_ID: pending.session_id,
                keys::REQUEST_ID: pending.request_id,
                keys::ITEM_ID: pending.item_id,
                keys::OFFSET: pending.offset,
            }),
        ) {
            self.output_page_pending = Some(pending);
            self.output_page_error = None;
        } else {
            self.output_page_error = Some((item_id.to_string(), REQUEST_QUEUE_FULL_MESSAGE.into()));
        }
        cx.notify();
    }

    pub(super) fn fail_output_page(&mut self, cx: &mut Context<Self>) {
        let item_id = self.output_page_pending.take().map(|pending| pending.item_id);
        self.output_page_task = None;
        self.output_page_error =
            item_id.map(|item_id| (item_id, "Could not load more captured output. Try again.".into()));
        cx.notify();
    }

    pub(super) fn begin_session_output_page(&mut self, result: Value, cx: &mut Context<Self>) {
        let Some(pending) = self.output_page_pending.clone() else {
            return;
        };
        self.output_page_task = Some(cx.spawn(async move |view, cx| {
            let page = cx
                .background_spawn(async move { serde_json::from_value::<SessionOutputPage>(result) })
                .await;
            let _updated = view.update(cx, |view, cx| {
                if view.output_page_pending.as_ref() != Some(&pending) {
                    return;
                }
                if let Some(task) = view.output_page_task.take() {
                    task.detach();
                }
                let Ok(mut page) = page else {
                    view.fail_output_page(cx);
                    return;
                };
                let Some(cursor) = view.restored_output_cursors.get(&pending.item_id).cloned() else {
                    view.fail_output_page(cx);
                    return;
                };
                if view.selected_session_id.as_deref() != Some(pending.session_id.as_str())
                    || !valid_session_output_page(&page, &pending, &cursor)
                {
                    view.fail_output_page(cx);
                    return;
                }

                let Some(item) = view.restored_activity.iter_mut().find(|item| item.id == pending.item_id) else {
                    view.fail_output_page(cx);
                    return;
                };
                let updated_output = append_restored_output_page(item, &mut view.restored_output_pages, &mut page);
                if !updated_output {
                    view.fail_output_page(cx);
                    return;
                }

                view.output_page_pending = None;
                view.output_page_error = None;
                if page.has_more {
                    let _previous = view.restored_output_cursors.insert(
                        pending.item_id,
                        SessionOutputCursor {
                            item_id: page.item_id,
                            next_offset: page.next_offset,
                            total_bytes: page.total_bytes,
                        },
                    );
                } else {
                    let _previous = view.restored_output_cursors.remove(&pending.item_id);
                }
                view.refresh_conversation_search(&pending.session_id);
                cx.notify();
            });
        }));
    }

    /// Borrowed CLI session options, once the service has discovered them.
    pub(super) fn session_options(&self) -> Option<&protocol::SessionOptionsSnapshot> {
        self.configuration.as_ref().and_then(|config| config.session_options.as_ref())
    }

    /// Refresh the cached session options after a session boundary while Full
    /// access was active. The mode is session-only: the service restores the
    /// prior mode, so the client re-discovers to stay in sync.
    pub(super) fn refresh_session_options_after_full_access(&self) {
        if self
            .session_options()
            .is_some_and(|options| options.permission_mode == protocol::PermissionMode::FullAccess)
        {
            let _requested = self.request(protocol::method::CONFIG_DISCOVER, json!({}));
        }
    }

    /// Decode and prepare bounded historical content away from the UI thread.
    pub(super) fn begin_session_load(&mut self, result: Value, cx: &mut Context<Self>) {
        if self.session_request != Some(protocol::method::SESSION_LOAD) {
            return;
        }
        self.session_load_generation = self.session_load_generation.wrapping_add(1);
        let generation = self.session_load_generation;
        self.status = "Restoring conversation…".to_string();
        self.session_load_task = Some(cx.spawn(async move |view, cx| {
            let loaded = cx
                .background_spawn(async move {
                    serde_json::from_value::<SessionLoadResult>(result).map(|mut loaded| {
                        let history =
                            prepare_session_history(&loaded.session.session_id, std::mem::take(&mut loaded.messages));
                        (loaded, history)
                    })
                })
                .await;
            let _updated = view.update(cx, |view, cx| {
                if view.session_load_generation != generation
                    || view.session_request != Some(protocol::method::SESSION_LOAD)
                {
                    return;
                }
                if let Some(task) = view.session_load_task.take() {
                    // The completion callback is running inside this task; let it return normally.
                    task.detach();
                }
                view.session_request = None;
                match loaded {
                    Ok((loaded, history)) => view.accept_loaded_session(loaded, history, cx),
                    Err(_) => {
                        view.fail_pending_navigation();
                        view.status = "Session load failed".to_string();
                        view.error = Some("The local service returned an invalid session load result.".into());
                    }
                }
                view.event_tx.wake();
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Decode and prepare one bounded archive-history page off the UI thread.
    pub(super) fn begin_session_history_page(&mut self, result: Value, cx: &mut Context<Self>) {
        let Some(request_id) = self.history_page_request_id.clone() else {
            return;
        };
        let Some(session_id) = self.selected_session_id.clone() else {
            return;
        };
        let generation = self.history_page_generation;
        let before_index = self.history_start_index;
        let limit = self.history_page_limit;
        self.history_page_task = Some(cx.spawn(async move |view, cx| {
            let page = cx
                .background_spawn(async move {
                    serde_json::from_value::<SessionHistoryPage>(result).map(|mut page| {
                        let valid_indices = page.messages.iter().enumerate().all(|(offset, message)| {
                            page.start_index.checked_add(offset) == message.archive_index
                        });
                        let valid = !page.messages.is_empty()
                            && page.messages.len() <= limit
                            && page.start_index < before_index
                            && valid_indices
                            && page.start_index.checked_add(page.messages.len()) == Some(before_index)
                            && page.has_earlier == (page.start_index > 0);
                        valid.then(|| {
                            let history = prepare_session_history(&page.session_id, std::mem::take(&mut page.messages));
                            (page, history)
                        })
                    })
                })
                .await;
            let _updated = view.update(cx, |view, cx| {
                if view.history_page_generation != generation
                    || view.history_page_request_id.as_deref() != Some(request_id.as_str())
                {
                    return;
                }
                if let Some(task) = view.history_page_task.take() {
                    task.detach();
                }
                view.history_page_request_id = None;
                view.history_page_limit = 0;
                view.history_page_loading = false;
                match page {
                    Ok(Some((page, history)))
                        if page.request_id == request_id
                            && page.session_id == session_id
                            && view.selected_session_id.as_deref() == Some(session_id.as_str()) =>
                    {
                        let visible_count = history.len();
                        let inserted = view.transcript.prepend_prepared(history);
                        if visible_count > 0 && inserted == 0 {
                            view.history_page_error = Some(
                                "The transcript reached its local message limit. Start a new session to continue browsing.".into(),
                            );
                        } else {
                            view.history_start_index = page.start_index;
                            view.history_has_earlier = page.has_earlier;
                            view.history_page_error = None;
                        }
                    }
                    Ok(_) => {
                        view.history_page_error = Some("The local service returned an invalid history page.".into());
                    }
                    Err(_) => {
                        view.history_page_error = Some("The local service returned an invalid history page.".into());
                    }
                }
                view.refresh_conversation_search(&session_id);
                cx.notify();
            });
        }));
    }

    fn accept_loaded_session(
        &mut self,
        loaded: SessionLoadResult,
        history: Vec<TranscriptItem>,
        cx: &mut Context<Self>,
    ) {
        if loaded.session.session_id.trim().is_empty() {
            self.fail_pending_navigation();
            self.status = "Session load failed".to_string();
            self.error = Some("The local service returned an invalid session load result.".into());
            return;
        }
        if let Some(pending) = self.pending_navigation
            && self
                .navigation_history
                .location(pending.target_index)
                .and_then(|location| location.session_id.as_deref())
                != Some(loaded.session.session_id.as_str())
        {
            self.error = Some("The service returned a different thread than the navigation target.".into());
            self.fail_pending_navigation();
            return;
        }
        if self.retained_submission.is_some()
            && self.selected_session_id.as_deref() != Some(loaded.session.session_id.as_str())
        {
            self.error = Some("The service returned a different session; the unsent message was retained.".to_string());
            return;
        }

        let retained_submission = self.retained_submission.take();
        self.load_history(history);
        self.history_start_index = loaded.history_start_index;
        self.history_has_earlier = loaded.history_has_earlier && loaded.history_start_index > 0;
        self.restored_review_history_incomplete = loaded.review_history_incomplete;
        self.restored_activity_history_incomplete = loaded.activity_history_incomplete;
        let output_item_ids = loaded
            .activity_items
            .iter()
            .filter(|item| matches!(&item.details, vtcode_exec_events::ThreadItemDetails::ToolOutput(_)))
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>();
        self.restored_output_cursors.clear();
        self.restored_output_pages.clear();
        for cursor in loaded.activity_output_cursors {
            if !cursor.item_id.is_empty()
                && cursor.item_id.len() <= 512
                && cursor.next_offset > 0
                && cursor.next_offset <= protocol::MAX_SESSION_OUTPUT_PAGE_BYTES as u64
                && cursor.next_offset < cursor.total_bytes
                && output_item_ids.contains(cursor.item_id.as_str())
            {
                let _previous = self.restored_output_cursors.insert(cursor.item_id.clone(), cursor);
            }
        }
        self.output_page_pending = None;
        self.output_page_task = None;
        self.output_page_error = None;
        for item in &loaded.review_items {
            if matches!(item.details, vtcode_exec_events::ThreadItemDetails::FileChange(_)) {
                self.update_runtime_item(item);
            }
        }
        for item in &loaded.activity_items {
            use vtcode_exec_events::ThreadItemDetails;
            if !matches!(
                item.details,
                ThreadItemDetails::AgentMessage(_)
                    | ThreadItemDetails::Plan(_)
                    | ThreadItemDetails::Reasoning(_)
                    | ThreadItemDetails::Decision(_)
                    | ThreadItemDetails::FileChange(_)
            ) {
                self.project_runtime_item(item, true);
            }
        }
        self.retained_submission = retained_submission;
        self.selected_session_id = Some(loaded.session.session_id.clone());
        if let Some(pending) = self.pending_navigation.take() {
            let _moved = self.navigation_history.navigate_to(pending.target_index);
        } else {
            self.record_current_navigation();
        }
        self.status = if loaded.review_history_incomplete || loaded.activity_history_incomplete {
            "Session loaded. Recent inspector history is partial or unavailable."
        } else {
            "Session loaded. Submit a prompt to continue."
        }
        .to_string();
        cx.notify();
    }

    pub(super) fn close_session(&mut self, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected_session_id.clone() else {
            return;
        };
        let _sent =
            self.request_session_change(protocol::method::SESSION_CLOSE, json!({ keys::SESSION_ID: session_id }), cx);
    }

    /// Switch the service to another catalog model. The service restarts its
    /// session; the next prompt starts a fresh one under the new model.
    pub(super) fn select_model(&mut self, provider: String, model: String, cx: &mut Context<Self>) {
        if self.attachments_picker_open {
            return;
        }
        if self.request_session_change(
            protocol::method::CONFIG_SELECT,
            json!({ "provider": provider, "model": model }),
            cx,
        ) {
            self.status = "Switching model…".to_string();
        }
    }

    /// Drain one bounded event batch. A full batch asks the async waiter to schedule
    /// another foreground update without returning to a periodic polling loop.
    pub(super) fn cancel_session_load_task(&mut self) {
        self.session_load_generation = self.session_load_generation.wrapping_add(1);
        if self.session_load_task.take().is_some() {
            self.event_tx.wake();
        }
        self.history_page_generation = self.history_page_generation.wrapping_add(1);
        self.history_page_request_id = None;
        self.history_page_limit = 0;
        self.history_page_task = None;
        self.history_page_loading = false;
        self.history_page_error = None;
    }

    /// Clear session-owned projections together so another conversation cannot inherit them.
    pub(super) fn reset_session_state(&mut self) {
        self.cancel_session_load_task();
        self.active_primary_agent_display = None;
        self.transcript.clear();
        self.restored_activity.clear();
        self.history_start_index = 0;
        self.history_has_earlier = false;
        self.restored_output_cursors.clear();
        self.restored_output_pages.clear();
        self.output_page_pending = None;
        self.output_page_task = None;
        self.output_page_error = None;
        self.selected_output_id = None;
        self.activity_scroll = VirtualListScrollHandle::new();
        self.change_list_scroll = VirtualListScrollHandle::new();
        self.restored_review_history_incomplete = false;
        self.restored_activity_history_incomplete = false;
        self.changes = ChangesProjection::default();
        self.selected_change = None;
        self.change_viewer = None;
        self.change_viewer_dirty = false;
        self.review_open = false;
        self.workspace_review = None;
        self.review_return_focus = None;
        self.last_usage = None;
        self.finished_turn_id = None;
        self.clear_pending_interaction();
        self.active_turn_id = None;
        self.cancel_requested = false;
        self.cancel_request_sent = false;
        self.attachments_task = None;
        self.attachments_picker_open = false;
        self.attachments.clear();
        self.pending_submission = None;
        self.pending_first_submission = false;
        self.first_submission = None;
        self.session_request = None;
        self.pending_new_thread = false;
        self.retained_submission = None;
    }

    /// Mirror a service-side session discard (provider switch, credential
    /// repair): drop the selection and turn state, keep attachments for reuse.
    pub(super) fn clear_session_after_service_reset(&mut self) {
        self.selected_session_id = None;
        let attachments = std::mem::take(&mut self.attachments);
        self.reset_session_state();
        self.attachments = attachments;
        self.record_current_navigation();
    }

    pub(super) fn request(&self, method: &str, params: Value) -> bool {
        self.backend.as_ref().is_some_and(|backend| {
            backend
                .commands
                .try_send(BackendCommand::Request { method: method.to_string(), params })
                .is_ok()
        })
    }

    pub(super) fn clear_error(&mut self) {
        self.error = None;
    }
}

/// Short single-line sidebar title derived from a session's first prompt,
/// mirroring the archive's first-prompt preview.
pub(super) fn session_title_from_prompt(prompt: &str) -> String {
    const MAX_TITLE_CHARS: usize = 72;
    let first_line = prompt.lines().find(|line| !line.trim().is_empty()).unwrap_or_default();
    let trimmed = first_line.trim();
    let mut title: String = trimmed.chars().take(MAX_TITLE_CHARS).collect();
    if trimmed.chars().count() > MAX_TITLE_CHARS {
        title.push('…');
    }
    title
}

pub(super) fn restored_history_element_id(session_id: &str, role: &str, text: &str, occurrence: usize) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    session_id.hash(&mut hasher);
    role.hash(&mut hasher);
    text.hash(&mut hasher);
    occurrence.hash(&mut hasher);
    format!("history-{:016x}", hasher.finish())
}

fn restored_archive_element_id(session_id: &str, archive_index: usize) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    session_id.hash(&mut hasher);
    format!("history-{:016x}-{archive_index}", hasher.finish())
}

/// Prepare stable transcript rows and Markdown accessibility projections for archived messages.
pub(super) fn prepare_session_history(session_id: &str, messages: Vec<SessionHistoryMessage>) -> Vec<TranscriptItem> {
    let mut occurrences = HashMap::<u64, usize>::new();
    messages
        .into_iter()
        .filter(|message| !message.hidden || message.role != TranscriptRole::System.as_str())
        .map(|message| {
            let SessionHistoryMessage { archive_index, role, text, .. } = message;
            let title = match TranscriptRole::from_str(&role) {
                Some(TranscriptRole::User) => USER_DISPLAY_NAME.to_string(),
                Some(TranscriptRole::Assistant) => app::NAME.to_string(),
                Some(TranscriptRole::Tool) => "Tool result".to_string(),
                _ => role.clone(),
            };
            let id = archive_index.map_or_else(
                || {
                    let mut fingerprint = std::collections::hash_map::DefaultHasher::new();
                    role.hash(&mut fingerprint);
                    text.hash(&mut fingerprint);
                    let occurrence = occurrences.entry(fingerprint.finish()).or_default();
                    let id = restored_history_element_id(session_id, &role, &text, *occurrence);
                    *occurrence = (*occurrence).saturating_add(1);
                    id
                },
                |index| restored_archive_element_id(session_id, index),
            );
            // Preserve collapsed archive tool text without inventing activity status.
            let mut item = TranscriptItem::new(id, role, title.into(), text);
            item.has_activity_status = false;
            item.prepare_render_content();
            item
        })
        .collect()
}
