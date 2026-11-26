//! Queued-message state: submit-behind-turn, ack reconciliation, take-latest.
//!
//! The service owns the queue (FIFO, CLI batching semantics); the client
//! mirrors it. Entries are added optimistically on submit, confirmed by
//! `TurnAccepted { queued: true }` (FIFO: the service responds in request
//! order), and reconciled against authoritative `turn/queue-status`
//! snapshots. A queued prompt owns no turn id until its batch dispatches; the
//! two boundary races (client thought immediate but service queued, and vice
//! versa) fold back into the immediate/queued paths in the ack handler.

use super::*;

// Static steer labels are reused across queue renders.
const STEERING_QUEUED_LABEL: gpui_kit::SharedString = gpui_kit::SharedString::new_static("Steering queued prompt");
const STEER_INTO_RUNNING_TURN_LABEL: gpui_kit::SharedString =
    gpui_kit::SharedString::new_static("Steer into the running turn");

/// One prompt waiting behind the running turn.
pub(super) struct QueuedSubmission {
    /// Stable client identity, reused when the message reaches the transcript.
    pub(super) echo_id: String,
    /// Service-minted entry id, assigned on queue acceptance.
    pub(super) entry_id: Option<String>,
    /// Submitted prompt text.
    pub(super) prompt: String,
    /// Attachment card metadata retained until dispatch.
    pub(super) user_attachments: Vec<UserAttachment>,
    /// Whether the service acknowledged this entry (vs optimistic).
    pub(super) acknowledged: bool,
}

impl QueuedSubmission {
    pub(super) fn from_draft(draft: SubmissionDraft, acknowledged: bool) -> Self {
        let user_attachments = Self::attachments(&draft);
        Self {
            echo_id: draft.echo_id,
            entry_id: None,
            prompt: draft.prompt,
            user_attachments,
            acknowledged,
        }
    }

    fn attachments(draft: &SubmissionDraft) -> Vec<UserAttachment> {
        draft
            .attachments
            .iter()
            .map(|attachment| UserAttachment {
                path: attachment.path.clone(),
                byte_len: attachment.text.len(),
                is_image: files::is_image_path(&attachment.path),
            })
            .collect()
    }

    fn echo(id: &str, prompt: &str, user_attachments: Vec<UserAttachment>) -> TranscriptItem {
        let mut item = TranscriptItem::new(
            id.to_string(),
            TranscriptRole::User.as_str().into(),
            rust_i18n::t!(i18n::TRANSCRIPT_YOU).to_string().into(),
            prompt.to_string(),
        );
        item.user_attachments = user_attachments;
        item
    }

    pub(super) fn into_echo(self) -> TranscriptItem {
        Self::echo(&self.echo_id, &self.prompt, self.user_attachments)
    }
}

/// Which queue surface the composer shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QueuePanelState {
    /// Nothing queued: the panel stays out of the way.
    Hidden,
    /// Entries (or authoritative previews) to list.
    Entries,
}

impl DesktopView {
    /// Submit a draft behind the running turn. Mirrors `enqueue_submission`
    /// without touching the active turn's lifecycle state.
    pub(super) fn enqueue_queued_submission(
        &mut self,
        draft: SubmissionDraft,
        batchable: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), SubmissionDraft> {
        let Some(session_id) = self.selected_session_id.clone() else {
            return Err(draft);
        };
        if self.credentials_missing() {
            self.set_provider_auth_error();
            return Err(draft);
        }
        let params = json!({
            keys::SESSION_ID: session_id,
            keys::PROMPT: draft.prompt,
            keys::ATTACHMENTS: draft.attachments,
            keys::BATCHABLE: batchable,
        });
        self.clear_error();
        if !request_fits_frame(protocol::method::TURN_SUBMIT, &params) {
            self.error = Some(rust_i18n::t!("app.composer.prompt_too_long").to_string());
            return Err(draft);
        }
        if !self.request(protocol::method::TURN_SUBMIT, params) {
            self.error = Some(rust_i18n::t!(i18n::QUEUE_FULL).to_string());
            return Err(draft);
        }
        self.ensure_session_title(&session_id, &draft.prompt);
        self.queued_submissions.push(QueuedSubmission::from_draft(draft, false));
        // Only card metadata stays with the queue; the service owns the payload.
        self.prune_attachment_thumbnails();
        self.transcript.follow_latest();
        cx.notify();
        Ok(())
    }

