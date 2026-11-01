//! Prompt submission, turn cancellation, and interaction decisions.

use super::*;

impl DesktopView {
    /// Whether submission is blocked on missing provider credentials.
    pub(super) fn credentials_missing(&self) -> bool {
        self.configuration
            .as_ref()
            .is_some_and(|configuration| !configuration.authenticated)
    }

    /// Banner the credential-guidance error for the current configuration.
    pub(super) fn set_provider_auth_error(&mut self) {
        self.error = Some(provider_auth_guidance(self.configuration.as_ref()));
    }

    pub(super) fn submit_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.submit_prompt_with_batching(window, cx, false);
    }

    /// Submit like [`Self::submit_prompt`], but a queued submission merges
    /// with other queued text into a single turn (CLI Ctrl+Enter parity).
    pub(super) fn submit_batched_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.submit_prompt_with_batching(window, cx, true);
    }

    fn submit_prompt_with_batching(&mut self, window: &mut Window, cx: &mut Context<Self>, batchable: bool) {
        let prompt = self.composer.read(cx).value().to_string();
        // Reset before every early return: webmcp and slash handlers consume
        // the draft without reaching the send block below.
        self.reset_history_recall();
        let desktop_slash_action = composer_controls::desktop_slash_action(&prompt);
        if desktop_slash_action.as_ref() == Some(&composer_controls::DesktopSlashAction::StopTurn) {
            let _handled =
                self.handle_desktop_slash_action(composer_controls::DesktopSlashAction::StopTurn, window, cx);
            return;
        }
        if !self.submit_allowed() || self.attachments_picker_open || self.retained_submission.is_some() {
            return;
        }
        if prompt.trim().is_empty() {
            return;
        }
        if self.maybe_handle_webmcp_command(&prompt, window, cx) {
            return;
        }
        if let Some(action) = desktop_slash_action {
            let unavailable_message = match &action {
                composer_controls::DesktopSlashAction::FastMode(_) => {
                    rust_i18n::t!("app.errors.fast_processing_unavailable").to_string()
                }
                composer_controls::DesktopSlashAction::ModelPicker => {
                    rust_i18n::t!("app.errors.model_selection_runtime_changing").to_string()
                }
                composer_controls::DesktopSlashAction::Modes | composer_controls::DesktopSlashAction::Permissions => {
                    rust_i18n::t!("app.errors.runtime_options_unavailable").to_string()
                }
                _ => rust_i18n::t!("app.errors.desktop_command_unavailable").to_string(),
            };
            if !self.handle_desktop_slash_action(action, window, cx) {
                self.error = Some(unavailable_message);
                cx.notify();
            }
            return;
        }
        if !self.workspace_permission_ready() {
            self.error = Some(rust_i18n::t!("app.composer.permission_restore_hint").to_string());
            cx.notify();
            return;
        }
        // Local slash commands above stay available so the provider can be
        // switched; only turn submission needs credentials.
        if self.credentials_missing() {
            self.set_provider_auth_error();
            cx.notify();
            return;
        }
        let draft = SubmissionDraft {
            prompt,
            attachments: std::mem::take(&mut self.attachments),
            echo_id: format!("desktop-user-{}", uuid_like_id()),
            delivery_uncertain: false,
        };
        let sent = if self.selected_session_id.is_none() {
            if self.request_session_change(protocol::method::SESSION_CREATE, json!({ "title": null }), cx) {
                self.first_submission = Some(draft);
                self.status = rust_i18n::t!("app.composer.creating_session").to_string();
                true
            } else {
                self.attachments = draft.attachments;
                false
            }
        } else if self.running {
            match self.enqueue_queued_submission(draft, batchable, cx) {
                Ok(()) => true,
                Err(draft) => {
                    self.attachments = draft.attachments;
                    false
                }
            }
        } else {
            match self.enqueue_submission(draft, cx) {
                Ok(()) => true,
                Err(draft) => {
                    self.attachments = draft.attachments;
                    false
                }
            }
        };
        if sent {
            self.composer.update(cx, |state, cx| state.set_value("", window, cx));
        }
        cx.notify();
    }

    /// The original Send intent can cross session creation without reading a newer composer draft.
    pub(super) fn enqueue_submission(
        &mut self,
        draft: SubmissionDraft,
        cx: &mut Context<Self>,
    ) -> Result<(), SubmissionDraft> {
        let Some(session_id) = self.selected_session_id.clone() else {
            return Err(draft);
        };
        // A deferred first submission can outlive the configuration it was
        // written under; re-check so a revoked credential fails with guidance
        // instead of a doomed turn.
        if self.credentials_missing() {
            self.set_provider_auth_error();
            return Err(draft);
        }
        let params =
            json!({ keys::SESSION_ID: session_id, keys::PROMPT: draft.prompt, keys::ATTACHMENTS: draft.attachments });
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
        self.pending_submission = Some(draft);
        self.pending_first_submission = false;
        self.transcript.follow_latest();
        self.running = true;
        self.turn_content_seen = false;
        self.active_turn_id = None;
        self.cancel_requested = false;
        self.cancel_request_sent = false;
        self.status = rust_i18n::t!("app.composer.submitting_turn").to_string();
        cx.notify();
        Ok(())
    }

    pub(super) fn recover_pending_submission(&mut self, delivery_uncertain: bool) {
        self.pending_first_submission = false;
        if let Some(mut draft) = self.pending_submission.take() {
            draft.delivery_uncertain = delivery_uncertain;
            if !delivery_uncertain {
                self.transcript.remove(&draft.echo_id);
            }
            self.retained_submission = Some(draft);
        }
    }

    pub(super) fn restore_submission(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.composer.read(cx).value().is_empty()
            || !self.attachments.is_empty()
            || self.attachments_picker_open
            || self.session_request.is_some()
        {
            return;
        }
        if let Some(draft) = self.retained_submission.take() {
            self.transcript.remove(&draft.echo_id);
            self.replace_composer_text(draft.prompt, window, cx);
            self.reset_history_recall();
            self.attachments = draft.attachments;
            self.clear_error();
            self.status = rust_i18n::t!("app.composer.draft_restored").to_string();
            cx.notify();
        }
    }

    pub(super) fn cancel_turn(&mut self, cx: &mut Context<Self>) {
        if !self.running {
            return;
        }
        self.cancel_requested = true;
        self.status = rust_i18n::t!("app.composer.cancelling_turn").to_string();
        self.try_send_pending_cancel();
        cx.notify();
    }

    pub(super) fn try_send_pending_cancel(&mut self) {
        if !self.cancel_requested || self.cancel_request_sent || !self.running {
            return;
        }
        let (Some(session_id), Some(turn_id)) = (self.selected_session_id.clone(), self.active_turn_id.clone()) else {
            self.status = rust_i18n::t!("app.composer.cancellation_queued").to_string();
            return;
        };
        if self.request(protocol::method::TURN_CANCEL, json!({ keys::SESSION_ID: session_id, keys::TURN_ID: turn_id }))
        {
            self.cancel_request_sent = true;
        } else {
            self.error = Some(rust_i18n::t!("app.composer.cancellation_not_queued").to_string());
        }
    }

    pub(super) fn handle_cancel_state_after_turn_event(&mut self) {
        if !self.running {
            self.cancel_requested = false;
            self.cancel_request_sent = false;
            return;
        }
        self.try_send_pending_cancel();
    }

    pub(super) fn clear_pending_interaction(&mut self) {
        self.interaction = None;
        self.pending_interaction_response = None;
        self.interaction_choice_ids.clear();
        self.interaction_answer_texts.clear();
        self.interaction_step_index = 0;
    }

    pub(super) fn submit_interaction(
        &mut self,
        choice_ids: Vec<String>,
        cancelled: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending_interaction_response.is_some() {
            return;
        }
        let Some(interaction) = self.interaction.clone() else {
            return;
        };
        self.store_current_interaction_answer(cx);
        let answer_texts = if cancelled {
            Vec::new()
        } else {
            self.interaction_answer_texts.clone()
        };
        let choice_ids = if cancelled { Vec::new() } else { choice_ids };
        let response = InteractionResponse {
            session_id: interaction.session_id,
            turn_id: interaction.turn_id,
            interaction_id: interaction.interaction_id,
            choice_id: choice_ids.first().cloned(),
            choice_ids,
            text: answer_texts.iter().flatten().next().cloned(),
            answer_texts,
            cancelled,
        };
        let params = json!(response);
        if !request_fits_frame(protocol::method::INTERACTION_RESPOND, &params) {
            self.error = Some(rust_i18n::t!("app.composer.answers_too_long").to_string());
            cx.notify();
            return;
        }
        if self.request(protocol::method::INTERACTION_RESPOND, params) {
            self.pending_interaction_response = Some(response.interaction_id);
            self.clear_error();
            self.status = rust_i18n::t!(i18n::COMPOSER_SENDING_DECISION).to_string();
        } else {
            self.error = Some(rust_i18n::t!("app.composer.decision_not_sent").to_string());
        }
        cx.notify();
    }

    pub(super) fn choose_interaction_option(
        &mut self,
        step_index: usize,
        choice_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending_interaction_response.is_some() {
            return;
        }
        let Some(interaction) = self.interaction.as_ref() else {
            return;
        };
        if interaction.kind == protocol::InteractionKind::Question && !interaction.steps.is_empty() {
            if step_index != self.interaction_step_index
                || !interaction
                    .steps
                    .get(step_index)
                    .is_some_and(|step| step.choices.iter().any(|choice| choice.id == choice_id))
            {
                return;
            }
            if let Some(selected) = self.interaction_choice_ids.get_mut(step_index) {
                *selected = Some(choice_id);
            }
            self.clear_error();
            cx.notify();
            return;
        }
        self.submit_interaction(vec![choice_id], false, window, cx);
    }

    fn store_current_interaction_answer(&mut self, cx: &App) {
        if self.interaction.as_ref().is_none_or(|interaction| interaction.steps.is_empty()) {
            return;
        }
        let answer = self.question_answer.read(cx).value().to_string();
        if let Some(stored) = self.interaction_answer_texts.get_mut(self.interaction_step_index) {
            *stored = (!answer.trim().is_empty()).then_some(answer);
        }
    }

    pub(super) fn show_interaction_step(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_interaction_response.is_some()
            || index == self.interaction_step_index
            || self.interaction.as_ref().is_none_or(|interaction| {
                interaction.kind != protocol::InteractionKind::Question || interaction.steps.get(index).is_none()
            })
        {
            return;
        }
        self.store_current_interaction_answer(cx);
        self.interaction_step_index = index;
        let answer = self
            .interaction_answer_texts
            .get(index)
            .and_then(Clone::clone)
            .unwrap_or_default();
        self.question_answer.update(cx, |state, cx| state.set_value(answer, window, cx));
        self.clear_error();
        cx.notify();
    }

    pub(super) fn advance_interaction_step(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_interaction_response.is_some() {
            return;
        }
        let Some(interaction) = self.interaction.as_ref() else {
            return;
        };
        let selected = self
            .interaction_choice_ids
            .get(self.interaction_step_index)
            .and_then(Option::as_deref);
        if !interaction
            .steps
            .get(self.interaction_step_index)
            .is_some_and(|step| step.choices.iter().any(|choice| Some(choice.id.as_str()) == selected))
        {
            self.error = Some(rust_i18n::t!("app.composer.choose_option").to_string());
            cx.notify();
            return;
        }
        let final_step = self.interaction_step_index + 1 >= interaction.steps.len();
        if final_step {
            let selections = self.interaction_choice_ids.iter().filter_map(Clone::clone).collect::<Vec<_>>();
            if selections.len() != interaction.steps.len() {
                self.error = Some(rust_i18n::t!("app.composer.choose_all_options").to_string());
                cx.notify();
                return;
            }
            self.submit_interaction(selections, false, window, cx);
        } else {
            self.show_interaction_step(self.interaction_step_index + 1, window, cx);
        }
    }
}
