//! Thread-event projection: notification matching and turn/matrix updates.

use super::*;

impl DesktopView {
    fn push_turn_notice(&mut self, mut item: TranscriptItem, turn_id: Option<&str>) {
        item.turn_id = turn_id.map(|id| id.to_owned().into());
        self.transcript.push(item);
    }

    pub(super) fn notification_matches(&self, session_id: &str, turn_id: Option<&str>) -> bool {
        if self.selected_session_id.as_deref() != Some(session_id) {
            return false;
        }
        if let Some(turn_id) = turn_id {
            if self.active_turn_id.as_deref().is_some_and(|active| active != turn_id) {
                return false;
            }
            if self.running && self.finished_turn_id.as_deref() == Some(turn_id) {
                return false;
            }
        }
        true
    }

    pub(in super::super) fn handle_thread_event(&mut self, notification: ThreadEventNotification) {
        use vtcode_exec_events::ThreadEvent;
        let terminal_turn_id = matches!(
            notification.event.event(),
            ThreadEvent::TurnCompleted(_) | ThreadEvent::TurnFailed(_) | ThreadEvent::TurnBlocked(_)
        )
        .then(|| notification.turn_id.clone())
        .flatten();
        // Transcript mutations clear the pending row; lifecycle events leave it untouched.
        let produced_content = matches!(
            notification.event.event(),
            ThreadEvent::PlanDelta(_)
                | ThreadEvent::ItemStarted(_)
                | ThreadEvent::ItemUpdated(_)
                | ThreadEvent::ItemCompleted(_)
                | ThreadEvent::Error(_)
                | ThreadEvent::ThreadCompactBoundary(_)
                | ThreadEvent::ContextReset(_)
                | ThreadEvent::AppSandboxDenied(_)
        );
        match notification.event.event() {
            ThreadEvent::TurnStarted(_) => {
                self.pending_submission = None;
                self.pending_first_submission = false;
                self.prune_attachment_thumbnails();
                self.running = true;
                self.turn_content_seen = false;
                self.active_turn_id = notification.turn_id;
                self.set_selected_session_completed(false);
                self.status = rust_i18n::t!("app.service.running").to_string();
            }
            ThreadEvent::TurnCompleted(event) => {
                if let (Some(turn_id), Some(final_id)) =
                    (notification.turn_id.as_deref(), event.final_response_item_id.as_deref())
                {
                    self.transcript.complete_turn_group(turn_id, final_id);
                }
                if let (Some(turn_id), Some(id), Some(timestamp)) =
                    (notification.turn_id.as_deref(), &event.final_response_item_id, &event.completed_at)
                    && let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(timestamp)
                {
                    let local_time = timestamp.with_timezone(&chrono::Local).format("%H:%M").to_string();
                    self.transcript.set_completion_time(turn_id, id, local_time.into());
                }
                self.pending_submission = None;
                self.pending_first_submission = false;
                self.prune_attachment_thumbnails();
                self.running = false;
                self.active_turn_id = None;
                self.cancel_requested = false;
                self.cancel_request_sent = false;
                self.status = rust_i18n::t!("app.service.completed_status").to_string();
                self.last_usage = Some(event.usage.clone());
                self.set_selected_session_completed(true);
            }
            ThreadEvent::TurnFailed(event) => {
                self.finish_unverified_turn(&event.message);
            }
            ThreadEvent::TurnBlocked(event) => {
                self.finish_unverified_turn(&event.message);
            }
            ThreadEvent::PlanDelta(delta) => {
                if self
                    .transcript
                    .accepts_item_turn(&delta.item_id, notification.turn_id.as_deref())
                {
                    self.transcript.append_text(
                        delta.item_id.clone(),
                        TranscriptRole::Assistant.as_str(),
                        rust_i18n::t!(i18n::TRANSCRIPT_PLAN_TITLE).to_string().into(),
                        &delta.delta,
                    );
                    if let Some(turn_id) = notification.turn_id.as_deref() {
                        self.transcript.set_item_turn(&delta.item_id, turn_id);
                    }
                } else {
                    // Conflicting turn identity: the stale update is dropped.
                }
            }
            ThreadEvent::ItemStarted(event) => {
                let row_id = self.runtime_output_row_id(&event.item).to_owned();
                let accepted = self
                    .transcript
                    .accepts_item_turn(&event.item.id, notification.turn_id.as_deref())
                    && self.transcript.accepts_item_turn(&row_id, notification.turn_id.as_deref());
                self.update_runtime_item_for_turn(&event.item, notification.turn_id.as_deref());
                if accepted && matches!(event.item.details, vtcode_exec_events::ThreadItemDetails::ToolOutput(_)) {
                    self.transcript.set_output_truncated(&row_id, false);
                }
            }
            ThreadEvent::ItemUpdated(event) => {
                let mut item = event.item.clone();
                let row_id = self.runtime_output_row_id(&item).to_owned();
                let accepted = self.transcript.accepts_item_turn(&item.id, notification.turn_id.as_deref())
                    && self.transcript.accepts_item_turn(&row_id, notification.turn_id.as_deref());
                let mut truncated = None;
                if let vtcode_exec_events::ThreadItemDetails::ToolOutput(output) = &mut item.details
                    && output.is_delta == Some(true)
                {
                    let row = self
                        .transcript
                        .position_by_id(&row_id)
                        .and_then(|index| self.transcript.get(index));
                    let previous = row.and_then(|row| row.output.as_deref()).unwrap_or_default();
                    truncated = Some(
                        row.is_some_and(|row| row.output_truncated)
                            || previous.len().saturating_add(output.output.len()) > LIVE_OUTPUT_LIMIT,
                    );
                    output.output = bounded_output_chunk(previous, &output.output);
                } else if matches!(item.details, vtcode_exec_events::ThreadItemDetails::ToolOutput(_)) {
                    truncated = Some(false);
                }
                self.update_runtime_item_for_turn(&item, notification.turn_id.as_deref());
                if accepted && let Some(truncated) = truncated {
                    self.transcript.set_output_truncated(&row_id, truncated);
                }
            }
            ThreadEvent::ItemCompleted(event) => {
                let row_id = self.runtime_output_row_id(&event.item).to_owned();
                let accepted = self
                    .transcript
                    .accepts_item_turn(&event.item.id, notification.turn_id.as_deref())
                    && self.transcript.accepts_item_turn(&row_id, notification.turn_id.as_deref());
                self.update_runtime_item_for_turn(&event.item, notification.turn_id.as_deref());
                if accepted && matches!(event.item.details, vtcode_exec_events::ThreadItemDetails::ToolOutput(_)) {
                    self.transcript.set_output_truncated(&row_id, false);
                }
            }
            ThreadEvent::Error(event) => {
                self.transcript.push(TranscriptItem::notice(
                    format!("runtime-error-{}", uuid_like_id()),
                    rust_i18n::t!("app.transcript.error_title").to_string().into(),
                    event.message.clone(),
                    NoticeSeverity::Error,
                ));
            }
            ThreadEvent::ThreadCompactBoundary(event) => {
                // Pre-boundary usage no longer describes the live context; the
                // next completed turn reports fresh post-boundary usage.
                self.last_usage = None;
                self.push_turn_notice(
                    TranscriptItem::notice(
                        format!("compact-boundary-{}", uuid_like_id()),
                        rust_i18n::t!("app.transcript.compact_boundary_title").to_string().into(),
                        rust_i18n::t!(
                            "app.transcript.compact_boundary_body",
                            original = event.original_message_count,
                            compacted = event.compacted_message_count,
                            trigger = event.trigger.as_str(),
                            mode = event.mode.as_str()
                        )
                        .to_string(),
                        NoticeSeverity::Info,
                    ),
                    notification.turn_id.as_deref(),
                );
            }
            ThreadEvent::ContextReset(event) => {
                self.last_usage = None;
                let trigger = match event.trigger {
                    vtcode_exec_events::ContextResetTrigger::PlanApproval => "plan approval",
                    vtcode_exec_events::ContextResetTrigger::Unknown => "unknown",
                };
                self.push_turn_notice(
                    TranscriptItem::notice(
                        format!("context-reset-{}", uuid_like_id()),
                        rust_i18n::t!("app.transcript.context_reset_title").to_string().into(),
                        rust_i18n::t!(
                            "app.transcript.context_reset_body",
                            trigger = trigger,
                            previous = event.previous_context_usage_percent
                        )
                        .to_string(),
                        NoticeSeverity::Info,
                    ),
                    notification.turn_id.as_deref(),
                );
            }
            ThreadEvent::AppSandboxDenied(event) => {
                self.transcript.push(TranscriptItem::notice(
                    format!("app-sandbox-denied-{}", uuid_like_id()),
                    rust_i18n::t!("app.transcript.app_refused_title").to_string().into(),
                    rust_i18n::t!(
                        "app.transcript.app_refused_body",
                        tool = event.tool.as_str(),
                        tier = event.tier.as_str(),
                        uris = event.uris.join(", ")
                    )
                    .to_string(),
                    NoticeSeverity::Warning,
                ));
            }
            ThreadEvent::MatrixUpdated(snapshot) => {
                self.background_activity_count = event_signals::matrix_loading_count(snapshot);
            }
            _ => {}
        }
        if produced_content {
            self.turn_content_seen = true;
        }
        if let Some(turn_id) = terminal_turn_id {
            self.finished_turn_id = Some(turn_id);
        }
    }
}