    /// Echo an immediate submission into the transcript.
    pub(super) fn push_submission_echo(&mut self, draft: &SubmissionDraft) {
        self.transcript.push(QueuedSubmission::echo(
            &draft.echo_id,
            &draft.prompt,
            QueuedSubmission::attachments(draft),
        ));
    }

    /// Fill an empty session title from the submission; shared by immediate
    /// and queued submits (idempotent: only empty titles are touched).
    pub(super) fn ensure_session_title(&mut self, session_id: &str, prompt: &str) {
        if let Some(entry) = self
            .sessions
            .iter_mut()
            .find(|session| session.session_id == session_id && session.title.as_deref().is_none_or(str::is_empty))
        {
            entry.title = Some(session_title_from_prompt(prompt));
        }
    }

    /// Confirm the oldest unconfirmed queued entry (FIFO: the service
    /// responds in request order). Returns whether an entry was confirmed.
    pub(super) fn confirm_oldest_queued_submission(&mut self, entry_id: Option<String>) -> bool {
        if let Some(entry) = self.queued_submissions.iter_mut().find(|entry| !entry.acknowledged) {
            entry.acknowledged = true;
            entry.entry_id = entry_id;
            true
        } else {
            false
        }
    }

    /// Promote the oldest unconfirmed queued entry to the running turn: the
    /// service ran it immediately instead of queueing it (idle race), so it
    /// leaves the pending list — no queue snapshot follows an immediate
    /// dispatch to clear it. Adds the transcript echo when promoted.
    pub(super) fn promote_oldest_queued_submission(&mut self) -> bool {
        if let Some(index) = self.queued_submissions.iter().position(|entry| !entry.acknowledged) {
            let entry = self.queued_submissions.remove(index);
            self.transcript.push(entry.into_echo());
            self.transcript.follow_latest();
            true
        } else {
            false
        }
    }

    /// Update the depth. Legacy services report FIFO dispatch implicitly
    /// through depth decreases, so their confirmed prefix supplies the echoes.
    pub(super) fn truncate_dispatched_to_depth(&mut self, depth: usize) {
        self.queue_depth = depth;
        if self.queue_has_dispatch_ids {
            return;
        }
        // Confirmed entries always form a prefix: confirmation marks the
        // oldest unconfirmed entry, demotion appends only when nothing is
        // unconfirmed, and new entries append unconfirmed. Truncating the
        // front therefore only ever removes confirmed (dispatched) entries.
        let confirmed = self.queued_submissions.iter().filter(|entry| entry.acknowledged).count();
        for entry in self.queued_submissions.drain(..confirmed.saturating_sub(depth)) {
            self.transcript.push(entry.into_echo());
        }
    }

    /// Whether the newest entry can be popped back into the composer: no
    /// request in flight and every entry acknowledged, so the service pop
    /// maps to the newest local row exactly.
    pub(super) fn take_latest_ready(&self) -> bool {
        self.steer_pending.is_empty()
            && !self.take_latest_pending
            && !self.queued_submissions.iter().any(|entry| !entry.acknowledged)
    }

    pub(super) fn discard_queued_entries(&mut self, entry_ids: &[String]) {
        self.queued_submissions
            .retain(|entry| entry.entry_id.as_ref().is_none_or(|id| !entry_ids.contains(id)));
    }

