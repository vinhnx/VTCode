use crate::agent::runloop::ui_list;
use crate::agent::runloop::ui_list::Tone;
use anyhow::{Context, Result};
use hashbrown::HashMap;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::Notify;
use vtcode_ui::tui::app::{
    InlineHandle, InlineListItem, InlineListSelection, InlineMessageKind, InlineSegment, InlineSession,
    InlineTextStyle, WizardStep,
};

use super::super::state::CtrlCState;
use super::super::wizard_modal::{WizardModalOutcome, show_wizard_modal_and_wait};
use super::options::{ensure_recommended_first, resolve_question_options};
use super::schema::{
    NormalizedRequestUserInput, RequestUserInputAnswer, RequestUserInputOption, RequestUserInputQuestion,
    RequestUserInputResponse, normalize_request_user_input_args,
};
#[cfg(test)]
use super::suggestions::generate_suggested_options;

#[cfg_attr(feature = "profiling", hotpath::measure)]
pub(crate) async fn execute_request_user_input_tool(
    handle: &InlineHandle,
    session: &mut InlineSession,
    args: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<Value> {
    let NormalizedRequestUserInput {
        args: parsed,
        wizard_mode,
        current_step,
        title_override,
        allow_freeform,
        freeform_label,
        freeform_placeholder,
    } = normalize_request_user_input_args(args).context("Invalid request_user_input arguments")?;

    if parsed.questions.is_empty() {
        return Ok(json!({
            "cancelled": true,
            "error": "No questions provided"
        }));
    }

    let resolved_options = resolve_question_options(&parsed.questions);
    let steps: Vec<WizardStep> = parsed
        .questions
        .iter()
        .zip(resolved_options)
        .map(|(q, options)| {
            let items = build_question_items_with_options(q, options);

            WizardStep {
                title: q.header.clone(),
                question: q.question.clone(),
                items,
                completed: false,
                answer: None,
                allow_freeform,
                freeform_label: freeform_label.clone(),
                freeform_placeholder: freeform_placeholder.clone(),
                freeform_default: None,
            }
        })
        .collect();

    let title = title_override.unwrap_or_else(|| {
        if steps.len() == 1 {
            steps[0].title.clone()
        } else {
            "Questions".to_string()
        }
    });

    let safe_current_step = current_step.min(steps.len().saturating_sub(1));
    let _status_wait = handle.program_status_wait(vtcode_commons::program_status::InteractionKind::Question);
    match show_wizard_modal_and_wait(
        handle,
        session,
        title,
        steps,
        safe_current_step,
        None,
        wizard_mode,
        ctrl_c_state,
        ctrl_c_notify,
    )
    .await?
    {
        WizardModalOutcome::Submitted(selections) => {
            let mut answers: HashMap<String, RequestUserInputAnswer> = HashMap::new();

            for selection in selections {
                if let InlineListSelection::RequestUserInputAnswer { question_id, selected, other } = selection {
                    answers.insert(question_id, RequestUserInputAnswer { selected, other });
                }
            }

            append_summary_lines(handle, &parsed.questions, &answers, wizard_mode);

            let response = RequestUserInputResponse { answers };
            serde_json::to_value(response).map_err(|e| anyhow::anyhow!("Failed to serialize response: {e}"))
        }
        WizardModalOutcome::Cancelled { signal } => {
            if let Some(signal) = signal {
                Ok(json!({"cancelled": true, "signal": signal}))
            } else {
                Ok(json!({"cancelled": true}))
            }
        }
    }
}

#[cfg(test)]
pub(super) fn build_question_items(question: &RequestUserInputQuestion) -> Vec<InlineListItem> {
    let options = question.options.clone().or_else(|| generate_suggested_options(question));
    build_question_items_with_options(question, options)
}

pub(super) fn build_question_items_with_options(
    question: &RequestUserInputQuestion,
    options: Option<Vec<RequestUserInputOption>>,
) -> Vec<InlineListItem> {
    let options = options.map(ensure_recommended_first);

    if let Some(options) = options {
        let mut items: Vec<InlineListItem> = options
            .iter()
            .enumerate()
            .map(|(index, opt)| InlineListItem {
                title: format!("{}. {}", index + 1, opt.label),
                subtitle: Some(opt.description.clone()),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::RequestUserInputAnswer {
                    question_id: question.id.clone(),
                    selected: vec![opt.label.clone()],
                    other: None,
                }),
                search_value: Some(format!("{} {}", opt.label, opt.description)),
                ..Default::default()
            })
            .collect();

        items.push(InlineListItem {
            title: format!("{}. Custom note (inline)", options.len() + 1),
            subtitle: Some("Type your custom response inline, then press Enter to continue".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: question.id.clone(),
                selected: vec![],
                other: Some(String::new()),
            }),
            search_value: Some("custom note other custom response free text".to_string()),
            ..Default::default()
        });
        items
    } else {
        vec![ui_list::action(
            "Enter your response...",
            "Type your answer in the input field".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::RequestUserInputAnswer {
                question_id: question.id.clone(),
                selected: vec![],
                other: Some(String::new()),
            }),
        )]
    }
}

