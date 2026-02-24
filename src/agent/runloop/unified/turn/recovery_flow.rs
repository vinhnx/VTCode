use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Notify;
use tokio::task;

use vtcode_core::core::agent::error_recovery::{ErrorType, RecoveryDiagnostics};
use vtcode_core::tools::circuit_breaker::ToolCircuitDiagnostics;
use vtcode_core::ui::tui::{
    InlineEvent, InlineHandle, InlineListItem, InlineListSearchConfig, InlineListSelection,
    InlineSession, WizardStep,
};

use crate::agent::runloop::unified::state::{CtrlCSignal, CtrlCState};

#[allow(dead_code)]
pub struct RecoveryPromptBuilder {
    pub title: String,
    pub summary: String,
    pub recommendations: Vec<RecoveryOption>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RecoveryOption {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub badge: Option<String>,
    pub action: RecoveryAction,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum RecoveryAction {
    RetryTool { tool_name: String },
    ResetAllCircuits,
    SkipStep,
    TryAlternative,
    ShowErrorLog,
    RunDiagnostics,
    CallDebugAgent,
    SaveAndExit,
    Continue,
}

#[derive(Debug, Deserialize)]
struct RecoveryPromptArgs {
    #[serde(default)]
    title: Option<String>,
    question: String,
    tabs: Vec<RecoveryTabArgs>,
    #[serde(default)]
    default_tab_id: Option<String>,
    #[serde(default)]
    default_choice_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RecoveryTabArgs {
    id: String,
    title: String,
    items: Vec<RecoveryItemArgs>,
}

#[derive(Debug, Clone, Deserialize)]
struct RecoveryItemArgs {
    id: String,
    title: String,
    #[serde(default)]
    subtitle: Option<String>,
    #[serde(default)]
    badge: Option<String>,
}

fn resolve_current_step(tabs: &[RecoveryTabArgs], default_tab_id: Option<&str>) -> usize {
    default_tab_id
        .and_then(|id| tabs.iter().position(|tab| tab.id == id))
        .unwrap_or(0)
}

fn find_default_choice_selection(
    items: &[InlineListItem],
    default_choice_id: &str,
) -> Option<InlineListSelection> {
    items.iter().find_map(|item| {
        if let Some(InlineListSelection::AskUserChoice { choice_id, .. }) = &item.selection
            && choice_id == default_choice_id
        {
            return item.selection.clone();
        }
        None
    })
}

#[allow(dead_code)]
impl RecoveryPromptBuilder {
    pub fn new(title: String) -> Self {
        Self {
            title,
            summary: String::new(),
            recommendations: Vec::new(),
        }
    }

    pub fn with_summary(mut self, summary: String) -> Self {
        self.summary = summary;
        self
    }

    pub fn add_recommendation(mut self, option: RecoveryOption) -> Self {
        self.recommendations.push(option);
        self
    }

    pub fn build(self) -> Value {
        #[derive(Debug, Clone, Serialize)]
        struct RecoveryTabInternal {
            id: String,
            title: String,
            items: Vec<RecoveryItemInternal>,
        }

        #[derive(Debug, Clone, Serialize)]
        struct RecoveryItemInternal {
            id: String,
            title: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            subtitle: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            badge: Option<String>,
        }

        let mut default_choice_id: Option<String> = None;
        let items: Vec<RecoveryItemInternal> = self
            .recommendations
            .into_iter()
            .map(|r| {
                if default_choice_id.is_none() && r.badge.as_deref() == Some("Recommended") {
                    default_choice_id = Some(r.id.clone());
                }
                RecoveryItemInternal {
                    id: r.id,
                    title: r.title,
                    subtitle: Some(r.subtitle),
                    badge: r.badge,
                }
            })
            .collect();
        if default_choice_id.is_none() {
            default_choice_id = items.first().map(|item| item.id.clone());
        }

        let tabs = vec![RecoveryTabInternal {
            id: "recovery".to_string(),
            title: "Recovery Options".to_string(),
            items,
        }];

        json!({
            "title": self.title,
            "question": self.summary,
            "tabs": tabs,
            "allow_freeform": true,
            "freeform_label": "Provide custom guidance",
            "freeform_placeholder": "Describe what you'd like me to do next...",
            "default_tab_id": "recovery",
            "default_choice_id": default_choice_id
        })
    }
}

#[allow(dead_code)]
pub async fn execute_recovery_prompt(
    handle: &InlineHandle,
    session: &mut InlineSession,
    args: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<Value> {
    let parsed: RecoveryPromptArgs =
        serde_json::from_value(args.clone()).context("Invalid recovery prompt arguments")?;

    if parsed.tabs.is_empty() {
        return Ok(json!({ "cancelled": true, "error": "No tabs provided" }));
    }
    if parsed.tabs.iter().any(|tab| tab.items.is_empty()) {
        return Ok(json!({
            "cancelled": true,
            "error": "Each tab must include at least one item"
        }));
    }

    let title = parsed
        .title
        .unwrap_or_else(|| "Circuit Breaker Activated".to_string());
    let current_step = resolve_current_step(&parsed.tabs, parsed.default_tab_id.as_deref());
    let default_tab_id = parsed.tabs.get(current_step).map(|tab| tab.id.as_str());

    let steps: Vec<WizardStep> = parsed
        .tabs
        .iter()
        .map(|tab| {
            let items: Vec<InlineListItem> = tab
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
                        text: None,
                    }),
                    search_value: Some(format!(
                        "{} {} {}",
                        item.title,
                        item.subtitle.clone().unwrap_or_default(),
                        item.badge.clone().unwrap_or_default()
                    )),
                })
                .collect();

            let answer = if default_tab_id == Some(tab.id.as_str()) {
                parsed
                    .default_choice_id
                    .as_deref()
                    .and_then(|choice_id| find_default_choice_selection(&items, choice_id))
            } else {
                None
            };

            WizardStep {
                title: tab.title.clone(),
                question: parsed.question.clone(),
                items,
                completed: answer.is_some(),
                answer,
                allow_freeform: true, // Recovery prompts often allow custom guidance
                freeform_label: Some("Provide custom guidance".to_string()),
                freeform_placeholder: Some("Describe what you'd like me to do next...".to_string()),
            }
        })
        .collect();