    /// Apply service dispatch and discard outcomes, along with queue previews.
    pub(super) fn apply_queue_status(&mut self, status: protocol::TurnQueueStatus) {
        if self.selected_session_id.as_deref() != Some(status.session_id.as_str()) {
            return;
        }
        if let Some(dispatched) = status.dispatched_entry_ids {
            self.queue_has_dispatch_ids = true;
            for id in dispatched {
                if let Some(index) = self
                    .queued_submissions
                    .iter()
                    .position(|entry| entry.entry_id.as_deref() == Some(&id))
                {
                    let entry = self.queued_submissions.remove(index);
                    self.transcript.push(entry.into_echo());
                }
            }
            self.discard_queued_entries(&status.discarded_entry_ids);
            self.queue_depth = status.queue_depth;
        } else {
            let dropped = status.dropped_total.saturating_sub(self.queue_dropped_total);
            let confirmed = self.queued_submissions.iter().take_while(|entry| entry.acknowledged).count();
            let evicted = dropped
                .min(confirmed)
                .min(self.queued_submissions.len().saturating_sub(status.queue_depth));
            drop(self.queued_submissions.drain(..evicted));
            self.truncate_dispatched_to_depth(status.queue_depth);
            // Legacy services only provide the oldest entry ids via snapshots.
            for (entry, id) in self
                .queued_submissions
                .iter_mut()
                .filter(|entry| entry.acknowledged)
                .zip(status.entry_ids)
            {
                entry.entry_id = Some(id);
            }
        }
        self.queue_previews = status.previews;
        if status.dropped_total > self.queue_dropped_total {
            // Overwrite-safe: the cumulative total stays correct no matter
            // how many drop reports arrive, and the banner clears on the next
            // submit like any other composer error.
            self.error = Some(rust_i18n::t!("app.composer.queue_dropped", dropped = status.dropped_total).to_string());
        }
        self.queue_dropped_total = self.queue_dropped_total.max(status.dropped_total);
    }

    /// Pop the newest queued message back into the composer for editing.
    /// Mirrors the CLI `EditQueue` gesture; the service text is authoritative.
    /// Blocked while any entry is unacknowledged: the service pops its
    /// newest entry, which only maps to the newest local row once every
    /// ack has landed.
    pub(super) fn take_latest_queued(&mut self, cx: &mut Context<Self>) {
        if !self.take_latest_ready() || self.attachments_picker_open || self.backend.is_none() {
            return;
        }
        let Some(session_id) = self.selected_session_id.clone() else {
            return;
        };
        if self.visible_queue_depth() == 0 {
            return;
        }
        if !self.composer.read(cx).value().is_empty() {
            return;
        }
        if !self.request(protocol::method::TURN_QUEUE_TAKE_LATEST, json!({ keys::SESSION_ID: session_id })) {
            self.error = Some(rust_i18n::t!(i18n::QUEUE_FULL).to_string());
            cx.notify();
            return;
        }
        self.take_latest_pending = true;
        cx.notify();
    }

    /// Remove a service-addressed entry, retaining its payload until the
    /// service reports whether removal won the race with dispatch.
    pub(super) fn remove_queued_submission(&mut self, echo_id: &str, cx: &mut Context<Self>) {
        if self.steer_pending.contains_key(echo_id) {
            return;
        }
        let Some(session_id) = self.selected_session_id.clone() else {
            return;
        };
        let Some((position, entry)) = self
            .queued_submissions
            .iter()
            .enumerate()
            .find(|(_, entry)| entry.echo_id == echo_id)
        else {
            return;
        };
        let Some(entry_id) = entry.entry_id.clone() else {
            return;
        };
        if self.request(
            protocol::method::TURN_QUEUE_REMOVE,
            json!({ keys::SESSION_ID: session_id, keys::ENTRY_ID: entry_id }),
        ) {
            if !self.queue_has_dispatch_ids {
                let entry = self.queued_submissions.remove(position);
                self.transcript.remove(&entry.echo_id);
            }
        } else {
            self.error = Some(rust_i18n::t!(i18n::REQUEST_QUEUE_UNAVAILABLE).to_string());
        }
        cx.notify();
    }

