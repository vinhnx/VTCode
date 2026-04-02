use anyhow::{Context, Result};
use hashbrown::HashMap;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::Notify;
use vtcode_tui::app::{
    InlineHandle, InlineListItem, InlineListSelection, InlineMessageKind, InlineSegment,
    InlineSession, InlineTextStyle, WizardStep,
};

use super::super::state::CtrlCState;
use super::super::wizard_modal::{WizardModalOutcome, show_wizard_modal_and_wait};
use super::options::{ensure_recommended_first, resolve_question_options};
use super::schema::{
    NormalizedRequestUserInput, RequestUserInputAnswer, RequestUserInputOption,
    RequestUserInputQuestion, RequestUserInputResponse, normalize_request_user_input_args,
};
#[cfg(test)]
use super::suggestions::generate_suggested_options;

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
                if let InlineListSelection::RequestUserInputAnswer {
                    question_id,
                    selected,
                    other,
                } = selection
                {
                    answers.insert(question_id, RequestUserInputAnswer { selected, other });
                }
            }

            append_summary_lines(handle, &parsed.questions, &answers, wizard_mode);

            let response = RequestUserInputResponse { answers };
            serde_json::to_value(response)
                .map_err(|e| anyhow::anyhow!("Failed to serialize response: {}", e))
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
    let options = question
        .options
        .clone()
        .or_else(|| generate_suggested_options(question));
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
            })
            .collect();

        items.push(InlineListItem {
            title: format!("{}. Custom note (inline)", options.len() + 1),
            subtitle: Some(
                "Type your custom response inline, then press Enter to continue".to_string(),
            ),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: question.id.clone(),
                selected: vec![],
                other: Some(String::new()),
            }),
            search_value: Some("custom note other custom response free text".to_string()),
        });
        items
    } else {
        vec![InlineListItem {
            title: "Enter your response...".to_string(),
            subtitle: Some("Type your answer in the input field".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: question.id.clone(),
                selected: vec![],
                other: Some(String::new()),
            }),
            search_value: None,
        }]
    }
}

fn append_summary_lines(
    handle: &InlineHandle,
    questions: &[RequestUserInputQuestion],
    answers: &HashMap<String, RequestUserInputAnswer>,
    wizard_mode: vtcode_tui::app::WizardModalMode,
) {
    let summary_style = Arc::new(InlineTextStyle::default());
    let summary_segment = |text: String| InlineSegment {
        text,
        style: summary_style.clone(),
    };

    if wizard_mode == vtcode_tui::app::WizardModalMode::TabbedList {
        handle.append_line(
            InlineMessageKind::Info,
            vec![summary_segment("• Selection captured".to_string())],
        );
        return;
    }

    let answered_count = answers.len();
    let total_count = questions.len();
    handle.append_line(
        InlineMessageKind::Info,
        vec![summary_segment(format!(
            "• Questions {}/{} answered",
            answered_count, total_count
        ))],
    );

    for question in questions {
        handle.append_line(
            InlineMessageKind::Info,
            vec![summary_segment(format!("  • {}", question.question))],
        );
        let answer_text = answers
            .get(&question.id)
            .map(|answer| {
                let mut parts = Vec::new();
                if !answer.selected.is_empty() {
                    parts.push(answer.selected.join(", "));
                }
                if let Some(other) = answer
                    .other
                    .as_ref()
                    .map(|text| text.trim())
                    .filter(|text| !text.is_empty())
                {
                    if parts.is_empty() {
                        parts.push(other.to_string());
                    } else {
                        parts.push(format!("notes: {}", other));
                    }
                }
                if parts.is_empty() {
                    "(unanswered)".to_string()
                } else {
                    parts.join(" — ")
                }
            })
            .unwrap_or_else(|| "(unanswered)".to_string());
        handle.append_line(
            InlineMessageKind::Info,
            vec![summary_segment(format!("    answer: {}", answer_text))],
        );
    }
}
