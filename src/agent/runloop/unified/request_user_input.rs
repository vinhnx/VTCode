use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Notify;
use tokio::task;

use vtcode_core::ui::tui::{
    InlineHandle, InlineListItem, InlineListSelection, InlineMessageKind, InlineSegment,
    InlineSession, InlineTextStyle, WizardModalMode, WizardStep,
};

use super::state::CtrlCState;
use super::wizard_modal::{WizardModalOutcome, wait_for_wizard_modal};

/// Arguments parsed from the request_user_input tool call.
#[derive(Debug, Deserialize)]
struct RequestUserInputArgs {
    questions: Vec<RequestUserInputQuestion>,
}

#[derive(Debug, Deserialize)]
struct RequestUserInputQuestion {
    id: String,
    header: String,
    question: String,

    #[serde(default)]
    options: Option<Vec<RequestUserInputOption>>,
}

#[derive(Debug, Deserialize)]
struct RequestUserInputOption {
    label: String,
    description: String,
}

/// Response format matching Codex's request_user_input tool output.
#[derive(Debug, serde::Serialize)]
struct RequestUserInputResponse {
    answers: HashMap<String, RequestUserInputAnswer>,
}

#[derive(Debug, serde::Serialize)]
struct RequestUserInputAnswer {
    selected: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    other: Option<String>,
}

/// Execute the request_user_input HITL tool.
///
/// This handler displays a wizard modal with the questions, collects user responses,
/// and returns them in a structured format matching Codex's output schema.
pub(crate) async fn execute_request_user_input_tool(
    handle: &InlineHandle,
    session: &mut InlineSession,
    args: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<Value> {
    let parsed: RequestUserInputArgs =
        serde_json::from_value(args.clone()).context("Invalid request_user_input arguments")?;

    if parsed.questions.is_empty() {
        return Ok(json!({
            "cancelled": true,
            "error": "No questions provided"
        }));
    }

    // Build wizard steps from questions
    let steps: Vec<WizardStep> = parsed
        .questions
        .iter()
        .map(|q| {
            let items = if let Some(ref options) = q.options {
                options
                    .iter()
                    .enumerate()
                    .map(|(index, opt)| InlineListItem {
                        title: format!("{}. {}", index + 1, opt.label),
                        subtitle: Some(opt.description.clone()),
                        badge: None,
                        indent: 0,
                        selection: Some(InlineListSelection::RequestUserInputAnswer {
                            question_id: q.id.clone(),
                            selected: vec![opt.label.clone()],
                            other: None,
                        }),
                        search_value: Some(format!("{} {}", opt.label, opt.description)),
                    })
                    .collect()
            } else {
                // Free-form question - show a single "Enter text..." option
                vec![InlineListItem {
                    title: "Enter your response...".to_string(),
                    subtitle: Some("Type your answer in the input field".to_string()),
                    badge: None,
                    indent: 0,
                    selection: Some(InlineListSelection::RequestUserInputAnswer {
                        question_id: q.id.clone(),
                        selected: vec![],
                        other: Some(String::new()),
                    }),
                    search_value: None,
                }]
            };

            WizardStep {
                title: q.header.clone(),
                question: q.question.clone(),
                items,
                completed: false,
                answer: None,
                allow_freeform: true,
                freeform_label: None,
                freeform_placeholder: None,
            }
        })
        .collect();

    let title = if steps.len() == 1 {
        steps[0].title.clone()
    } else {
        "Questions".to_string()
    };

    let search = None;

    handle.show_wizard_modal_with_mode(title, steps, 0, search, WizardModalMode::MultiStep);
    handle.force_redraw();
    task::yield_now().await;

    match wait_for_wizard_modal(handle, session, ctrl_c_state, ctrl_c_notify).await? {
        WizardModalOutcome::Submitted(selections) => {
            // Convert selections to response format
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

            let answered_count = answers.len();
            let total_count = parsed.questions.len();
            let summary_style = std::sync::Arc::new(InlineTextStyle::default());
            let summary_segment = |text: String| InlineSegment {
                text,
                style: summary_style.clone(),
            };

            handle.append_line(
                InlineMessageKind::Info,
                vec![summary_segment(format!(
                    "• Questions {}/{} answered",
                    answered_count, total_count
                ))],
            );

            for question in &parsed.questions {
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
