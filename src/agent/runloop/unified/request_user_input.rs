use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::Notify;
use tokio::task;

use vtcode_core::ui::tui::{
    InlineEvent, InlineHandle, InlineListItem, InlineListSearchConfig, InlineListSelection,
    InlineSession, WizardStep,
};

use super::state::{CtrlCSignal, CtrlCState};

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
                    
                    .map(|opt| InlineListItem {
                        title: opt.label.clone(),
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
            }
        })
        .collect();

    let title = if steps.len() == 1 {
        steps[0].title.clone()
    } else {
        "Questions".to_string()
    };

    // Enable search for better UX
    let search = Some(InlineListSearchConfig {
        label: "Search".to_string(),
        placeholder: Some("Type to filter…".to_string()),
    });

    handle.show_tabbed_list_modal(title, steps, 0, search);
    handle.force_redraw();
    task::yield_now().await;

    loop {
        if ctrl_c_state.is_cancel_requested() {
            handle.close_modal();
            handle.force_redraw();
            task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(100)).await;
            return Ok(json!({"cancelled": true}));
        }

        let notify = ctrl_c_notify.clone();
        let maybe_event = tokio::select! {
            _ = notify.notified() => None,
            event = session.next_event() => event,
        };

        let Some(event) = maybe_event else {
            handle.close_modal();
            handle.force_redraw();
            task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(100)).await;
            return Ok(json!({"cancelled": true}));
        };

        match event {
            InlineEvent::Interrupt => {
                let signal = if ctrl_c_state.is_exit_requested() {
                    CtrlCSignal::Exit
                } else if ctrl_c_state.is_cancel_requested() {
                    CtrlCSignal::Cancel
                } else {
                    ctrl_c_state.register_signal()
                };
                ctrl_c_notify.notify_waiters();
                handle.close_modal();
                handle.force_redraw();
                task::yield_now().await;
                tokio::time::sleep(Duration::from_millis(100)).await;

                return Ok(json!({
                    "cancelled": true,
                    "signal": match signal {
                        CtrlCSignal::Exit => "exit",
                        CtrlCSignal::Cancel => "cancel",
                    }
                }));
            }
            InlineEvent::WizardModalSubmit(selections) => {
                ctrl_c_state.disarm_exit();
                handle.close_modal();
                handle.force_redraw();
                task::yield_now().await;
                tokio::time::sleep(Duration::from_millis(100)).await;

                // Convert selections to response format
                let mut answers: HashMap<String, RequestUserInputAnswer> = HashMap::new();

                for selection in selections {
                    if let InlineListSelection::RequestUserInputAnswer {
                        question_id,
                        selected,
                        other,
                    } = selection
                    {
                        answers.insert(
                            question_id,
                            RequestUserInputAnswer {
                                selected,
                                other,
                            },
                        );
                    }
                }

                let response = RequestUserInputResponse { answers };
                return serde_json::to_value(response)
                    .map_err(|e| anyhow::anyhow!("Failed to serialize response: {}", e));
            }
            InlineEvent::WizardModalCancel | InlineEvent::ListModalCancel | InlineEvent::Cancel => {
                ctrl_c_state.disarm_exit();
                handle.close_modal();
                handle.force_redraw();
                task::yield_now().await;
                tokio::time::sleep(Duration::from_millis(100)).await;
                return Ok(json!({"cancelled": true}));
            }
            InlineEvent::Exit => {
                ctrl_c_state.disarm_exit();
                handle.close_modal();
                handle.force_redraw();
                task::yield_now().await;
                tokio::time::sleep(Duration::from_millis(100)).await;
                return Ok(json!({"cancelled": true, "signal": "exit"}));
            }
            InlineEvent::Submit(_) | InlineEvent::QueueSubmit(_) => {
                // Ignore text input while modal is shown.
                continue;
            }
            _ => {}
        }
    }
}