    let search = Some(InlineListSearchConfig {
        label: "Search".to_string(),
        placeholder: Some("Filter options...".to_string()),
    });

    handle.show_tabbed_list_modal(title, steps, current_step, search);
    handle.force_redraw();
    task::yield_now().await;

    loop {
        if (*ctrl_c_state).is_cancel_requested() {
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
                let signal = if (*ctrl_c_state).is_exit_requested() {
                    CtrlCSignal::Exit
                } else if (*ctrl_c_state).is_cancel_requested() {
                    CtrlCSignal::Cancel
                } else {
                    (*ctrl_c_state).register_signal()
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
                (*ctrl_c_state).disarm_exit();
                handle.close_modal();
                handle.force_redraw();
                task::yield_now().await;
                tokio::time::sleep(Duration::from_millis(100)).await;

                if let Some(InlineListSelection::AskUserChoice {
                    tab_id, choice_id, ..
                }) = selections.pop()
                {
                    return Ok(json!({
                        "tab_id": tab_id,
                        "choice_id": choice_id
                    }));
                }

                return Ok(json!({"cancelled": true}));
            }
            InlineEvent::WizardModalCancel | InlineEvent::ListModalCancel | InlineEvent::Cancel => {
                (*ctrl_c_state).disarm_exit();
                handle.close_modal();
                handle.force_redraw();
                task::yield_now().await;
                tokio::time::sleep(Duration::from_millis(100)).await;
                return Ok(json!({"cancelled": true}));
            }
            InlineEvent::Exit => {
                (*ctrl_c_state).disarm_exit();
                handle.close_modal();
                handle.force_redraw();
                task::yield_now().await;
                tokio::time::sleep(Duration::from_millis(100)).await;
                return Ok(json!({"cancelled": true, "signal": "exit"}));
            }
            InlineEvent::Submit(_) | InlineEvent::QueueSubmit(_) => {
                continue;
            }
            _ => {}
        }
    }
}