    /// Steer one queued prompt into the live turn instead of waiting for the
    /// FIFO dispatch. The row stays put until the service confirms delivery;
    /// without a live turn the prompt remains queued and an error is shown.
    pub(super) fn steer_queued_submission(&mut self, echo_id: &str, cx: &mut Context<Self>) {
        // Single-flight: the response/notification router binds receipts by
        // the one pending row. Allowing a second steer would mis-correlate
        // acks because responses carry no per-request identity.
        if !self.steer_pending.is_empty() {
            return;
        }
        let Some(session_id) = self.selected_session_id.clone() else {
            return;
        };
        let Some(entry) = self.queued_submissions.iter().find(|entry| entry.echo_id == echo_id) else {
            return;
        };
        let Some(entry_id) = entry.entry_id.clone() else {
            return;
        };
        let prompt = entry.prompt.clone();
        let params = json!({
            keys::SESSION_ID: session_id,
            keys::TEXT: prompt,
            keys::ENTRY_ID: entry_id,
        });
        if !self.request(protocol::method::TURN_STEER, params) {
            self.error = Some(rust_i18n::t!(i18n::REQUEST_QUEUE_UNAVAILABLE).to_string());
            cx.notify();
            return;
        }
        self.clear_error();
        let _ = self.steer_pending.insert(echo_id.to_string(), String::new());
        cx.notify();
    }

