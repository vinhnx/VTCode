use hashbrown::{HashMap, HashSet};

use super::schema::{RequestUserInputOption, RequestUserInputQuestion};
use super::suggestions::{contains_any, generate_suggested_options, generic_planning_options};

pub(super) fn resolve_question_options(
    questions: &[RequestUserInputQuestion],
) -> Vec<Option<Vec<RequestUserInputOption>>> {
    let mut provided_signature_counts: HashMap<String, usize> = HashMap::new();
    for question in questions {
        if let Some(options) = question.options.as_ref() {
            let sanitized = sanitize_provided_options(options);
            let signature = options_signature(&sanitized);
            if !signature.is_empty() {
                *provided_signature_counts.entry(signature).or_insert(0) += 1;
            }
        }
    }

    questions
        .iter()
        .map(|question| match question.options.clone() {
            Some(provided_options) => {
                let sanitized = sanitize_provided_options(&provided_options);
                let signature = options_signature(&sanitized);
                let repeated_signature = provided_signature_counts
                    .get(&signature)
                    .copied()
                    .unwrap_or(0)
                    > 1;
                if should_regenerate_provided_options(question, &sanitized, repeated_signature) {
                    generate_suggested_options(question)
                        .or_else(|| Some(generic_planning_options()))
                } else {
                    Some(sanitized)
                }
            }
            None => {
                generate_suggested_options(question).or_else(|| Some(generic_planning_options()))
            }
        })
        .collect()
}

fn should_regenerate_provided_options(
    question: &RequestUserInputQuestion,
    options: &[RequestUserInputOption],
    repeated_signature: bool,
) -> bool {
    if options.len() < 2 || options.len() > 3 {
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

pub(super) fn ensure_recommended_first(
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

pub(super) fn sanitize_provided_options(
    options: &[RequestUserInputOption],
) -> Vec<RequestUserInputOption> {
    let mut seen_labels = HashSet::new();
    let mut sanitized = Vec::new();

    for option in options {
        let label = option.label.trim();
        let description = option.description.trim();
        if label.is_empty() || description.is_empty() {
            continue;
        }

        if is_other_option_label(label) {
            continue;
        }

        let normalized = normalize_option_text(label);
        if normalized.is_empty() || !seen_labels.insert(normalized) {
            continue;
        }

        sanitized.push(RequestUserInputOption {
            label: label.to_string(),
            description: description.to_string(),
        });

        if sanitized.len() == 3 {
            break;
        }
    }

    sanitized
}

fn is_other_option_label(label: &str) -> bool {
    let normalized = normalize_option_text(label);
    normalized == "other" || normalized.starts_with("other ")
}
