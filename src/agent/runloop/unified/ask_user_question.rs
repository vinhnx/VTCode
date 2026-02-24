use std::sync::Arc;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{sync::Notify, task};

use vtcode_core::ui::tui::{
    InlineHandle, InlineListItem, InlineListSearchConfig, InlineListSelection, InlineSession,
    WizardStep,
};

use super::state::CtrlCState;
use super::wizard_modal::{WizardModalOutcome, wait_for_wizard_modal};

#[derive(Debug, Deserialize)]
struct AskUserQuestionArgs {
    #[serde(default)]
    title: Option<String>,
    question: String,
    tabs: Vec<AskUserTab>,

    #[serde(default)]
    allow_freeform: bool,
    #[serde(default)]
    freeform_label: Option<String>,
    #[serde(default)]
    freeform_placeholder: Option<String>,

    #[serde(default)]
    default_tab_id: Option<String>,
    #[serde(default)]
    default_choice_id: Option<String>,
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

fn resolve_current_step(tabs: &[AskUserTab], default_tab_id: Option<&str>) -> usize {
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
    if parsed.tabs.iter().any(|tab| tab.items.is_empty()) {
        return Ok(json!({
            "cancelled": true,
            "error": "Each tab must include at least one item"
        }));
    }

    let title = parsed
        .title
        .clone()
        .unwrap_or_else(|| "Question".to_string());
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
                allow_freeform: parsed.allow_freeform,
                freeform_label: parsed.freeform_label.clone(),
                freeform_placeholder: parsed.freeform_placeholder.clone(),
            }
        })
        .collect();

    // Enable search by default for better UX on larger lists.
    let search = Some(InlineListSearchConfig {
        label: "Search".to_string(),
        placeholder: Some("Type to filter…".to_string()),
    });

    handle.show_tabbed_list_modal(title, steps, current_step, search);
    handle.force_redraw();
    task::yield_now().await;
    match wait_for_wizard_modal(handle, session, ctrl_c_state, ctrl_c_notify).await? {
        WizardModalOutcome::Submitted(mut selections) => {
            if let Some(InlineListSelection::AskUserChoice {
                tab_id,
                choice_id,
                text,
            }) = selections.pop()
            {
                return Ok(json!({
                    "tab_id": tab_id,
                    "choice_id": choice_id,
                    "text": text
                }));
            }

            Ok(json!({"cancelled": true}))
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
mod tests {
    use super::*;

    #[test]
    fn resolve_current_step_uses_default_tab_when_found() {
        let tabs = vec![
            AskUserTab {
                id: "a".to_string(),
                title: "A".to_string(),
                items: vec![AskUserItem {
                    id: "1".to_string(),
                    title: "one".to_string(),
                    subtitle: None,
                    badge: None,
                }],
            },
            AskUserTab {
                id: "b".to_string(),
                title: "B".to_string(),
                items: vec![AskUserItem {
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
                title: "Alpha".to_string(),
                subtitle: None,
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::AskUserChoice {
                    tab_id: "tab".to_string(),
                    choice_id: "alpha".to_string(),
                    text: None,
                }),
                search_value: None,
            },
            InlineListItem {
                title: "Beta".to_string(),
                subtitle: None,
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::AskUserChoice {
                    tab_id: "tab".to_string(),
                    choice_id: "beta".to_string(),
                    text: None,
                }),
                search_value: None,
            },
        ];

        let selected = find_default_choice_selection(&items, "beta");
        assert!(matches!(
            selected,
            Some(InlineListSelection::AskUserChoice {
                choice_id,
                ..
            }) if choice_id == "beta"
        ));
        assert!(find_default_choice_selection(&items, "missing").is_none());
    }
}
