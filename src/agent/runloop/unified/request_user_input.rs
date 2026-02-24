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
    #[serde(default)]
    focus_area: Option<String>,
    #[serde(default)]
    analysis_hints: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
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
            let items = build_question_items(q);

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

fn build_question_items(question: &RequestUserInputQuestion) -> Vec<InlineListItem> {
    let options = question
        .options
        .clone()
        .or_else(|| generate_suggested_options(question));

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

        // Keep free-form input explicit when choices are present.
        items.push(InlineListItem {
            title: format!("{}. Other (type custom response)", options.len() + 1),
            subtitle: Some("Use notes to provide your own response".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: question.id.clone(),
                selected: vec![],
                other: Some(String::new()),
            }),
            search_value: Some("other custom response free text".to_string()),
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

fn generate_suggested_options(
    question: &RequestUserInputQuestion,
) -> Option<Vec<RequestUserInputOption>> {
    if question.options.is_some() {
        return None;
    }

    let mut context = format!("{} {} {}", question.id, question.header, question.question);
    if let Some(focus_area) = question.focus_area.as_ref() {
        context.push(' ');
        context.push_str(focus_area);
    }
    if !question.analysis_hints.is_empty() {
        context.push(' ');
        context.push_str(&question.analysis_hints.join(" "));
    }
    let context = context.to_lowercase();

    let mut options = Vec::new();

    if contains_any(
        &context,
        &[
            "system prompt",
            "prompt",
            "harness",
            "plan mode",
            "agent",
            "planning",
        ],
    ) {
        if contains_any(
            &context,
            &[
                "timeout", "stream", "fallback", "provider", "retry", "latency",
            ],
        ) {
            push_unique_option(
                &mut options,
                "Provider fallback hardening",
                "Prioritize timeout recovery and stream-to-non-stream fallback behavior first.",
            );
        }

        if contains_any(
            &context,
            &["loop", "stuck", "navigation", "repeat", "stall", "retry"],
        ) {
            push_unique_option(
                &mut options,
                "Loop prevention and recovery",
                "Improve loop detection and force synthesis-or-act transitions before repeated calls.",
            );
        }

        if contains_any(
            &context,
            &[
                "question",
                "modal",
                "guided",
                "choice",
                "free text",
                "freeform",
                "input",
            ],
        ) {
            push_unique_option(
                &mut options,
                "Guided question UX",
                "Show suggested options in Questions modal while preserving custom free-text input.",
            );
        }

        if contains_any(
            &context,
            &[
                "token",
                "context",
                "verbose",
                "length",
                "compact",
                "efficiency",
            ],
        ) {
            push_unique_option(
                &mut options,
                "Prompt token efficiency",
                "Reduce duplicated instructions and tighten wording to improve reliability per token.",
            );
        }

        if contains_any(
            &context,
            &["redundan", "overlap", "duplicate", "repetitive", "verbose"],
        ) {
            push_unique_option(
                &mut options,
                "Prompt redundancy reduction",
                "Remove duplicated guidance across variants to increase instruction signal quality.",
            );
        }

        if contains_any(
            &context,
            &[
                "missing",
                "failure",
                "patch",
                "circular",
                "dependency",
                "recovery",
                "error pattern",
            ],
        ) {
            push_unique_option(
                &mut options,
                "Failure pattern coverage",
                "Add concrete recovery guidance for known failure modes and repeated error patterns.",
            );
        }

        if contains_any(
            &context,
            &[
                "harness",
                "docs",
                "doc refs",
                "invariant",
                "tech debt",
                "tracker",
            ],
        ) {
            push_unique_option(
                &mut options,
                "Harness integration strengthening",
                "Add explicit references to harness docs, invariants, and debt tracking touchpoints.",
            );
        }

        if contains_any(
            &context,
            &[
                "minimal",
                "lightweight",
                "resource-constrained",
                "compact mode",
            ],
        ) {
            push_unique_option(
                &mut options,
                "Minimal/Lightweight optimization",
                "Tighten minimal/lightweight modes for clarity while preserving required safeguards.",
            );
        }

        if options.is_empty() {
            push_unique_option(
                &mut options,
                "Loop prevention and recovery",
                "Tighten anti-loop prompts and transition rules to avoid repeated navigation cycles.",
            );
            push_unique_option(
                &mut options,
                "Prompt token efficiency",
                "Trim redundant guidance and prioritize high-signal instructions.",
            );
            push_unique_option(
                &mut options,
                "Guided question UX",
                "Provide suggested plan options with a clear custom-response fallback.",
            );
        }
    } else if contains_any(
        &context,
        &[
            "improve",
            "improvement",
            "optimize",
            "fix",
            "priority",
            "focus",
        ],
    ) {
        push_unique_option(
            &mut options,
            "Fix highest-risk issue",
            "Address the riskiest blocker first so follow-up work has lower failure risk.",
        );
        push_unique_option(
            &mut options,
            "Balance impact and effort",
            "Choose a medium-scope improvement that ships quickly with clear validation.",
        );
        push_unique_option(
            &mut options,
            "Deep quality pass",
            "Prioritize thoroughness, including stronger tests and operational guardrails.",
        );
    } else if contains_any(
        &context,
        &[
            "goal",
            "outcome",
            "constraints",
            "non-goals",
            "step",
            "composable",
            "verification",
            "manual check",
        ],
    ) {
        push_unique_option(
            &mut options,
            "Minimal implementation slice",
            "Choose the smallest end-to-end slice with clear user-visible impact first.",
        );
        push_unique_option(
            &mut options,
            "Balanced phased plan",
            "Split work into medium-size phases balancing delivery speed and risk control.",
        );
        push_unique_option(
            &mut options,
            "Thorough validation-first plan",
            "Emphasize stronger verification gates before and after each implementation step.",
        );
    }

    if options.is_empty() {
        return None;
    }

    options.truncate(3);
    if let Some(first) = options
        .first_mut()
        .filter(|first| !first.label.contains("(Recommended)"))
    {
        first.label.push_str(" (Recommended)");
    }

    Some(options)
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn push_unique_option(options: &mut Vec<RequestUserInputOption>, label: &str, description: &str) {
    if options.iter().any(|existing| existing.label == label) {
        return;
    }

    options.push(RequestUserInputOption {
        label: label.to_string(),
        description: description.to_string(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt_question_with_hints() -> RequestUserInputQuestion {
        RequestUserInputQuestion {
            id: "system_prompt_plan".to_string(),
            header: "Direction".to_string(),
            question: "Which area should we prioritize to improve plan mode behavior?".to_string(),
            options: None,
            focus_area: Some("system prompt".to_string()),
            analysis_hints: vec!["navigation loop".to_string(), "stream timeout".to_string()],
        }
    }

    #[test]
    fn generates_prompt_specific_suggestions() {
        let question = prompt_question_with_hints();
        let options = generate_suggested_options(&question).expect("expected generated options");

        assert!((1..=3).contains(&options.len()));
        assert!(options[0].label.contains("(Recommended)"));
        assert!(
            options
                .iter()
                .any(|option| option.label.contains("fallback") || option.label.contains("Loop"))
        );
    }

    #[test]
    fn generates_weakness_aware_prompt_options() {
        let question = RequestUserInputQuestion {
            id: "prompt_improvement".to_string(),
            header: "Direction".to_string(),
            question: "Which system prompt improvement should we prioritize?".to_string(),
            options: None,
            focus_area: Some("system_prompt".to_string()),
            analysis_hints: vec![
                "Redundancy exists between prompt variants".to_string(),
                "Missing explicit guidance for failure patterns".to_string(),
            ],
        };

        let options = generate_suggested_options(&question).expect("expected generated options");
        assert!((1..=3).contains(&options.len()));
        assert!(options.iter().any(|opt| {
            opt.label.contains("redundancy")
                || opt.label.contains("Failure pattern")
                || opt.label.contains("Prompt")
        }));
    }

    #[test]
    fn generates_planning_options_for_goal_constraints_questions() {
        let question = RequestUserInputQuestion {
            id: "constraints".to_string(),
            header: "Plan".to_string(),
            question: "Break the work into 3-7 composable steps. For each step include target file(s) and a concrete expected outcome.".to_string(),
            options: None,
            focus_area: None,
            analysis_hints: Vec::new(),
        };

        let options = generate_suggested_options(&question).expect("expected planning options");
        assert!((1..=3).contains(&options.len()));
        assert!(options[0].label.contains("(Recommended)"));
    }

    #[test]
    fn option_questions_add_explicit_other_choice() {
        let question = RequestUserInputQuestion {
            id: "scope".to_string(),
            header: "Scope".to_string(),
            question: "Pick direction".to_string(),
            options: Some(vec![
                RequestUserInputOption {
                    label: "Option A".to_string(),
                    description: "A".to_string(),
                },
                RequestUserInputOption {
                    label: "Option B".to_string(),
                    description: "B".to_string(),
                },
            ]),
            focus_area: None,
            analysis_hints: Vec::new(),
        };

        let items = build_question_items(&question);
        assert_eq!(items.len(), 3);
        assert!(items[2].title.contains("Other"));

        let selection = items[2]
            .selection
            .clone()
            .expect("expected selection for other choice");
        match selection {
            InlineListSelection::RequestUserInputAnswer {
                selected, other, ..
            } => {
                assert!(selected.is_empty());
                assert_eq!(other, Some(String::new()));
            }
            _ => panic!("expected request_user_input selection"),
        }
    }

    #[test]
    fn keeps_freeform_only_when_no_suggestions_apply() {
        let question = RequestUserInputQuestion {
            id: "env".to_string(),
            header: "Env".to_string(),
            question: "What environment are you using?".to_string(),
            options: None,
            focus_area: None,
            analysis_hints: Vec::new(),
        };

        let items = build_question_items(&question);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Enter your response...");
    }
}
