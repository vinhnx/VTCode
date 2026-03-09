use super::schema::{RequestUserInputOption, RequestUserInputQuestion};

pub(super) fn generate_suggested_options(
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
        QuestionIntent::GenericPlanning => generic_planning_options(),
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

pub(super) fn generic_planning_options() -> Vec<RequestUserInputOption> {
    vec![
        RequestUserInputOption {
            label: "Proceed with best default".to_string(),
            description:
                "Continue with the most conservative implementation path and document assumptions explicitly."
                    .to_string(),
        },
        RequestUserInputOption {
            label: "Constrain scope first".to_string(),
            description:
                "Lock a tighter MVP boundary before implementation to reduce risk and rework."
                    .to_string(),
        },
        RequestUserInputOption {
            label: "Surface key tradeoffs".to_string(),
            description:
                "Clarify the highest-impact tradeoff first so plan and execution stay aligned."
                    .to_string(),
        },
    ]
}

fn priority_selection_options(
    local_context: &str,
    global_context: &str,
) -> Vec<RequestUserInputOption> {
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

pub(super) fn contains_any(text: &str, needles: &[&str]) -> bool {
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
