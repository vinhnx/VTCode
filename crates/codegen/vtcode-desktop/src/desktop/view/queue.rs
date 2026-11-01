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

/// One prompt waiting behind the running turn.
pub(super) struct QueuedSubmission {
    /// Stable client echo id for the transcript row.
    pub(super) echo_id: String,
    /// Submitted prompt text.
    pub(super) prompt: String,
    /// Whether the service acknowledged this entry (vs optimistic).
    pub(super) acknowledged: bool,
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
        self.push_submission_echo(&draft);
        self.queued_submissions.push(QueuedSubmission {
            echo_id: draft.echo_id.clone(),
            prompt: draft.prompt.clone(),
            acknowledged: false,
        });
        self.transcript.follow_latest();
        cx.notify();
        Ok(())
    }

    /// Echo a user submission into the transcript; shared by immediate and
    /// queued submits.
    pub(super) fn push_submission_echo(&mut self, draft: &SubmissionDraft) {
        let echo = if draft.attachments.is_empty() {
            draft.prompt.clone()
        } else {
            let paths = draft
                .attachments
                .iter()
                .map(|attachment| attachment.path.as_str())
                .collect::<Vec<_>>();
            format!("{}\n\n{}", draft.prompt, rust_i18n::t!("app.composer.attached_list", paths = paths.join(", ")))
        };
        self.transcript.push(TranscriptItem::new(
            draft.echo_id.clone(),
            TranscriptRole::User.as_str().into(),
            rust_i18n::t!(i18n::TRANSCRIPT_YOU).to_string().into(),
            echo,
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
    pub(super) fn confirm_oldest_queued_submission(&mut self) -> bool {
        if let Some(entry) = self.queued_submissions.iter_mut().find(|entry| !entry.acknowledged) {
            entry.acknowledged = true;
            true
        } else {
            false
        }
    }

    /// Apply an authoritative queue snapshot: reconcile entries, record the
    /// depth, previews, and drop count. Confirmed entries beyond the service
    /// depth dispatched (their transcript rows stay as history); every
    /// unconfirmed entry is kept because its ack is still in flight.
    pub(super) fn apply_queue_status(&mut self, status: protocol::TurnQueueStatus) {
        if self.selected_session_id.as_deref() != Some(status.session_id.as_str()) {
            return;
        }
        // Confirmed entries always form a prefix: confirmation marks the
        // oldest unconfirmed entry, demotion appends only when nothing is
        // unconfirmed, and new entries append unconfirmed. Truncating the
        // front therefore only ever removes confirmed (dispatched) entries.
        let confirmed = self.queued_submissions.iter().filter(|entry| entry.acknowledged).count();
        if confirmed > status.queue_depth {
            drop(self.queued_submissions.drain(..confirmed - status.queue_depth));
        }
        self.queue_depth = status.queue_depth;
        self.queue_previews = status.previews;
        if status.dropped_total > self.queue_dropped_total {
            // Overwrite-safe: the cumulative total stays correct no matter
            // how many drop reports arrive, and the banner clears on the next
            // submit like any other composer error.
            self.error = Some(rust_i18n::t!("app.composer.queue_dropped", dropped = status.dropped_total).to_string());
        }
        self.queue_dropped_total = status.dropped_total;
    }

    /// Pop the newest queued message back into the composer for editing.
    /// Mirrors the CLI `EditQueue` gesture; the service text is authoritative.
    pub(super) fn take_latest_queued(&mut self, cx: &mut Context<Self>) {
        if self.take_latest_pending || self.attachments_picker_open || self.backend.is_none() {
            return;
        }
        let Some(session_id) = self.selected_session_id.clone() else {
            return;
        };
        if self.queued_submissions.is_empty() && self.queue_depth == 0 {
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

    /// Which queue surface the composer shows: the entry list when
    /// something is queued, otherwise nothing.
    pub(super) fn queue_panel_state(&self) -> QueuePanelState {
        if !self.queued_submissions.is_empty() || self.queue_depth > 0 {
            QueuePanelState::Entries
        } else {
            QueuePanelState::Hidden
        }
    }

    /// Render the queued-message panel: a count title, one preview row per
    /// entry (oldest first), and an edit action for the newest. When local
    /// entries are missing but the service reports depth, the authoritative
    /// previews render instead so a non-empty queue is never hidden.
    pub(super) fn render_queued_messages(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let count = self.queued_submissions.len().max(self.queue_depth);
        let title = rust_i18n::t!("app.composer.queued_count", count = count).to_string();
        let rows: Vec<String> = if self.queued_submissions.is_empty() {
            self.queue_previews.clone()
        } else {
            self.queued_submissions
                .iter()
                .map(|entry| protocol::queue_preview_label(&entry.prompt))
                .collect()
        };
        div()
            .id("queued-messages")
            .test_support()
            .role(gpui_kit::Role::Status)
            .aria_label(title.clone())
            .w_full()
            .rounded_md()
            .border_1()
            .border_color(themes::hairline(colors.popover, colors.foreground))
            .bg(colors.popover)
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(row_title(title, &colors))
                    .child(
                        desktop_button("edit-newest-queued")
                            .label(rust_i18n::t!("app.composer.edit_queued").to_string())
                            .ghost()
                            .small()
                            .desktop_disabled(self.take_latest_pending || !self.composer.read(cx).value().is_empty())
                            .on_click(cx.listener(|view, _, _, cx| view.take_latest_queued(cx))),
                    ),
            )
            .children(rows.into_iter().map(|row| row_description(row, &colors)))
    }

    /// Drop ephemeral queue state (session switch, teardown, disconnect).
    pub(super) fn clear_queue_state(&mut self) {
        self.queued_submissions.clear();
        self.queue_depth = 0;
        self.queue_previews.clear();
        self.queue_dropped_total = 0;
        self.take_latest_pending = false;
        self.pending_composer_restore = None;
    }
}