fn append_summary_lines(
    handle: &InlineHandle,
    questions: &[RequestUserInputQuestion],
    answers: &HashMap<String, RequestUserInputAnswer>,
    wizard_mode: vtcode_ui::tui::app::WizardModalMode,
) {
    let summary_style = Arc::new(InlineTextStyle::default());
    let summary_segment = |text: String| InlineSegment { text, style: summary_style.clone() };

    if wizard_mode == vtcode_ui::tui::app::WizardModalMode::TabbedList {
        handle.append_line(InlineMessageKind::Info, vec![summary_segment("• Selection captured".to_string())]);
        // Mirror UI-only append into the shared transcript log so tracker
        // replace decisions do not treat a stale TRANSCRIPT tail as current.
        vtcode_core::utils::transcript::append("• Selection captured");
        return;
    }

    let answered_count = answers.len();
    let total_count = questions.len();
    let questions_summary = format!("• Questions {answered_count}/{total_count} answered");
    handle.append_line(InlineMessageKind::Info, vec![summary_segment(questions_summary.clone())]);
    vtcode_core::utils::transcript::append(&questions_summary);

    for question in questions {
        let question_line = format!("  • {}", question.question);
        handle.append_line(InlineMessageKind::Info, vec![summary_segment(question_line.clone())]);
        vtcode_core::utils::transcript::append(&question_line);
        let answer_text = answers
            .get(&question.id)
            .map(|answer| {
                let mut parts = Vec::new();
                if !answer.selected.is_empty() {
                    parts.push(answer.selected.join(", "));
                }
                if let Some(other) = answer.other.as_ref().map(|text| text.trim()).filter(|text| !text.is_empty()) {
                    if parts.is_empty() {
                        parts.push(other.to_string());
                    } else {
                        parts.push(format!("notes: {other}"));
                    }
                }
                if parts.is_empty() {
                    "(unanswered)".to_string()
                } else {
                    parts.join(" — ")
                }
            })
            .unwrap_or_else(|| "(unanswered)".to_string());
        let answer_line = format!("    answer: {answer_text}");
        handle.append_line(InlineMessageKind::Info, vec![summary_segment(answer_line.clone())]);
        vtcode_core::utils::transcript::append(&answer_line);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use tokio::sync::{Notify, mpsc};

    use super::execute_request_user_input_tool;
    use crate::agent::runloop::unified::state::CtrlCState;
    use vtcode_ui::tui::app::{
        InlineCommand, InlineEvent, InlineHandle, InlineListSelection, InlineSession, TransientEvent, TransientRequest,
        TransientSubmission,
    };

    fn next_transient_request(command_rx: &mut mpsc::UnboundedReceiver<InlineCommand>) -> TransientRequest {
        loop {
            match command_rx.try_recv() {
                Ok(InlineCommand::ShowTransient { request }) => return *request,
                Ok(_) => {}
                Err(error) => panic!("expected a transient request, got {error:?}"),
            }
        }
    }

    fn test_handles() -> (
        InlineHandle,
        mpsc::UnboundedReceiver<InlineCommand>,
        mpsc::UnboundedSender<InlineEvent>,
        InlineSession,
    ) {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(command_tx);
        let session = InlineSession {
            handle: handle.clone(),
            events: event_rx,
            worker: None,
        };
        (handle, command_rx, event_tx, session)
    }

    #[tokio::test]
    async fn request_user_input_shows_wizard_and_returns_selected_answer() {
        let (handle, mut command_rx, event_tx, mut session) = test_handles();
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());
        let args = json!({
            "questions": [{
                "id": "goal",
                "header": "Goal",
                "question": "What should this change deliver?",
                "options": [{
                    "label": "Small scope",
                    "description": "Keep the implementation focused."
                }, {
                    "label": "Broader scope",
                    "description": "Cover adjacent behavior in the same change."
                }]
            }]
        });

        event_tx
            .send(InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Wizard(vec![
                InlineListSelection::RequestUserInputAnswer {
                    question_id: "goal".to_string(),
                    selected: vec!["Small scope".to_string()],
                    other: None,
                },
            ]))))
            .expect("send wizard answer");

        let output = execute_request_user_input_tool(&handle, &mut session, &args, &ctrl_c_state, &ctrl_c_notify)
            .await
            .expect("request_user_input result");

        assert_eq!(output["answers"]["goal"]["selected"], json!(["Small scope"]));
        match next_transient_request(&mut command_rx) {
            TransientRequest::Wizard(request) => {
                assert_eq!(request.title, "Goal");
                assert_eq!(request.steps.len(), 1);
                assert_eq!(request.steps[0].question, "What should this change deliver?");
                assert!(request.steps[0].items.iter().any(|item| {
                    item.title.contains("Small scope")
                        && matches!(
                            item.selection.as_ref(),
                            Some(InlineListSelection::RequestUserInputAnswer { question_id, .. })
                                if question_id == "goal"
                        )
                }));
            }
            other => panic!("expected wizard request, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn request_user_input_cancellation_returns_cancelled_without_answers() {
        let (handle, mut command_rx, event_tx, mut session) = test_handles();
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());
        let args = json!({
            "questions": [{
                "id": "goal",
                "header": "Goal",
                "question": "What should this change deliver?"
            }]
        });

        event_tx
            .send(InlineEvent::Transient(TransientEvent::Cancelled))
            .expect("send wizard cancellation");

        let output = execute_request_user_input_tool(&handle, &mut session, &args, &ctrl_c_state, &ctrl_c_notify)
            .await
            .expect("cancelled request_user_input result");

        assert_eq!(output["cancelled"], json!(true));
        assert!(output.get("answers").is_none());
        assert!(matches!(next_transient_request(&mut command_rx), TransientRequest::Wizard(_)));
        loop {
            match command_rx.try_recv() {
                Ok(InlineCommand::CloseTransient) => break,
                Ok(_) => {}
                Err(error) => panic!("expected close command, got {error:?}"),
            }
        }
    }
}