    /// Open the inline editor on one queued row, prefilled with its current
    /// prompt. Only service-addressed rows (those with a bound entry id) can
    /// be edited; the shared input is moved to the newly selected row.
    pub(super) fn start_queue_edit(&mut self, echo_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.steer_pending.contains_key(echo_id) {
            return;
        }
        // One edit at a time: the pending rollback slot is not correlated to a
        // response id, so a second in-flight edit would restore the wrong row.
        if self.queue_edit_pending.is_some() {
            return;
        }
        let Some(entry) = self.queued_submissions.iter().find(|entry| entry.echo_id == echo_id) else {
            return;
        };
        if entry.entry_id.is_none() {
            return;
        }
        let prompt = entry.prompt.clone();
        self.editing_queue_echo_id = Some(echo_id.to_string());
        self.clear_error();
        self.queue_edit_input.update(cx, |input, cx| {
            input.set_value(prompt, window, cx);
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        cx.notify();
    }

    pub(super) fn apply_queued_steer_result(
        &mut self,
        outcome: protocol::TurnSteerResultNotification,
        cx: &mut Context<Self>,
    ) {
        if self.selected_session_id.as_deref() != Some(outcome.session_id.as_str()) {
            return;
        }
        let echo_id = self
            .steer_pending
            .iter()
            .find(|(_, receipt_id)| **receipt_id == outcome.receipt_id)
            .map(|(echo_id, _)| echo_id.clone());
        if let Some(echo_id) = echo_id {
            let _ = self.steer_pending.remove(&echo_id);
            if outcome.accepted {
                // The service removed the addressed row atomically with
                // admission; converge our mirror to its depth.
                if let Some(index) = self.queued_submissions.iter().position(|row| row.echo_id == echo_id) {
                    let row = self.queued_submissions.remove(index);
                    self.transcript.push(row.into_echo());
                    if index < self.queue_previews.len() {
                        let _ = self.queue_previews.remove(index);
                    }
                }
                self.truncate_dispatched_to_depth(outcome.queue_depth);
            } else {
                self.error = Some(format!(
                    "The prompt stayed queued: steering was rejected ({:?}).",
                    outcome.reason.unwrap_or(protocol::TurnSteerRejection::Superseded)
                ));
            }
            cx.notify();
        }
    }

    /// Close the inline editor without changing the queued prompt.
    pub(super) fn cancel_queue_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing_queue_echo_id.take().is_none() {
            return;
        }
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// Commit the inline edit: replace this entry's text in place on the
    /// service (keeping FIFO position and entry id) and update the mirror
    /// optimistically. The edited prompt supplies the eventual transcript echo.
    pub(super) fn commit_queue_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(echo_id) = self.editing_queue_echo_id.clone() else {
            return;
        };
        let text = self.queue_edit_input.read(cx).value().trim().to_string();
        if text.is_empty() {
            self.error = Some("Queued prompt cannot be empty.".to_string());
            cx.notify();
            return;
        }
        let Some(session_id) = self.selected_session_id.clone() else {
            return;
        };
        let Some(entry) = self.queued_submissions.iter().find(|entry| entry.echo_id == echo_id) else {
            self.editing_queue_echo_id = None;
            cx.notify();
            return;
        };
        let Some(entry_id) = entry.entry_id.clone() else {
            self.editing_queue_echo_id = None;
            cx.notify();
            return;
        };
        let params = json!({
            keys::SESSION_ID: session_id,
            keys::ENTRY_ID: entry_id,
            keys::TEXT: text.clone(),
        });
        if !self.request(protocol::method::TURN_QUEUE_EDIT, params) {
            self.error = Some(rust_i18n::t!(i18n::REQUEST_QUEUE_UNAVAILABLE).to_string());
            cx.notify();
            return;
        }
        if let Some(entry) = self.queued_submissions.iter_mut().find(|entry| entry.echo_id == echo_id) {
            self.queue_edit_pending = Some((echo_id.clone(), entry.prompt.clone()));
            entry.prompt = text;
        }
        self.editing_queue_echo_id = None;
        self.clear_error();
        self.focus_composer(window, cx);
        cx.notify();
    }

    fn visible_queue_depth(&self) -> usize {
        // Modern services acknowledge every Desktop draft by ID. Older
        // preview snapshots can arrive after a dispatch and must not bring
        // back a row whose exact entry has already left the local queue.
        if self.queue_has_dispatch_ids {
            self.queued_submissions.len()
        } else {
            self.queued_submissions.len().max(self.queue_depth)
        }
    }

    /// Which queue surface the composer shows: the entry list when
    /// something is queued, otherwise nothing.
    pub(super) fn queue_panel_state(&self) -> QueuePanelState {
        if self.visible_queue_depth() > 0 {
            QueuePanelState::Entries
        } else {
            QueuePanelState::Hidden
        }
    }

    /// Render the queued-message panel: one compact row per entry (oldest
    /// first) with a leading clock icon, the message preview, the wait time,
    /// and per-row actions — pop-to-composer for the newest and remove for
    /// every service-addressed entry. When local entries are missing but the
    /// service reports depth, the authoritative previews render instead so a
    /// non-empty queue is never hidden.
    pub(super) fn render_queued_messages(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let count = self.visible_queue_depth();
        let title = rust_i18n::t!("app.composer.queued_count", count = count).to_string();
        // (row index, echo id, entry id, prompt) per row, oldest first.
        // Authoritative previews (no local entries) carry no ids: their rows
        // render without actions rather than guessing identity the service
        // has not confirmed.
        let rows: Vec<(usize, Option<String>, Option<String>, String)> = if self.queued_submissions.is_empty() {
            self.queue_previews
                .iter()
                .enumerate()
                .map(|(index, preview)| (index, None, None, protocol::queue_preview_label(preview)))
                .collect()
        } else {
            self.queued_submissions
                .iter()
                .enumerate()
                .map(|(index, entry)| {
                    (
                        index,
                        Some(entry.echo_id.clone()),
                        entry.entry_id.clone(),
                        protocol::queue_preview_label(&entry.prompt),
                    )
                })
                .collect()
        };
        let muted = themes::muted_text(&colors);
        let hairline = themes::hairline(colors.popover, colors.foreground);
        div()
            .id("queued-messages")
            .test_support()
            .role(gpui_kit::Role::Status)
            .aria_label(title.clone())
            .w_full()
            .rounded_md()
            .border_1()
            .border_color(hairline)
            .bg(colors.popover)
            .p_2()
            .flex()
            .flex_col()
            .gap_1()
            // Header row: only the count. Editing moved inline to each entry
            // so any queued prompt can be changed without popping it to the
            // composer; the count stays on the shared title spine.
            .child(div().w_full().flex().items_center().px_1().child(row_title(title, &colors)))
            .children(rows.into_iter().map(|(row_index, echo_id, entry_id, prompt)| {
                let steer_pending = echo_id.as_ref().is_some_and(|id| self.steer_pending.contains_key(id));
                let steer_available = self
                    .queued_submissions
                    .get(row_index)
                    .is_some_and(|entry| !entry.prompt.trim().is_empty() && entry.user_attachments.is_empty());
                let remove_label = rust_i18n::t!("app.composer.remove_queued").to_string();
                // Each entry reads as one card in a compact stack: its own
                // surface, a hairline boundary, and stable icon/text/action
                // lanes. No age column — the count is the state that matters.
                let card = || {
                    div()
                        .id(queued_row_element_id(row_index, echo_id.as_deref()))
                        .test_support()
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .border_1()
                        .border_color(hairline)
                        .bg(colors.background)
                };

                // Inline editor for the selected row. Enter commits through
                // the shared input's subscription; Save/Cancel expose the
                // same actions to pointer and keyboard users.
                if self.editing_queue_echo_id.as_deref() == echo_id.as_deref() {
                    return card()
                        .child(Icon::new(IconName::Pencil).size_3().text_color(muted).into_any_element())
                        .child(
                            Input::new(&self.queue_edit_input)
                                .id(queued_edit_input_id(row_index, echo_id.as_deref()))
                                .accessibility_id("queued-edit-input")
                                .aria_label("Edit queued prompt".to_string())
                                .flex_1(),
                        )
                        .child(
                            desktop_ghost_button(queued_edit_save_id(row_index, echo_id.as_deref()))
                                .icon(IconName::Check)
                                .accessibility_label("Save prompt".to_string())
                                .tooltip("Save prompt".to_string())
                                .xsmall()
                                .on_click(cx.listener(|view, _, window, cx| view.commit_queue_edit(window, cx))),
                        )
                        .child(
                            desktop_ghost_button(queued_edit_cancel_id(row_index, echo_id.as_deref()))
                                .icon(IconName::X)
                                .accessibility_label("Cancel edit".to_string())
                                .tooltip("Cancel edit".to_string())
                                .xsmall()
                                .on_click(cx.listener(|view, _, window, cx| view.cancel_queue_edit(window, cx))),
                        )
                        .into_any_element();
                }

                let row = card()
                    .child(
                        // Leading status icon: the clock marks the waiting
                        // state without encoding it by color alone.
                        Icon::new(IconName::Clock).size_3().text_color(muted).into_any_element(),
                    )
                    .child(
                        div().min_w_0().flex_1().flex().items_baseline().gap_2().child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .text_sm()
                                .text_color(colors.foreground)
                                .child(prompt.clone()),
                        ),
                    );

                // Trailing action lane: edit, steer, and remove are icon-only
                // with hover labels. Only service-addressed entries (those
                // with a bound entry id) offer actions: an optimistic row has
                // no wire identity for the service to address yet.
                match (&echo_id, &entry_id) {
                    (_, Some(_)) => row
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(
                                    desktop_ghost_button(queued_edit_element_id(row_index, echo_id.as_deref()))
                                        .icon(IconName::Pencil)
                                        .desktop_disabled(steer_pending)
                                        .accessibility_label("Edit prompt".to_string())
                                        .tooltip("Edit prompt".to_string())
                                        .xsmall()
                                        .on_click({
                                            let echo_id = echo_id.clone();
                                            cx.listener(move |view, _, window, cx| {
                                                let Some(echo_id) = echo_id.as_deref() else {
                                                    return;
                                                };
                                                view.start_queue_edit(echo_id, window, cx);
                                            })
                                        }),
                                )
                                .child(
                                    desktop_ghost_button(queued_steer_element_id(row_index, echo_id.as_deref()))
                                        .icon(IconName::ArrowRight)
                                        .loading(steer_pending)
                                        .desktop_disabled(
                                            !steer_available || !self.running || !self.steer_pending.is_empty(),
                                        )
                                        .accessibility_label(if steer_pending {
                                            STEERING_QUEUED_LABEL.clone()
                                        } else {
                                            STEER_INTO_RUNNING_TURN_LABEL.clone()
                                        })
                                        .tooltip(STEER_INTO_RUNNING_TURN_LABEL.clone())
                                        .xsmall()
                                        .on_click({
                                            let echo_id = echo_id.clone();
                                            cx.listener(move |view, _, _, cx| {
                                                let Some(echo_id) = echo_id.as_deref() else {
                                                    return;
                                                };
                                                view.steer_queued_submission(echo_id, cx);
                                            })
                                        }),
                                )
                                .child(
                                    desktop_danger_button(queued_remove_element_id(row_index, echo_id.as_deref()))
                                        .icon(IconName::Trash)
                                        .desktop_disabled(steer_pending)
                                        .accessibility_label(remove_label.clone())
                                        .tooltip(remove_label)
                                        .xsmall()
                                        .on_click({
                                            let echo_id = echo_id.clone();
                                            cx.listener(move |view, _, _, cx| {
                                                let Some(echo_id) = echo_id.as_deref() else {
                                                    return;
                                                };
                                                view.remove_queued_submission(echo_id, cx);
                                            })
                                        }),
                                ),
                        )
                        .into_any_element(),
                    _ => row.into_any_element(),
                }
            }))
    }

    /// Drop ephemeral queue state (session switch, teardown, disconnect).
    pub(super) fn clear_queue_state(&mut self) {
        self.queued_submissions.clear();
        self.queue_depth = 0;
        self.queue_has_dispatch_ids = false;
        self.queue_previews.clear();
        self.queue_dropped_total = 0;
        self.take_latest_pending = false;
        self.steer_pending.clear();
        self.editing_queue_echo_id = None;
        self.queue_edit_pending = None;
        self.pending_composer_restore = None;
    }

    /// The newest acknowledged queue entry's wire identity, when the service
    /// has bound it. Gates offer-sensitive surfaces so the panel never
    /// promises identity the queue state cannot yet honor.
    #[cfg(test)]
    pub(super) fn newest_entry_id_is_bound(&self) -> bool {
        self.queued_submissions
            .iter()
            .rev()
            .find(|entry| entry.acknowledged)
            .is_some_and(|entry| entry.entry_id.is_some())
    }
}