#[allow(dead_code)]
pub fn build_recovery_prompt_from_diagnostics(
    diagnostics: &RecoveryDiagnostics,
    _circuit_diagnostics: &[ToolCircuitDiagnostics],
) -> RecoveryPromptBuilder {
    let mut builder = RecoveryPromptBuilder::new("Circuit Breaker Activated".to_string());

    let summary = format!(
        "Multiple tools are experiencing failures:\n\n\
         **Open Circuits ({})**: {}\n\n\
         **Recent Errors**:\n{}\n\n\
         How would you like to proceed?",
        diagnostics.open_circuits.len(),
        diagnostics.open_circuits.join(", "),
        build_error_summary(diagnostics)
    );

    let mut recommendations = Vec::new();

    if !diagnostics.open_circuits.is_empty() {
        recommendations.push(RecoveryOption {
            id: "retry_all".to_string(),
            title: "Reset All & Retry".to_string(),
            subtitle: "Clear all circuit breakers and try again".to_string(),
            badge: Some("Recommended".to_string()),
            action: RecoveryAction::ResetAllCircuits,
        });
    }

    recommendations.push(RecoveryOption {
        id: "continue".to_string(),
        title: "Continue Anyway".to_string(),
        subtitle: "Ignore circuit breakers and proceed".to_string(),
        badge: None,
        action: RecoveryAction::Continue,
    });

    recommendations.push(RecoveryOption {
        id: "skip".to_string(),
        title: "Skip This Step".to_string(),
        subtitle: "Move on to the next part of the task".to_string(),
        badge: None,
        action: RecoveryAction::SkipStep,
    });

    recommendations.push(RecoveryOption {
        id: "alternative".to_string(),
        title: "Try Different Approach".to_string(),
        subtitle: "Suggest an alternative strategy".to_string(),
        badge: None,
        action: RecoveryAction::TryAlternative,
    });

    recommendations.push(RecoveryOption {
        id: "show_errors".to_string(),
        title: "Show Error Log".to_string(),
        subtitle: "View detailed error history".to_string(),
        badge: None,
        action: RecoveryAction::ShowErrorLog,
    });

    recommendations.push(RecoveryOption {
        id: "diagnostics".to_string(),
        title: "Run Diagnostics".to_string(),
        subtitle: "Check tool health and configuration".to_string(),
        badge: None,
        action: RecoveryAction::RunDiagnostics,
    });

    recommendations.push(RecoveryOption {
        id: "debug_agent".to_string(),
        title: "Call Debug Agent".to_string(),
        subtitle: "Spawn specialist to investigate issues".to_string(),
        badge: None,
        action: RecoveryAction::CallDebugAgent,
    });

    recommendations.push(RecoveryOption {
        id: "save_exit".to_string(),
        title: "Save Progress & Exit".to_string(),
        subtitle: "Write task summary and end session".to_string(),
        badge: None,
        action: RecoveryAction::SaveAndExit,
    });

    builder = builder.with_summary(summary);

    for rec in recommendations {
        builder = builder.add_recommendation(rec);
    }

    builder
}

#[allow(dead_code)]
fn build_error_summary(diagnostics: &RecoveryDiagnostics) -> String {
    if diagnostics.recent_errors.is_empty() {
        return "No recent errors recorded.".to_string();
    }

    let mut summary = String::new();
    for (i, error) in diagnostics.recent_errors.iter().take(5).enumerate() {
        let error_type = match error.error_type {
            ErrorType::ToolExecution => "Execution Error",
            ErrorType::CircuitBreaker => "Circuit Breaker",
            ErrorType::Timeout => "Timeout",
            ErrorType::PermissionDenied => "Permission Denied",
            ErrorType::InvalidArguments => "Invalid Arguments",
            ErrorType::ResourceNotFound => "Not Found",
            ErrorType::Other => "Other",
        };
        summary.push_str(&format!(
            "{}. **{}** ({}) - {}\n",
            i + 1,
            error.tool_name,
            error_type,
            error.error_message
        ));
    }

    if diagnostics.recent_errors.len() > 5 {
        summary.push_str(&format!(
            "... and {} more errors\n",
            diagnostics.recent_errors.len() - 5
        ));
    }

    summary
}

