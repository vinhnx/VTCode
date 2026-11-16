//! Response-error routing: how a failed backend request is surfaced.
//!
//! Each method owns its own failure surface (a pane-level error, a
//! silent acknowledgement, or the generic banner); the routing here keeps
//! that decision in one place instead of inside the response dispatcher.

use super::*;

impl DesktopView {
    /// Route a failed backend response.
    ///
    /// `gate_freed` says the request already released the transition gate,
    /// so a waiting handoff may resume once the error state is settled.
    pub(super) fn handle_response_error(
        &mut self,
        method: &str,
        error: String,
        gate_freed: bool,
        cx: &mut Context<Self>,
    ) {
        if method == protocol::method::SESSION_HISTORY {
            self.fail_history_page(error, cx);
            return;
        }
        if self.session_request == Some(method) {
            self.session_request = None;
        }
        if matches!(
            method,
            protocol::method::INITIALIZE
                | protocol::method::SESSION_LIST
                | protocol::method::SESSION_LOAD
                | protocol::method::SESSION_CLOSE
        ) {
            self.fail_pending_navigation();
        }
        if method == protocol::method::SESSION_CLOSE {
            self.pending_new_thread = false;
        }
        if method == protocol::method::CONFIG_OPTIONS {
            self.pending_permission_choice = None;
        }
        if self.handle_webmcp_error(method, &error) {
            cx.notify();
            return;
        }
        if method == protocol::method::COMPOSER_RESOURCES {
            self.resources_loading = false;
        }
        if method == protocol::method::CONFIG_SETTINGS_WRITE {
            self.config_write_pending = false;
            self.config_editor_error = Some(error.clone());
        }
        if method == protocol::method::CONFIG_SECRET_STORE {
            // Form-scoped: the inline error carries it; the
            // generic banner would duplicate it away from the field.
            self.handle_secret_store_error(error.clone(), cx);
            return;
        }
        if method == protocol::method::STORAGE_SUMMARY || method == protocol::method::SESSION_DELETE {
            self.fail_storage_summary(error.clone(), cx);
            return;
        }
        if method == protocol::method::SESSION_RECENT_PROMPTS {
            // Enhancement, not a requirement: older services lack
            // the method, and the live thread still searches fine.
            self.recent_prompts_loading = false;
            return;
        }
        if method == protocol::method::WORKTREE_LIST || method == protocol::method::WORKTREE_REMOVE {
            self.fail_storage_worktrees(error.clone(), cx);
            return;
        }
        if method == protocol::method::UPDATE_ACKNOWLEDGE {
            // Enhancement, not a requirement: an older service
            // lacks the method, and the local banner already
            // retired itself. Never banner for bookkeeping.
            return;
        }
        if method == protocol::method::UPDATE_CHECK && self.update_backfill_in_flight {
            // Best-effort backfill, never user-visible: an offline
            // launch must not paint an error for a quiet refresh.
            self.update_request_pending = false;
            self.update_backfill_in_flight = false;
            return;
        }
        if method == protocol::method::UPDATE_STATUS || method == protocol::method::UPDATE_CHECK {
            self.update_request_pending = false;
        }
        if method == protocol::method::UPDATE_INSTALL {
            // A rejected install must not leave the UI showing an
            // in-flight download; report the failure instead and
            // skip the generic banner below, which would repeat
            // the raw error the row already sanitizes.
            self.update_install_state = protocol::UpdateInstallState::Failed {
                message: rust_i18n::t!("app.service.invalid_updates").to_string(),
            };
            return;
        }
        if method == protocol::method::WORKSPACE_REVIEW {
            self.fail_workspace_review(error);
            return;
        }
        if method == protocol::method::TURN_UNDO {
            self.clear_pending_turn_undos();
        }
        if method == protocol::method::INITIALIZE {
            self.backend = None;
        }
        if method == protocol::method::SESSION_CREATE
            && let Some(draft) = self.first_submission.take()
        {
            self.retained_submission = Some(draft);
        }
        self.error = Some(error);
        if method == protocol::method::TURN_SUBMIT {
            let had_pending = self.pending_submission.is_some();
            self.recover_pending_submission(false);
            if had_pending {
                self.running = false;
                self.cancel_requested = false;
                self.cancel_request_sent = false;
                self.drop_armed_running_switch();
            } else if let Some(index) = self.queued_submissions.iter().position(|entry| !entry.acknowledged) {
                // The failed submit is the oldest unconfirmed
                // queued entry (FIFO); the running turn it waited
                // on is untouched.
                let entry = self.queued_submissions.remove(index);
                self.transcript.remove(&entry.echo_id);
            }
        } else if method == protocol::method::TURN_CANCEL {
            self.cancel_request_sent = false;
            // The service is authoritative: if it rejects a cancel
            // for our live turn, our running state is stale (e.g. a
            // dropped terminal event) — reconcile instead of
            // spinning forever. Correlated against the last-sent
            // turn so a late error cannot clear a newer turn.
            let stale = matches!(
                (&self.cancel_sent_turn_id, &self.active_turn_id),
                (Some(sent), Some(active)) if sent == active
            );
            if stale && let Some(message) = self.error.clone() {
                let was_running = self.running;
                self.finish_unverified_turn(&message);
                if was_running {
                    self.invalidate_webmcp_status();
                    self.drain_armed_running_switch(cx);
                }
            }
        } else if method == protocol::method::TURN_QUEUE_TAKE_LATEST {
            self.take_latest_pending = false;
        } else if method == protocol::method::TURN_STEER {
            // Leave the row queued: a rejected steer must not lose the prompt.
            self.steer_pending_echo_id = None;
        } else if method == protocol::method::TURN_QUEUE_EDIT {
            // Restore the pre-edit prompt the service still holds.
            if let Some((echo_id, original)) = self.queue_edit_pending.take()
                && let Some(entry) = self.queued_submissions.iter_mut().find(|entry| entry.echo_id == echo_id)
            {
                entry.prompt = original;
            }
        }
        // A busy thread is actionable, not generic: surface the close-and-reload /
        // fork remedy in the status line too, so it survives banner dismissal.
        // Detection shares the service's marker so the two cannot drift.
        self.status = if method == protocol::method::SESSION_LOAD
            && let Some(message) = self.error.as_deref()
            && protocol::is_session_busy_message(message)
        {
            message.to_owned()
        } else {
            rust_i18n::t!("app.service.request_failed").to_string()
        };
        if gate_freed {
            // A failed request still frees the transition gate:
            // retry a waiting handoff now that error state is
            // settled. Fatal errors already cleared pending, so
            // this only proceeds for survivable ones.
            self.continue_pending_workspace_navigation(cx);
        }
    }
}
