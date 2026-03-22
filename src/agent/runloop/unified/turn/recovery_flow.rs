use std::sync::Arc;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Notify;

use vtcode_core::core::agent::error_recovery::{ErrorType, RecoveryDiagnostics};
use vtcode_tui::app::{
    InlineHandle, InlineListItem, InlineListSearchConfig, InlineListSelection, InlineSession,
    ListOverlayRequest, TransientRequest, TransientSubmission,
};

use crate::agent::runloop::unified::overlay_prompt::{OverlayWaitOutcome, show_overlay_and_wait};
use crate::agent::runloop::unified::state::CtrlCState;

pub(crate) struct RecoveryPromptBuilder {
    pub(crate) title: String,
    pub(crate) summary: String,
    pub(crate) recommendations: Vec<RecoveryOption>,
}

#[derive(Debug, Clone)]
pub(crate) struct RecoveryOption {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) badge: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) enum RecoveryAction {
    ResetAllCircuits,
    SkipStep,
    SaveAndExit,
    Continue,
}

const RECOVERY_TAB_ID: &str = "recovery";
const CHOICE_RETRY_ALL: &str = "retry_all";
const CHOICE_CONTINUE: &str = "continue";
const CHOICE_SKIP: &str = "skip";
const CHOICE_SAVE_EXIT: &str = "save_exit";

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

fn split_question_lines(question: &str) -> Vec<String> {
    let mut lines = question
        .lines()
        .map(|line| line.trim_end().to_string())
        .collect::<Vec<_>>();

    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }

    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

impl RecoveryPromptBuilder {
    pub(crate) fn new(title: String) -> Self {
        Self {
            title,
            summary: String::new(),
            recommendations: Vec::new(),
        }
    }

    pub(crate) fn with_summary(mut self, summary: String) -> Self {
        self.summary = summary;
        self
    }

    pub(crate) fn add_recommendation(mut self, option: RecoveryOption) -> Self {
        self.recommendations.push(option);
        self
    }

    pub(crate) fn build(self) -> Value {
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
            id: RECOVERY_TAB_ID.to_string(),
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
            "default_tab_id": RECOVERY_TAB_ID,
            "default_choice_id": default_choice_id
        })
    }
}

pub(crate) async fn execute_recovery_prompt(
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
    let Some(tab) = parsed.tabs.get(current_step) else {
        return Ok(json!({ "cancelled": true, "error": "Invalid tab index" }));
    };

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

    let selected = parsed
        .default_choice_id
        .as_deref()
        .and_then(|choice_id| find_default_choice_selection(&items, choice_id))
        .or_else(|| items.first().and_then(|item| item.selection.clone()));

    let search = Some(InlineListSearchConfig {
        label: "Search options".to_string(),
        placeholder: Some("title, subtitle, or badge".to_string()),
    });

    let outcome = show_overlay_and_wait(
        handle,
        session,
        TransientRequest::List(ListOverlayRequest {
            title,
            lines: split_question_lines(&parsed.question),
            footer_hint: None,
            items,
            selected,
            search,
            hotkeys: Vec::new(),
        }),
        ctrl_c_state,
        ctrl_c_notify,
        |submission| match submission {
            TransientSubmission::Selection(InlineListSelection::AskUserChoice {
                tab_id,
                choice_id,
                ..
            }) => Some((tab_id, choice_id)),
            _ => None,
        },
    )
    .await?;

    Ok(match outcome {
        OverlayWaitOutcome::Submitted((tab_id, choice_id)) => json!({
            "tab_id": tab_id,
            "choice_id": choice_id
        }),
        OverlayWaitOutcome::Cancelled => json!({"cancelled": true}),
        OverlayWaitOutcome::Interrupted => json!({"cancelled": true, "signal": "cancel"}),
        OverlayWaitOutcome::Exit => json!({"cancelled": true, "signal": "exit"}),
    })
}

pub(crate) fn build_recovery_prompt_from_diagnostics(
    diagnostics: &RecoveryDiagnostics,
) -> RecoveryPromptBuilder {
    let summary = format!(
        "Multiple tools are experiencing failures:\n\n\
         Open Circuits ({}): {}\n\n\
         Recent Errors:\n{}\n\n\
         How would you like to proceed?",
        diagnostics.open_circuits.len(),
        diagnostics.open_circuits.join(", "),
        build_error_summary(diagnostics)
    );

    let mut builder =
        RecoveryPromptBuilder::new("Circuit Breaker Activated".to_string()).with_summary(summary);

    if !diagnostics.open_circuits.is_empty() {
        builder = builder.add_recommendation(RecoveryOption {
            id: CHOICE_RETRY_ALL.to_string(),
            title: "Reset All & Retry".to_string(),
            subtitle: "Clear all circuit breakers and try again".to_string(),
            badge: Some("Recommended".to_string()),
        });
    }

    for (id, title, subtitle) in [
        (
            CHOICE_CONTINUE,
            "Continue Anyway",
            "Ignore circuit breakers and proceed",
        ),
        (
            CHOICE_SKIP,
            "Skip This Step",
            "Move on to the next part of the task",
        ),
        (
            CHOICE_SAVE_EXIT,
            "Save Progress & Exit",
            "Write task summary and end session",
        ),
    ] {
        builder = builder.add_recommendation(RecoveryOption {
            id: id.to_string(),
            title: title.to_string(),
            subtitle: subtitle.to_string(),
            badge: None,
        });
    }

    builder
}

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
            ErrorType::ApiCall => "API Error",
            ErrorType::FileSystem => "File System Error",
            ErrorType::Network => "Network Error",
            ErrorType::Validation => "Validation Error",
            ErrorType::Other => "Other",
        };
        summary.push_str(&format!(
            "{}. {} ({}) - {}\n",
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

fn action_from_choice_id(choice_id: &str) -> Option<RecoveryAction> {
    match choice_id {
        CHOICE_RETRY_ALL => Some(RecoveryAction::ResetAllCircuits),
        CHOICE_CONTINUE => Some(RecoveryAction::Continue),
        CHOICE_SKIP => Some(RecoveryAction::SkipStep),
        CHOICE_SAVE_EXIT => Some(RecoveryAction::SaveAndExit),
        _ => None,
    }
}

pub(crate) fn parse_recovery_response(response: &Value) -> Option<RecoveryAction> {
    let choice_id = response.get("choice_id")?.as_str()?;
    let tab_id = response
        .get("tab_id")
        .and_then(|v| v.as_str())
        .unwrap_or(RECOVERY_TAB_ID);

    if tab_id != RECOVERY_TAB_ID {
        return None;
    }

    action_from_choice_id(choice_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_current_step_uses_default_tab_when_found() {
        let tabs = vec![
            RecoveryTabArgs {
                id: "a".to_string(),
                items: vec![RecoveryItemArgs {
                    id: "1".to_string(),
                    title: "one".to_string(),
                    subtitle: None,
                    badge: None,
                }],
            },
            RecoveryTabArgs {
                id: "b".to_string(),
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
            })
            .add_recommendation(RecoveryOption {
                id: "continue".to_string(),
                title: "Continue Anyway".to_string(),
                subtitle: "Ignore and proceed".to_string(),
                badge: None,
            })
            .build();

        assert_eq!(prompt["default_tab_id"], "recovery");
        assert_eq!(prompt["default_choice_id"], "retry_all");
    }

    #[test]
    fn parse_recovery_response_rejects_unimplemented_choices() {
        let legacy_choice = json!({
            "tab_id": "recovery",
            "choice_id": "alternative",
        });
        assert!(parse_recovery_response(&legacy_choice).is_none());
    }
}
