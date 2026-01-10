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

#[derive(Debug, Deserialize)]
struct AskUserQuestionArgs {
    #[serde(default)]
    title: Option<String>,
    question: String,
    tabs: Vec<AskUserTab>,

    #[serde(default)]
    default_tab_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AskUserTab {
    id: String,
    title: String,
    items: Vec<AskUserItem>,
}

#[derive(Debug, Deserialize)]
struct AskUserItem {
    id: String,
    title: String,

    #[serde(default)]
    subtitle: Option<String>,

    #[serde(default)]
    badge: Option<String>,
}

pub(crate) async fn execute_ask_user_question_tool(
    handle: &InlineHandle,
    session: &mut InlineSession,
    args: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<Value> {
    let parsed: AskUserQuestionArgs =
        serde_json::from_value(args.clone()).context("Invalid ask_user_question arguments")?;

    if parsed.tabs.is_empty() {
        return Ok(json!({
            "cancelled": true,
            "error": "No tabs provided"
        }));
    }

    let title = parsed
        .title
        .clone()
        .unwrap_or_else(|| "Question".to_string());

    let steps: Vec<WizardStep> = parsed
        .tabs
        .iter()
        .map(|tab| {
            let items = tab
                .items
                .iter()
                .map(|item| InlineListItem {
                    title: item.title.clone(),
                    subtitle: item.subtitle.clone(),
                    badge: item.badge.clone(),
                    indent: 0,
                    selection: Some(InlineListSelection::AskUserChoice {
                        tab_id: tab.id.clone(),
                        choice_id: item.id.clone(),
                    }),
                    search_value: Some(format!(
                        "{} {} {}",
                        item.title,
                        item.subtitle.clone().unwrap_or_default(),
                        item.badge.clone().unwrap_or_default()
                    )),
                })
                .collect();

            WizardStep {
                title: tab.title.clone(),
                question: parsed.question.clone(),
                items,
                completed: false,
                answer: None,
            }
        })
        .collect();

    let current_step = parsed
        .default_tab_id
        .as_deref()
        .and_then(|id| parsed.tabs.iter().position(|t| t.id == id))
        .unwrap_or(0);

    // Enable search by default for better UX on larger lists.
    let search = Some(InlineListSearchConfig {
        label: "Search".to_string(),
        placeholder: Some("Type to filter…".to_string()),
    });

    handle.show_tabbed_list_modal(title, steps, current_step, search);
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
            InlineEvent::WizardModalSubmit(mut selections) => {
                ctrl_c_state.disarm_exit();
                handle.close_modal();
                handle.force_redraw();
                task::yield_now().await;
                tokio::time::sleep(Duration::from_millis(100)).await;

                if let Some(InlineListSelection::AskUserChoice { tab_id, choice_id }) =
                    selections.pop()
                {
                    return Ok(json!({
                        "tab_id": tab_id,
                        "choice_id": choice_id
                    }));
                }

                return Ok(json!({"cancelled": true}));
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
