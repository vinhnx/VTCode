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
    /// Service-minted bounded entry id (from the ack's status snapshot or a
    /// later `turn/queue-status`), matching the entry one-to-one. Absent on
    /// optimistic entries until the next snapshot.
    pub(super) entry_id: Option<String>,
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
            entry_id: None,
            prompt: draft.prompt.clone(),
            acknowledged: false,
        });
        // The queue entry keeps no attachments: the draft's images are consumed
        // with the send, so their thumbnails must go with them.
        self.prune_attachment_thumbnails();
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
    /// depth, previews, ids, elapsed seconds, and drop count. Confirmed entries
    /// beyond the service depth dispatched (their transcript rows stay as
    /// history); every unconfirmed entry is kept because its ack is still in
    /// flight.
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
        // Bind optimistic entries to their wire identity: the service lists
        // its queue oldest first, and confirmed entries (whose acks arrived)
        // own the leading ids. Optimistic entries wait for a later snapshot.
        let mut ids = status.entry_ids.into_iter();
        for entry in self.queued_submissions.iter_mut().filter(|entry| entry.acknowledged) {
            let Some(id) = ids.next() else { break };
            entry.entry_id = Some(id);
        }
        self.queue_depth = status.queue_depth;
        self.queue_previews = status.previews;
        self.queue_ages_secs = status.queued_for_secs;
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

    /// Remove one queued message by its service-minted entry id. The row is
    /// removed optimistically when the local entry matches; the authoritative
    /// snapshot restores it if the service disagrees. Entries without an id
    /// (optimistic, unsnapped) cannot be removed until their snapshot arrives.
    pub(super) fn remove_queued_submission(&mut self, echo_id: &str, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected_session_id.clone() else {
            return;
        };
        let Some(entry) = self.queued_submissions.iter().find(|entry| entry.echo_id == echo_id) else {
            return;
        };
        let Some(entry_id) = entry.entry_id.clone() else {
            return;
        };
        let position = self.queued_submissions.iter().position(|entry| entry.echo_id == echo_id);
        if let Some(position) = position {
            let removed = self.queued_submissions.remove(position);
            self.transcript.remove(&removed.echo_id);
        }
        // The optimistic row hides immediately; the service publishes the
        // resulting snapshot, which reconciles or restores on disagreement.
        if !self.request(
            protocol::method::TURN_QUEUE_REMOVE,
            json!({ keys::SESSION_ID: session_id, keys::ENTRY_ID: entry_id }),
        ) {
            self.error = Some(rust_i18n::t!(i18n::REQUEST_QUEUE_UNAVAILABLE).to_string());
        }
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

    /// Render the queued-message panel: one compact row per entry (oldest
    /// first) with a leading clock icon, the message preview, the wait time,
    /// and per-row actions — pop-to-composer for the newest and remove for
    /// every service-addressed entry. When local entries are missing but the
    /// service reports depth, the authoritative previews render instead so a
    /// non-empty queue is never hidden.
    pub(super) fn render_queued_messages(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let count = self.queued_submissions.len().max(self.queue_depth);
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
        let rows_len = rows.len();
        let composer_busy = !self.composer.read(cx).value().is_empty();
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
            // Header row: count on the shared title spine, the edit action
            // trailing. Keeps the alt+up gesture reachable per keyboard rule.
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_1()
                    .child(row_title(title, &colors))
                    .child(
                        desktop_button("edit-newest-queued")
                            .label(rust_i18n::t!("app.composer.edit_queued").to_string())
                            .ghost()
                            .small()
                            .desktop_disabled(self.take_latest_pending || composer_busy)
                            .on_click(cx.listener(|view, _, _, cx| view.take_latest_queued(cx))),
                    ),
            )
            .children(rows.into_iter().map(|(row_index, echo_id, entry_id, prompt)| {
                let wait = self.queue_wait_label(row_index);
                let remove_label = rust_i18n::t!("app.composer.remove_queued").to_string();
                let row = div()
                    .id(queued_row_element_id(row_index, echo_id.as_deref()))
                    .test_support()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_1()
                    .py_0p5()
                    .rounded_sm()
                    .child(
                        // Leading status icon: the clock marks the waiting
                        // state without encoding it by color alone.
                        Icon::new(IconName::Clock).size_3().text_color(muted).into_any_element(),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .items_baseline()
                            .gap_2()
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .text_sm()
                                    .text_color(colors.foreground)
                                    .child(prompt.clone()),
                            )
                            .when_some(wait, |meta, wait| {
                                meta.child(div().flex_shrink_0().text_xs().text_color(muted).child(wait))
                            }),
                    );
                // Trailing action lane: one quiet icon button per row. Every
                // service-addressed entry removes by id; the newest local
                // entry without an id keeps the header-level pop affordance.
                let row = match (&echo_id, &entry_id) {
                    (_, Some(_)) => row.child(
                        desktop_button(queued_remove_element_id(row_index, echo_id.as_deref()))
                            .icon(IconName::Trash)
                            .accessibility_label(remove_label.clone())
                            .tooltip(remove_label)
                            .ghost()
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
                    (Some(_), None) if row_index + 1 == rows_len && !composer_busy => row.child(
                        desktop_button("edit-newest-queued-row")
                            .icon(IconName::ArrowUp)
                            .accessibility_label(rust_i18n::t!("app.composer.edit_queued").to_string())
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(|view, _, _, cx| view.take_latest_queued(cx))),
                    ),
                    _ => row,
                };
                row
            }))
    }

    /// Drop ephemeral queue state (session switch, teardown, disconnect).
    pub(super) fn clear_queue_state(&mut self) {
        self.queued_submissions.clear();
        self.queue_depth = 0;
        self.queue_previews.clear();
        self.queue_ages_secs.clear();
        self.queue_dropped_total = 0;
        self.take_latest_pending = false;
        self.pending_composer_restore = None;
    }

    /// Human-readable wait time for one preview or entry row. The snapshot
    /// lists ages oldest-first, matching preview slots one-to-one; local
    /// entries count their slot from the front the same way. `None` renders
    /// no time (service omits ages, or the row sits past the snapshot).
    fn queue_wait_label(&self, row_index: usize) -> Option<String> {
        let secs = *self.queue_ages_secs.get(row_index)?;
        Some(queue_wait_label(secs))
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

/// Format the per-row wait time. Below a minute shows seconds; beyond it,
/// short human units keep the column scannable.
fn queue_wait_label(secs: u64) -> String {
    queue_wait_time_label(secs)
}

/// Shared wait-time formatter (published for tests).
#[cfg(test)]
pub(crate) fn queue_wait_time_label(secs: u64) -> String {
    if secs < 60 {
        return rust_i18n::t!("app.composer.queue_wait_time", time = format!("{secs}s")).to_string();
    }
    let minutes = secs / 60;
    if minutes < 60 {
        return rust_i18n::t!("app.composer.queue_wait_time", time = format!("{minutes}m")).to_string();
    }
    let hours = minutes / 60;
    rust_i18n::t!("app.composer.queue_wait_time", time = format!("{hours}h")).to_string()
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

/// Hash the row's stable identity (echo id when bound, slot index fallback)
/// into a bounded usize discriminator, matching `opaque_path_element_id`.
fn queued_row_discriminator(row_index: usize, echo_id: Option<&str>) -> usize {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    echo_id.map_or(row_index, |echo_id| {
        echo_id.hash(&mut hasher);
        hasher.finish() as usize
    })
}
