use anyhow::{Result, bail};
use hashbrown::HashMap;
use serde::Deserialize;
use serde_json::Value;
use vtcode_tui::WizardModalMode;

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