/// Bounded row element id: local rows echo the client transcript id,
/// authoritative preview rows fall back to their slot index.
fn queued_row_element_id(row_index: usize, echo_id: Option<&str>) -> gpui_kit::ElementId {
    gpui_kit::ElementId::named_usize("queued-row", queued_row_discriminator(row_index, echo_id))
}

/// Bounded remove-action element id, paired with its row.
fn queued_remove_element_id(row_index: usize, echo_id: Option<&str>) -> gpui_kit::ElementId {
    gpui_kit::ElementId::named_usize("queued-remove", queued_row_discriminator(row_index, echo_id))
}

/// Bounded steer-action element id, paired with its row.
pub(super) fn queued_steer_element_id(row_index: usize, echo_id: Option<&str>) -> gpui_kit::ElementId {
    gpui_kit::ElementId::named_usize("queued-steer", queued_row_discriminator(row_index, echo_id))
}

/// Bounded edit-action element id, paired with its row.
fn queued_edit_element_id(row_index: usize, echo_id: Option<&str>) -> gpui_kit::ElementId {
    gpui_kit::ElementId::named_usize("queued-edit", queued_row_discriminator(row_index, echo_id))
}

/// Stable id for the inline editor's input, scoped to its row.
fn queued_edit_input_id(row_index: usize, echo_id: Option<&str>) -> gpui_kit::ElementId {
    gpui_kit::ElementId::named_usize("queued-edit-input", queued_row_discriminator(row_index, echo_id))
}

/// Stable id for the inline editor's Save action.
fn queued_edit_save_id(row_index: usize, echo_id: Option<&str>) -> gpui_kit::ElementId {
    gpui_kit::ElementId::named_usize("queued-edit-save", queued_row_discriminator(row_index, echo_id))
}

/// Stable id for the inline editor's Cancel action.
fn queued_edit_cancel_id(row_index: usize, echo_id: Option<&str>) -> gpui_kit::ElementId {
    gpui_kit::ElementId::named_usize("queued-edit-cancel", queued_row_discriminator(row_index, echo_id))
}

/// Hash the row's stable identity (echo id when bound, slot index fallback)
/// into a bounded usize discriminator, matching `opaque_path_element_id`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "all supported targets are 64-bit, so the u64 hash never truncates"
)]
fn queued_row_discriminator(row_index: usize, echo_id: Option<&str>) -> usize {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    echo_id.map_or(row_index, |echo_id| {
        echo_id.hash(&mut hasher);
        hasher.finish() as usize
    })
}
