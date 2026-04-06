use anyhow::{Result, bail};
use hashbrown::HashMap;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use vtcode_tui::app::WizardModalMode;

#[derive(Debug, Deserialize)]
pub(super) struct RequestUserInputArgs {
    pub(super) questions: Vec<RequestUserInputQuestion>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RequestUserInputQuestion {
    pub(super) id: String,
    pub(super) header: String,
    pub(super) question: String,

    #[serde(default)]
    pub(super) options: Option<Vec<RequestUserInputOption>>,
    #[serde(default)]
    pub(super) focus_area: Option<String>,
    #[serde(default)]
    pub(super) analysis_hints: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RequestUserInputOption {
    pub(super) label: String,
    pub(super) description: String,
}

#[derive(Debug, serde::Serialize)]
pub(super) struct RequestUserInputResponse {
    pub(super) answers: HashMap<String, RequestUserInputAnswer>,
}

#[derive(Debug, serde::Serialize)]
pub(super) struct RequestUserInputAnswer {
    pub(super) selected: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) other: Option<String>,
}

pub(super) struct NormalizedRequestUserInput {
    pub(super) args: RequestUserInputArgs,
    pub(super) wizard_mode: WizardModalMode,
    pub(super) current_step: usize,
    pub(super) title_override: Option<String>,
    pub(super) allow_freeform: bool,
    pub(super) freeform_label: Option<String>,
    pub(super) freeform_placeholder: Option<String>,
}

pub(super) fn normalize_request_user_input_args(
    args: &Value,
) -> Result<NormalizedRequestUserInput> {
    let parsed: RequestUserInputArgs = serde_json::from_value(args.clone())?;
    validate_questions(&parsed.questions)?;
    Ok(NormalizedRequestUserInput {
        args: parsed,
        wizard_mode: WizardModalMode::MultiStep,
        current_step: 0,
        title_override: None,
        allow_freeform: true,
        freeform_label: Some("Custom note".to_string()),
        freeform_placeholder: Some("Type your response...".to_string()),
    })
}

pub(crate) fn normalize_request_user_input_fallback_args(args_val: &Value) -> Option<Value> {
    let single_text_question = args_val
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if let Some(question) = single_text_question {
        return Some(json!({
            "questions": [{
                "id": "question_1",
                "header": "Question",
                "question": question
            }]
        }));
    }

    let args_obj = args_val.as_object()?;

    if let Some(questions_value) = find_case_insensitive_field(args_obj, "questions") {
        let mut normalized_questions = Vec::new();
        match questions_value {
            Value::Array(entries) => {
                for (index, entry) in entries.iter().enumerate() {
                    if let Some(obj) = entry.as_object()
                        && let Some(question) = normalize_fallback_question(obj, index)
                    {
                        normalized_questions.push(Value::Object(question));
                    }
                }
            }
            Value::Object(obj) => {
                if let Some(question) = normalize_fallback_question(obj, 0) {
                    normalized_questions.push(Value::Object(question));
                }
            }
            _ => {}
        }
        if !normalized_questions.is_empty() {
            return Some(json!({ "questions": normalized_questions }));
        }
    }

    if let Some(tabs_value) = find_case_insensitive_field(args_obj, "tabs")
        && let Some(first_tab) = tabs_value.as_array().and_then(|tabs| tabs.first())
        && let Some(tab_obj) = first_tab.as_object()
    {
        let question_text = find_case_insensitive_field(args_obj, "question")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .or_else(|| {
                find_case_insensitive_field(tab_obj, "question")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
            })
            .unwrap_or("What should we prioritize?");
        let question_id = find_case_insensitive_field(tab_obj, "id")
            .and_then(Value::as_str)
            .or_else(|| find_case_insensitive_field(args_obj, "id").and_then(Value::as_str));
        let header_source = find_case_insensitive_field(tab_obj, "title")
            .and_then(Value::as_str)
            .or_else(|| find_case_insensitive_field(args_obj, "header").and_then(Value::as_str));

        let mut question = serde_json::Map::new();
        question.insert(
            "id".to_string(),
            Value::String(normalize_fallback_question_id(question_id, 0)),
        );
        question.insert(
            "header".to_string(),
            Value::String(normalize_fallback_header(header_source, "Question")),
        );
        question.insert(
            "question".to_string(),
            Value::String(question_text.to_string()),
        );
        if let Some(items) = find_case_insensitive_field(tab_obj, "items")
            && let Some(options) = normalize_fallback_options(items)
        {
            question.insert("options".to_string(), Value::Array(options));
        }

        return Some(json!({ "questions": [Value::Object(question)] }));
    }

    if let Some(question_text) = find_case_insensitive_field(args_obj, "question")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let mut question = serde_json::Map::new();
        let question_id = find_case_insensitive_field(args_obj, "id").and_then(Value::as_str);
        let header_source = find_case_insensitive_field(args_obj, "header").and_then(Value::as_str);
        question.insert(
            "id".to_string(),
            Value::String(normalize_fallback_question_id(question_id, 0)),
        );
        question.insert(
            "header".to_string(),
            Value::String(normalize_fallback_header(header_source, "Question")),
        );
        question.insert(
            "question".to_string(),
            Value::String(question_text.to_string()),
        );
        if let Some(options_value) = find_case_insensitive_field(args_obj, "options")
            && let Some(options) = normalize_fallback_options(options_value)
        {
            question.insert("options".to_string(), Value::Array(options));
        }

        return Some(json!({ "questions": [Value::Object(question)] }));
    }

