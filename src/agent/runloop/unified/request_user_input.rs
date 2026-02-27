use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Notify;
use tokio::task;

use vtcode_tui::{
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

struct NormalizedRequestUserInput {
    args: RequestUserInputArgs,
    wizard_mode: WizardModalMode,
    current_step: usize,
    title_override: Option<String>,
    allow_freeform: bool,
    freeform_label: Option<String>,
    freeform_placeholder: Option<String>,
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

    // Build wizard steps from questions
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

    let search = None;

    let safe_current_step = current_step.min(steps.len().saturating_sub(1));
    handle.show_wizard_modal_with_mode(title, steps, safe_current_step, search, wizard_mode);
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

            let summary_style = std::sync::Arc::new(InlineTextStyle::default());
            let summary_segment = |text: String| InlineSegment {
                text,
                style: summary_style.clone(),
            };

            if wizard_mode == WizardModalMode::TabbedList {
                handle.append_line(
                    InlineMessageKind::Info,
                    vec![summary_segment("• Selection captured".to_string())],
                );
            } else {
                let answered_count = answers.len();
                let total_count = parsed.questions.len();
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

fn normalize_request_user_input_args(args: &Value) -> Result<NormalizedRequestUserInput> {
    let parsed: RequestUserInputArgs = serde_json::from_value(args.clone())?;
    Ok(NormalizedRequestUserInput {
        args: parsed,
        wizard_mode: WizardModalMode::MultiStep,
        current_step: 0,
        title_override: None,
        allow_freeform: true,
        freeform_label: None,
        freeform_placeholder: None,
    })
}

#[cfg(test)]
fn build_question_items(question: &RequestUserInputQuestion) -> Vec<InlineListItem> {
    let options = question
        .options
        .clone()
        .or_else(|| generate_suggested_options(question));
    build_question_items_with_options(question, options)
}

fn build_question_items_with_options(
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

fn resolve_question_options(
    questions: &[RequestUserInputQuestion],
) -> Vec<Option<Vec<RequestUserInputOption>>> {
    let mut provided_signature_counts: HashMap<String, usize> = HashMap::new();
    for question in questions {
        if let Some(options) = question.options.as_ref() {
            let signature = options_signature(options);
            if !signature.is_empty() {
                *provided_signature_counts.entry(signature).or_insert(0) += 1;
            }
        }
    }

    questions
        .iter()
        .map(|question| match question.options.clone() {
            Some(provided_options) => {
                let signature = options_signature(&provided_options);
                let repeated_signature = provided_signature_counts
                    .get(&signature)
                    .copied()
                    .unwrap_or(0)
                    > 1;
                if should_regenerate_provided_options(
                    question,
                    &provided_options,
                    repeated_signature,
                ) {
                    generate_suggested_options(question).or(Some(provided_options))
                } else {
                    Some(provided_options)
                }
            }
            None => generate_suggested_options(question),
        })
        .collect()
}

fn should_regenerate_provided_options(
    question: &RequestUserInputQuestion,
    options: &[RequestUserInputOption],
    repeated_signature: bool,
) -> bool {
    if options.is_empty() || options.len() > 3 {
        return true;
    }

    if repeated_signature {
        return true;
    }

    if options
        .iter()
        .any(|option| option.label.trim().is_empty() || option.description.trim().is_empty())
    {
        return true;
    }

    let unique_labels = options
        .iter()
        .map(|option| normalize_option_text(&option.label))
        .collect::<HashSet<_>>();
    if unique_labels.len() != options.len() {
        return true;
    }

    let question_text = question.question.to_lowercase();
    let generic_option_count = options
        .iter()
        .filter(|option| is_generic_planning_option_label(&option.label))
        .count();
    if generic_option_count == options.len()
        && contains_any(
            &question_text,
            &[
                "user-visible outcome",
                "user visible outcome",
                "break the work",
                "composable steps",
                "exact command",
                "manual check",
                "prove it is complete",
                "proves it is complete",
            ],
        )
    {
        return true;
    }

    false
}

fn options_signature(options: &[RequestUserInputOption]) -> String {
    let mut entries = options
        .iter()
        .map(|option| {
            format!(
                "{}::{}",
                normalize_option_text(&option.label),
                normalize_option_text(&option.description)
            )
        })
        .collect::<Vec<_>>();

    entries.sort_unstable();
    entries.join("||")
}

fn normalize_option_text(text: &str) -> String {
    let lowered = text.to_lowercase().replace("(recommended)", "");
    lowered
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn is_generic_planning_option_label(label: &str) -> bool {
    let normalized = normalize_option_text(label);
    contains_any(
        &normalized,
        &[
            "minimal implementation slice",
            "minimal implementation",
            "implementation slice",
            "balanced implementation",
            "comprehensive implementation",
            "quick win",
            "deep dive",
            "thorough implementation",
        ],
    )
}

fn ensure_recommended_first(
    mut options: Vec<RequestUserInputOption>,
) -> Vec<RequestUserInputOption> {
    if options.is_empty() {
        return options;
    }

    for option in &mut options {
        option.label = option
            .label
            .replace("(Recommended)", "")
            .replace("(recommended)", "")
            .trim()
            .to_string();
    }

    if !options[0].label.contains("(Recommended)") {
        options[0].label.push_str(" (Recommended)");
    }

    options
}

fn generate_suggested_options(
    question: &RequestUserInputQuestion,
) -> Option<Vec<RequestUserInputOption>> {
    let question_context = question.question.to_lowercase();
    let metadata_context = format!("{} {}", question.id, question.header).to_lowercase();
    let local_context = format!("{} {}", question_context, metadata_context);
    let mut global_context = String::new();
    if let Some(focus_area) = question.focus_area.as_ref() {
        global_context.push(' ');
        global_context.push_str(focus_area);
    }
    if !question.analysis_hints.is_empty() {
        global_context.push(' ');
        global_context.push_str(&question.analysis_hints.join(" "));
    }
    let global_context = global_context.to_lowercase();

    let intent = classify_question_intent(&question_context, &metadata_context);
    let mut options = match intent {
        QuestionIntent::OutcomeAndConstraints => outcome_and_constraint_options(),
        QuestionIntent::StepDecomposition => step_decomposition_options(),
        QuestionIntent::VerificationEvidence => verification_evidence_options(),
        QuestionIntent::PrioritySelection => {
            priority_selection_options(&local_context, &global_context)
        }
        QuestionIntent::GenericImprovement => generic_improvement_options(),
        QuestionIntent::GenericPlanning => Vec::new(),
    };

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuestionIntent {
    OutcomeAndConstraints,
    StepDecomposition,
    VerificationEvidence,
    PrioritySelection,
    GenericImprovement,
    GenericPlanning,
}

fn classify_question_intent(question_context: &str, metadata_context: &str) -> QuestionIntent {
    detect_question_intent(question_context)
        .or_else(|| detect_question_intent(metadata_context))
        .unwrap_or(QuestionIntent::GenericPlanning)
}

fn detect_question_intent(context: &str) -> Option<QuestionIntent> {
    if contains_any(
        context,
        &[
            "user-visible outcome",
            "user visible outcome",
            "success criteria",
            "constraints",
            "non-goals",
            "non goals",
        ],
    ) {
        return Some(QuestionIntent::OutcomeAndConstraints);
    }

    if contains_any(
        context,
        &[
            "break the work",
            "composable steps",
            "composable step",
            "3-7",
            "target file",
            "expected outcome",
            "decompose",
            "implementation steps",
        ],
    ) {
        return Some(QuestionIntent::StepDecomposition);
    }

    if contains_any(
        context,
        &[
            "exact command",
            "manual check",
            "prove it is complete",
            "proves it is complete",
            "verification",
            "acceptance check",
            "completion check",
        ],
    ) {
        return Some(QuestionIntent::VerificationEvidence);
    }

    if contains_any(
        context,
        &[
            "prioritize first",
            "should we prioritize",
            "which area should",
            "which improvement",
            "focus area",
            "pick direction",
        ],
    ) {
        return Some(QuestionIntent::PrioritySelection);
    }

    if contains_any(
        context,
        &[
            "improve",
            "improvement",
            "optimize",
            "fix",
            "priority",
            "focus",
        ],
    ) {
        return Some(QuestionIntent::GenericImprovement);
    }

    None
}

fn outcome_and_constraint_options() -> Vec<RequestUserInputOption> {
    vec![
        RequestUserInputOption {
            label: "Define outcome metric".to_string(),
            description: "Set one clear user-visible success metric and keep scope aligned to that outcome.".to_string(),
        },
        RequestUserInputOption {
            label: "Lock constraints/non-goals".to_string(),
            description: "Explicitly capture boundaries to avoid accidental scope expansion during implementation.".to_string(),
        },
        RequestUserInputOption {
            label: "Scope MVP boundary".to_string(),
            description: "Choose the smallest deliverable that demonstrates the intended user impact.".to_string(),
        },
    ]
}

fn step_decomposition_options() -> Vec<RequestUserInputOption> {
    vec![
        RequestUserInputOption {
            label: "Dependency-first slices".to_string(),
            description: "Break work by dependencies so each slice can be implemented and verified independently.".to_string(),
        },
        RequestUserInputOption {
            label: "User-flow slices".to_string(),
            description: "Split steps along the user journey so each slice improves one visible interaction path.".to_string(),
        },
        RequestUserInputOption {
            label: "Risk-isolated slices".to_string(),
            description: "Isolate high-risk changes into separate steps to simplify rollback and debugging.".to_string(),
        },
    ]
}

fn verification_evidence_options() -> Vec<RequestUserInputOption> {
    vec![
        RequestUserInputOption {
            label: "Command-based proof".to_string(),
            description: "Require explicit check/test commands for each step to prove completion objectively.".to_string(),
        },
        RequestUserInputOption {
            label: "Behavioral/manual proof".to_string(),
            description: "Use concrete manual checks tied to user-visible behavior when automation is limited.".to_string(),
        },
        RequestUserInputOption {
            label: "Hybrid proof strategy".to_string(),
            description: "Combine automated checks with targeted manual verification for stronger confidence.".to_string(),
        },
    ]
}

fn generic_improvement_options() -> Vec<RequestUserInputOption> {
    vec![
        RequestUserInputOption {
            label: "Fix highest-risk issue".to_string(),
            description:
                "Address the riskiest blocker first so follow-up work has lower failure risk."
                    .to_string(),
        },
        RequestUserInputOption {
            label: "Balance impact and effort".to_string(),
            description:
                "Choose a medium-scope improvement that ships quickly with clear validation."
                    .to_string(),
        },
        RequestUserInputOption {
            label: "Deep quality pass".to_string(),
            description:
                "Prioritize thoroughness, including stronger tests and operational guardrails."
                    .to_string(),
        },
    ]
}

fn priority_selection_options(
    local_context: &str,
    global_context: &str,
) -> Vec<RequestUserInputOption> {
    // Local question intent ranks first; global hints act as tie-breakers.
    let mut options = Vec::new();
    append_domain_priority_options(&mut options, local_context);
    append_domain_priority_options(&mut options, global_context);

    if options.is_empty() {
        options.extend(generic_improvement_options());
    }
    options
}

fn append_domain_priority_options(options: &mut Vec<RequestUserInputOption>, context: &str) {
    if context.trim().is_empty() {
        return;
    }

    if contains_any(
        context,
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
            context,
            &[
                "timeout", "stream", "fallback", "provider", "retry", "latency",
            ],
        ) {
            push_unique_option(
                options,
                "Provider fallback hardening",
                "Prioritize timeout recovery and stream-to-non-stream fallback behavior first.",
            );
        }

        if contains_any(
            context,
            &["loop", "stuck", "navigation", "repeat", "stall", "retry"],
        ) {
            push_unique_option(
                options,
                "Loop prevention and recovery",
                "Improve loop detection and force synthesis-or-act transitions before repeated calls.",
            );
        }

        if contains_any(
            context,
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
                options,
                "Guided question UX",
                "Show suggested options in Questions modal while preserving custom free-text input.",
            );
        }

        if contains_any(
            context,
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
                options,
                "Prompt token efficiency",
                "Reduce duplicated instructions and tighten wording to improve reliability per token.",
            );
        }

        if contains_any(
            context,
            &["redundan", "overlap", "duplicate", "repetitive", "verbose"],
        ) {
            push_unique_option(
                options,
                "Prompt redundancy reduction",
                "Remove duplicated guidance across variants to increase instruction signal quality.",
            );
        }

        if contains_any(
            context,
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
                options,
                "Failure pattern coverage",
                "Add concrete recovery guidance for known failure modes and repeated error patterns.",
            );
        }

        if contains_any(
            context,
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
                options,
                "Harness integration strengthening",
                "Add explicit references to harness docs, invariants, and debt tracking touchpoints.",
            );
        }

        if contains_any(
            context,
            &[
                "minimal",
                "lightweight",
                "resource-constrained",
                "compact mode",
            ],
        ) {
            push_unique_option(
                options,
                "Minimal/Lightweight optimization",
                "Tighten minimal/lightweight modes for clarity while preserving required safeguards.",
            );
        }

        if options.is_empty() {
            push_unique_option(
                options,
                "Loop prevention and recovery",
                "Tighten anti-loop prompts and transition rules to avoid repeated navigation cycles.",
            );
            push_unique_option(
                options,
                "Prompt token efficiency",
                "Trim redundant guidance and prioritize high-signal instructions.",
            );
            push_unique_option(
                options,
                "Guided question UX",
                "Provide suggested plan options with a clear custom-response fallback.",
            );
        }
    }
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
    use serde_json::json;

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
    fn generates_distinct_options_for_outcome_steps_and_verification_questions() {
        let outcome_question = RequestUserInputQuestion {
            id: "q1".to_string(),
            header: "Q1".to_string(),
            question: "What user-visible outcome should this change deliver, and what constraints or non-goals must be respected?".to_string(),
            options: None,
            focus_area: Some("system_prompt".to_string()),
            analysis_hints: vec![
                "Redundancy exists between prompt variants".to_string(),
                "Missing explicit guidance for failure patterns".to_string(),
            ],
        };
        let steps_question = RequestUserInputQuestion {
            id: "q2".to_string(),
            header: "Q2".to_string(),
            question: "Break the work into 3-7 composable steps. For each step include target file(s) and a concrete expected outcome.".to_string(),
            options: None,
            focus_area: Some("system_prompt".to_string()),
            analysis_hints: vec![
                "Redundancy exists between prompt variants".to_string(),
                "Missing explicit guidance for failure patterns".to_string(),
            ],
        };
        let verification_question = RequestUserInputQuestion {
            id: "q3".to_string(),
            header: "Q3".to_string(),
            question: "For each step, what exact command or manual check proves it is complete?"
                .to_string(),
            options: None,
            focus_area: Some("system_prompt".to_string()),
            analysis_hints: vec![
                "Redundancy exists between prompt variants".to_string(),
                "Missing explicit guidance for failure patterns".to_string(),
            ],
        };

        let outcome = generate_suggested_options(&outcome_question).expect("outcome options");
        let steps = generate_suggested_options(&steps_question).expect("step options");
        let verification =
            generate_suggested_options(&verification_question).expect("verification options");

        let outcome_labels = outcome
            .iter()
            .map(|opt| opt.label.clone())
            .collect::<Vec<_>>();
        let step_labels = steps
            .iter()
            .map(|opt| opt.label.clone())
            .collect::<Vec<_>>();
        let verification_labels = verification
            .iter()
            .map(|opt| opt.label.clone())
            .collect::<Vec<_>>();

        assert_ne!(
            outcome_labels, step_labels,
            "outcome and decomposition questions should not reuse identical options"
        );
        assert_ne!(
            step_labels, verification_labels,
            "decomposition and verification questions should not reuse identical options"
        );
        assert_ne!(
            outcome_labels, verification_labels,
            "outcome and verification questions should not reuse identical options"
        );

        assert!(outcome[0].label.contains("Recommended"));
        assert!(steps[0].label.contains("Recommended"));
        assert!(verification[0].label.contains("Recommended"));
    }

    #[test]
    fn provided_duplicate_options_are_regenerated_per_question() {
        let duplicate_options = vec![
            RequestUserInputOption {
                label: "Minimal implementation slice (Recommended)".to_string(),
                description: "Ship only the smallest possible scope.".to_string(),
            },
            RequestUserInputOption {
                label: "Balanced implementation".to_string(),
                description: "Ship medium scope with moderate risk.".to_string(),
            },
            RequestUserInputOption {
                label: "Comprehensive implementation".to_string(),
                description: "Ship full scope with deeper validation.".to_string(),
            },
        ];

        let questions = vec![
            RequestUserInputQuestion {
                id: "goal".to_string(),
                header: "Goal".to_string(),
                question: "What user-visible outcome should this change deliver, and what constraints or non-goals must be respected?".to_string(),
                options: Some(duplicate_options.clone()),
                focus_area: None,
                analysis_hints: Vec::new(),
            },
            RequestUserInputQuestion {
                id: "constraints".to_string(),
                header: "Plan".to_string(),
                question: "Break the work into 3-7 composable steps. For each step include target file(s) and a concrete expected outcome.".to_string(),
                options: Some(duplicate_options.clone()),
                focus_area: None,
                analysis_hints: Vec::new(),
            },
            RequestUserInputQuestion {
                id: "verification".to_string(),
                header: "Verification".to_string(),
                question: "For each step, what exact command or manual check proves it is complete?"
                    .to_string(),
                options: Some(duplicate_options),
                focus_area: None,
                analysis_hints: Vec::new(),
            },
        ];

        let resolved = resolve_question_options(&questions);
        assert_eq!(resolved.len(), 3);

        let goal_labels = resolved[0]
            .as_ref()
            .expect("goal options should resolve")
            .iter()
            .map(|option| option.label.clone())
            .collect::<Vec<_>>();
        let step_labels = resolved[1]
            .as_ref()
            .expect("step options should resolve")
            .iter()
            .map(|option| option.label.clone())
            .collect::<Vec<_>>();
        let verification_labels = resolved[2]
            .as_ref()
            .expect("verification options should resolve")
            .iter()
            .map(|option| option.label.clone())
            .collect::<Vec<_>>();

        assert_ne!(goal_labels, step_labels);
        assert_ne!(step_labels, verification_labels);
        assert_ne!(goal_labels, verification_labels);
    }

    #[test]
    fn valid_provided_options_are_preserved() {
        let provided_options = vec![
            RequestUserInputOption {
                label: "Outcome KPI (Recommended)".to_string(),
                description: "Define one measurable user-visible result.".to_string(),
            },
            RequestUserInputOption {
                label: "Constraint checklist".to_string(),
                description: "Lock boundaries before implementation.".to_string(),
            },
            RequestUserInputOption {
                label: "MVP boundary".to_string(),
                description: "Limit scope to the smallest deliverable.".to_string(),
            },
        ];

        let questions = vec![RequestUserInputQuestion {
            id: "goal".to_string(),
            header: "Goal".to_string(),
            question: "What user-visible outcome should this change deliver, and what constraints or non-goals must be respected?".to_string(),
            options: Some(provided_options.clone()),
            focus_area: None,
            analysis_hints: Vec::new(),
        }];

        let resolved = resolve_question_options(&questions);
        let resolved_options = resolved[0]
            .as_ref()
            .expect("provided options should be preserved");

        assert_eq!(resolved_options.len(), provided_options.len());
        for (resolved_option, provided_option) in resolved_options.iter().zip(provided_options) {
            assert_eq!(resolved_option.label, provided_option.label);
            assert_eq!(resolved_option.description, provided_option.description);
        }
    }

    #[test]
    fn id_keyword_does_not_override_question_text_intent() {
        let question = RequestUserInputQuestion {
            id: "constraints".to_string(),
            header: "Plan".to_string(),
            question: "For each step, what exact command or manual check proves it is complete?"
                .to_string(),
            options: None,
            focus_area: None,
            analysis_hints: Vec::new(),
        };

        let options = generate_suggested_options(&question).expect("verification options");
        let labels = options
            .iter()
            .map(|option| option.label.to_lowercase())
            .collect::<Vec<_>>();

        assert!(
            labels
                .iter()
                .any(|label| label.contains("command-based proof"))
        );
        assert!(
            !labels
                .iter()
                .any(|label| label.contains("dependency-first slices"))
        );
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

    #[test]
    fn structured_payload_normalizes_to_multi_step_mode() {
        let args = json!({
            "questions": [
                {
                    "id": "scope",
                    "header": "Scope",
                    "question": "Which direction should we take?",
                    "options": [
                        {"label": "Minimal (Recommended)", "description": "Smallest viable slice"},
                        {"label": "Full", "description": "Complete implementation"}
                    ]
                }
            ]
        });

        let normalized = normalize_request_user_input_args(&args).expect("normalize structured");
        assert_eq!(normalized.args.questions.len(), 1);
        assert_eq!(normalized.wizard_mode, WizardModalMode::MultiStep);
        assert_eq!(normalized.current_step, 0);
        assert_eq!(normalized.title_override, None);
    }

    #[test]
    fn legacy_payload_is_rejected() {
        let legacy_args = json!({
            "question": "Choose one",
            "tabs": [
                {
                    "id": "scope",
                    "title": "Scope",
                    "items": [
                        {"id": "minimal", "title": "Minimal scope"},
                        {"id": "full", "title": "Full scope"}
                    ]
                }
            ]
        });
        let result = normalize_request_user_input_args(&legacy_args);
        assert!(result.is_err());
    }
}
