//! Per-turn file-change undo: request, response, and completion notices.
//!
//! The file card on a transcript row offers one quiet action: revert exactly
//! the file changes that turn recorded. The request rides the generic
//! `turn/undo` round trip (`BackendEvent::Response`); this module owns the
//! per-turn flight state plus the pure outcome mapping shared by the state
//! handler and the notification hook so the two paths cannot disagree.

use gpui_kit::component::notification::Notification;

use super::*;

/// Undo flight state for one turn. Absent from [`DesktopView::turn_undos`]
/// means idle; failures remove the entry so the button re-enables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TurnUndoState {
    /// Request sent, response not yet applied.
    Pending,
    /// Server confirmed every recorded change was reversed.
    Undone,
}

/// Classified `turn/undo` response, before any state mutation.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum TurnUndoOutcome {
    /// Recorded changes reversed for a turn this view is waiting on.
    Undone { turn_id: String },
    /// Server declined; nothing (or only the disclosed prefix) changed.
    Refused { turn_id: String, reason: String },
    /// Response for an unknown turn, another session, or a stale retry.
    Stale,
    /// Payload does not parse as [`protocol::TurnUndoResult`].
    Invalid,
}

/// Turn identity to send for an undo request: restored rows keep the event
/// identity in `canonical_turn_id` while live rows carry it in `turn_id`.
pub(super) fn undo_turn_id(item: &TranscriptItem) -> Option<&str> {
    item.canonical_turn_id.as_deref().or(item.turn_id.as_deref())
}

/// Pure outcome mapping shared by [`DesktopView::handle_turn_undo_response`]
/// and [`DesktopView::notify_turn_undo`]: only a parseable result for a
/// pending turn of the selected session applies; anything else is stale.
pub(super) fn turn_undo_outcome(
    undo_states: &HashMap<String, TurnUndoState>,
    selected_session_id: Option<&str>,
    result: &Value,
) -> TurnUndoOutcome {
    let Ok(outcome) = serde_json::from_value::<protocol::TurnUndoResult>(result.clone()) else {
        return TurnUndoOutcome::Invalid;
    };
    let pending = undo_states.get(outcome.turn_id.as_str()) == Some(&TurnUndoState::Pending);
    let selected = selected_session_id == Some(outcome.session_id.as_str());
    if !pending || !selected {
        return TurnUndoOutcome::Stale;
    }
    if outcome.undone {
        TurnUndoOutcome::Undone { turn_id: outcome.turn_id }
    } else {
        TurnUndoOutcome::Refused { turn_id: outcome.turn_id, reason: outcome.reason }
    }
}

impl DesktopView {
    /// Send `turn/undo` for one turn. Returns false (sending nothing) when
    /// the turn is already pending or undone, no session is selected, or the
    /// request queue is unavailable.
    pub(super) fn request_turn_undo(&mut self, turn_id: &str) -> bool {
        if self.turn_undos.contains_key(turn_id) {
            return false;
        }
        let Some(session_id) = self.selected_session_id.clone() else {
            return false;
        };
        let params = protocol::TurnUndoParams { session_id, turn_id: turn_id.to_owned() };
        let Ok(params) = serde_json::to_value(params) else {
            return false;
        };
        if self.request(protocol::method::TURN_UNDO, params) {
            let _previous = self.turn_undos.insert(turn_id.to_owned(), TurnUndoState::Pending);
            true
        } else {
            self.error = Some(rust_i18n::t!(i18n::REQUEST_QUEUE_UNAVAILABLE).to_string());
            false
        }
    }

    /// Apply a successful `turn/undo` response: confirm undone turns, reopen
    /// refused ones, drop stale answers. The drain loop notifies afterwards.
    pub(super) fn handle_turn_undo_response(&mut self, result: Value) {
        match turn_undo_outcome(&self.turn_undos, self.selected_session_id.as_deref(), &result) {
            TurnUndoOutcome::Undone { turn_id } => {
                let _previous = self.turn_undos.insert(turn_id, TurnUndoState::Undone);
                self.status = rust_i18n::t!("app.notifications.turn_undo_done").to_string();
            }
            TurnUndoOutcome::Refused { turn_id, .. } => {
                let _removed = self.turn_undos.remove(&turn_id);
                self.status = rust_i18n::t!("app.service.request_failed").to_string();
            }
            TurnUndoOutcome::Invalid => {
                // Unparseable payloads cannot name their turn; reopen every
                // pending button rather than stranding one disabled.
                self.clear_pending_turn_undos();
                self.error = Some(rust_i18n::t!("app.notifications.turn_undo_failed").to_string());
            }
            TurnUndoOutcome::Stale => {}
        }
    }