    None
}

pub(super) fn validate_questions(questions: &[RequestUserInputQuestion]) -> Result<()> {
    if questions.is_empty() || questions.len() > 3 {
        bail!("questions must contain 1 to 3 entries");
    }

    for question in questions {
        if !is_snake_case(&question.id) {
            bail!(
                "question id '{}' must be snake_case (letters, digits, underscore)",
                question.id
            );
        }

        let header = question.header.trim();
        if header.is_empty() {
            bail!("question header cannot be empty");
        }
        if header.chars().count() > 12 {
            bail!(
                "question header '{}' must be 12 characters or fewer",
                header
            );
        }

        let prompt = question.question.trim();
        if prompt.is_empty() {
            bail!("question prompt cannot be empty");
        }
        if prompt.contains('\n') {
            bail!(
                "question '{}' must be a single sentence on one line",
                question.id
            );
        }
    }

    Ok(())
}

fn find_case_insensitive_field<'a>(
    obj: &'a serde_json::Map<String, Value>,
    key: &str,
) -> Option<&'a Value> {
    obj.get(key).or_else(|| {
        obj.iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| value)
    })
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn normalize_fallback_question_id(raw: Option<&str>, index: usize) -> String {
    let source = raw.unwrap_or_default();
    let mut out = String::with_capacity(source.len());
    let mut last_was_underscore = false;
    for ch in source.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_underscore = false;
        } else if !last_was_underscore {
            out.push('_');
            last_was_underscore = true;
        }
    }
    let mut out = out.trim_matches('_').to_string();
    if out.is_empty() {
        return format!("question_{}", index + 1);
    }
    if !out
        .chars()
        .next()
        .map(|ch| ch.is_ascii_lowercase())
        .unwrap_or(false)
    {
        out.insert(0, 'q');
    }
    out
}

fn normalize_fallback_header(raw: Option<&str>, fallback: &str) -> String {
    let candidate = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback);
    truncate_chars(candidate, 12)
}

fn normalize_fallback_option(value: &Value) -> Option<Value> {
    match value {
        Value::String(label) => {
            let label = label.trim();
            if label.is_empty() {
                return None;
            }
            Some(json!({
                "label": label,
                "description": "Select this option."
            }))
        }
        Value::Object(obj) => {
            let label = ["label", "title", "id"]
                .iter()
                .find_map(|key| find_case_insensitive_field(obj, key).and_then(Value::as_str))
                .map(str::trim)
                .filter(|value| !value.is_empty())?;
            let description = ["description", "subtitle", "details"]
                .iter()
                .find_map(|key| find_case_insensitive_field(obj, key).and_then(Value::as_str))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("Select this option.");
            Some(json!({
                "label": label,
                "description": description
            }))
        }
        _ => None,
    }
}

fn normalize_fallback_options(value: &Value) -> Option<Vec<Value>> {
    let Value::Array(raw_options) = value else {
        return None;
    };
    let mut normalized = Vec::new();
    let mut seen_labels = BTreeSet::new();
    for option in raw_options {
        let Some(normalized_option) = normalize_fallback_option(option) else {
            continue;
        };
        let label = normalized_option
            .get("label")
            .and_then(Value::as_str)
            .map(|value| value.to_ascii_lowercase());
        if let Some(label) = label
            && !seen_labels.insert(label)
        {
            continue;
        }
        normalized.push(normalized_option);
        if normalized.len() == 3 {
            break;
        }
    }
    if normalized.len() >= 2 {
        Some(normalized)
    } else {
        None
    }
}

fn normalize_fallback_question(
    obj: &serde_json::Map<String, Value>,
    index: usize,
) -> Option<serde_json::Map<String, Value>> {
    let question_text = ["question", "prompt", "text"]
        .iter()
        .find_map(|key| find_case_insensitive_field(obj, key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    let mut question = serde_json::Map::new();
    let question_id = ["id", "question_id", "name"]
        .iter()
        .find_map(|key| find_case_insensitive_field(obj, key).and_then(Value::as_str));
    question.insert(
        "id".to_string(),
        Value::String(normalize_fallback_question_id(question_id, index)),
    );
    let header_source = ["header", "title"]
        .iter()
        .find_map(|key| find_case_insensitive_field(obj, key).and_then(Value::as_str));
    question.insert(
        "header".to_string(),
        Value::String(normalize_fallback_header(header_source, "Question")),
    );
    question.insert(
        "question".to_string(),
        Value::String(question_text.to_string()),
    );

    if let Some(options_value) = find_case_insensitive_field(obj, "options")
        .or_else(|| find_case_insensitive_field(obj, "items"))
        && let Some(options) = normalize_fallback_options(options_value)
    {
        question.insert("options".to_string(), Value::Array(options));
    }

    Some(question)
}

fn is_snake_case(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }

    let mut last_was_underscore = false;
    for ch in chars {
        if ch == '_' {
            if last_was_underscore {
                return false;
            }
            last_was_underscore = true;
            continue;
        }

        if !ch.is_ascii_lowercase() && !ch.is_ascii_digit() {
            return false;
        }
        last_was_underscore = false;
    }

    !last_was_underscore
}
