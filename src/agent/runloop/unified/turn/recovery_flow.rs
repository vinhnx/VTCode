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

        let tabs = vec![RecoveryTabInternal {
            id: "recovery".to_string(),
            title: "Recovery Options".to_string(),
            items: self
                .recommendations
                .into_iter()
                .map(|r| RecoveryItemInternal {
                    id: r.id,
                    title: r.title,
                    subtitle: Some(r.subtitle),
                    badge: r.badge,
                })
                .collect(),
        }];

        json!({
            "title": self.title,
            "question": self.summary,
            "tabs": tabs,
            "allow_freeform": true,
            "freeform_label": "Provide custom guidance",
            "freeform_placeholder": "Describe what you'd like me to do next..."
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
    #[derive(Debug, Deserialize)]
    struct RecoveryPromptArgs {
        #[serde(default)]
        title: Option<String>,
        question: String,
        tabs: Vec<RecoveryTabArgs>,
        #[serde(default)]
        default_tab_id: Option<String>,
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

    let parsed: RecoveryPromptArgs =
        serde_json::from_value(args.clone()).context("Invalid recovery prompt arguments")?;

    if parsed.tabs.is_empty() {
        return Ok(json!({ "cancelled": true, "error": "No tabs provided" }));
    }

    let title = parsed
        .title
        .unwrap_or_else(|| "Circuit Breaker Activated".to_string());

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