#[allow(dead_code)]
pub fn parse_recovery_response(response: &Value) -> Option<RecoveryAction> {
    let choice_id = response.get("choice_id")?.as_str()?;
    let tab_id = response
        .get("tab_id")
        .and_then(|v| v.as_str())
        .unwrap_or("recovery");

    if tab_id != "recovery" {
        return None;
    }

    match choice_id {
        "retry_all" => Some(RecoveryAction::ResetAllCircuits),
        "continue" => Some(RecoveryAction::Continue),
        "skip" => Some(RecoveryAction::SkipStep),
        "alternative" => Some(RecoveryAction::TryAlternative),
        "show_errors" => Some(RecoveryAction::ShowErrorLog),
        "diagnostics" => Some(RecoveryAction::RunDiagnostics),
        "debug_agent" => Some(RecoveryAction::CallDebugAgent),
        "save_exit" => Some(RecoveryAction::SaveAndExit),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_current_step_uses_default_tab_when_found() {
        let tabs = vec![
            RecoveryTabArgs {
                id: "a".to_string(),
                title: "A".to_string(),
                items: vec![RecoveryItemArgs {
                    id: "1".to_string(),
                    title: "one".to_string(),
                    subtitle: None,
                    badge: None,
                }],
            },
            RecoveryTabArgs {
                id: "b".to_string(),
                title: "B".to_string(),
                items: vec![RecoveryItemArgs {
                    id: "2".to_string(),
                    title: "two".to_string(),
                    subtitle: None,
                    badge: None,
                }],
            },
        ];

        assert_eq!(resolve_current_step(&tabs, Some("b")), 1);
        assert_eq!(resolve_current_step(&tabs, Some("missing")), 0);
        assert_eq!(resolve_current_step(&tabs, None), 0);
    }

    #[test]
    fn find_default_choice_selection_matches_item_id() {
        let items = vec![
            InlineListItem {
                title: "Retry".to_string(),
                subtitle: None,
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::AskUserChoice {
                    tab_id: "recovery".to_string(),
                    choice_id: "retry_all".to_string(),
                    text: None,
                }),
                search_value: None,
            },
            InlineListItem {
                title: "Skip".to_string(),
                subtitle: None,
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::AskUserChoice {
                    tab_id: "recovery".to_string(),
                    choice_id: "skip".to_string(),
                    text: None,
                }),
                search_value: None,
            },
        ];

        let selected = find_default_choice_selection(&items, "skip");
        assert!(matches!(
            selected,
            Some(InlineListSelection::AskUserChoice {
                choice_id,
                ..
            }) if choice_id == "skip"
        ));
        assert!(find_default_choice_selection(&items, "missing").is_none());
    }

    #[test]
    fn builder_sets_default_choice_to_recommended_option() {
        let prompt = RecoveryPromptBuilder::new("Recovery".to_string())
            .with_summary("summary".to_string())
            .add_recommendation(RecoveryOption {
                id: "retry_all".to_string(),
                title: "Reset All & Retry".to_string(),
                subtitle: "Clear all circuit breakers and try again".to_string(),
                badge: Some("Recommended".to_string()),
                action: RecoveryAction::ResetAllCircuits,
            })
            .add_recommendation(RecoveryOption {
                id: "continue".to_string(),
                title: "Continue Anyway".to_string(),
                subtitle: "Ignore and proceed".to_string(),
                badge: None,
                action: RecoveryAction::Continue,
            })
            .build();

        assert_eq!(prompt["default_tab_id"], "recovery");
        assert_eq!(prompt["default_choice_id"], "retry_all");
    }
}