    /// Toast the undo completion its response reports. Runs before
    /// [`DesktopView::handle_backend_event`] while the turn is still pending;
    /// failures already surface inline, so only classified outcomes notify.
    pub(super) fn notify_turn_undo(&self, event: &BackendEvent, window: &mut Window, cx: &mut Context<Self>) {
        let BackendEvent::Response { method, result, error } = event else {
            return;
        };
        if method != protocol::method::TURN_UNDO || error.is_some() {
            return;
        }
        let Some(result) = result else { return };
        match turn_undo_outcome(&self.turn_undos, self.selected_session_id.as_deref(), result) {
            TurnUndoOutcome::Undone { .. } => {
                // The outcome mapping guarantees a selected session; the
                // `else` only keeps this total if that ever changes.
                let Some(session_id) = self.selected_session_id.clone() else {
                    return;
                };
                let weak = cx.entity().downgrade();
                let target = deeplinks::NotificationTarget::Thread { session_id };
                window.push_notification(
                    Notification::success(rust_i18n::t!("app.notifications.turn_undo_done").to_string())
                        .id::<super::super::notifications::TurnUndoNotification>()
                        .placement(Anchor::BottomRight)
                        .on_click(deeplinks::click_handler(target, weak)),
                    cx,
                );
            }
            TurnUndoOutcome::Refused { reason, .. } => {
                let message = if reason.is_empty() {
                    rust_i18n::t!("app.notifications.turn_undo_failed").to_string()
                } else {
                    reason
                };
                // No click target: refusals fire on the already-selected
                // thread and surface inline plus the status row.
                window.push_notification(
                    Notification::error(message).id::<super::super::notifications::TurnUndoNotification>(),
                    cx,
                );
            }
            TurnUndoOutcome::Stale | TurnUndoOutcome::Invalid => {}
        }
    }

    /// Reopen every turn waiting on an undo response. Transport errors never
    /// name their turn, so a failed `turn/undo` round trip clears the whole
    /// pending set; confirmed undone marks are historical facts and stay.
    pub(super) fn clear_pending_turn_undos(&mut self) {
        self.turn_undos.retain(|_, state| *state != TurnUndoState::Pending);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(turn_id: &str) -> HashMap<String, TurnUndoState> {
        HashMap::from([(turn_id.to_owned(), TurnUndoState::Pending)])
    }

    fn undo_result(session_id: &str, turn_id: &str, undone: bool, reason: &str) -> Value {
        serde_json::to_value(protocol::TurnUndoResult {
            session_id: session_id.to_owned(),
            turn_id: turn_id.to_owned(),
            undone,
            files: Vec::new(),
            reason: reason.to_owned(),
        })
        .expect("undo result serializes")
    }

    #[test]
    fn outcome_confirms_a_pending_turn_of_the_selected_session() {
        let result = undo_result("session-a", "turn-a", true, "");
        assert_eq!(
            turn_undo_outcome(&pending("turn-a"), Some("session-a"), &result),
            TurnUndoOutcome::Undone { turn_id: "turn-a".to_owned() }
        );
    }

    #[test]
    fn outcome_carries_the_refusal_reason() {
        let result = undo_result("session-a", "turn-a", false, "file changed since turn");
        assert_eq!(
            turn_undo_outcome(&pending("turn-a"), Some("session-a"), &result),
            TurnUndoOutcome::Refused {
                turn_id: "turn-a".to_owned(),
                reason: "file changed since turn".to_owned()
            }
        );
    }

    #[test]
    fn outcome_ignores_unknown_turns_other_sessions_and_finished_turns() {
        let foreign_turn = undo_result("session-a", "turn-b", true, "");
        assert_eq!(turn_undo_outcome(&pending("turn-a"), Some("session-a"), &foreign_turn), TurnUndoOutcome::Stale);
        let foreign_session = undo_result("session-b", "turn-a", true, "");
        assert_eq!(turn_undo_outcome(&pending("turn-a"), Some("session-a"), &foreign_session), TurnUndoOutcome::Stale);
        let undone: HashMap<String, TurnUndoState> = HashMap::from([("turn-a".to_owned(), TurnUndoState::Undone)]);
        let replayed = undo_result("session-a", "turn-a", true, "");
        assert_eq!(turn_undo_outcome(&undone, Some("session-a"), &replayed), TurnUndoOutcome::Stale);
    }

    #[test]
    fn outcome_rejects_payloads_that_do_not_parse() {
        assert_eq!(turn_undo_outcome(&pending("turn-a"), Some("session-a"), &Value::Null), TurnUndoOutcome::Invalid);
        assert_eq!(
            turn_undo_outcome(&pending("turn-a"), Some("session-a"), &json!({"turnId": "turn-a"})),
            TurnUndoOutcome::Invalid
        );
    }

    #[test]
    fn undo_prefers_the_canonical_turn_identity() {
        let mut item = TranscriptItem::new("row".into(), "assistant".into(), "Agent".into(), "done".into());
        item.turn_id = Some("archive-turn".into());
        assert_eq!(undo_turn_id(&item), Some("archive-turn"));
        item.canonical_turn_id = Some("turn-canonical".into());
        assert_eq!(undo_turn_id(&item), Some("turn-canonical"));
        item.turn_id = None;
        item.canonical_turn_id = None;
        assert_eq!(undo_turn_id(&item), None);
    }
}
