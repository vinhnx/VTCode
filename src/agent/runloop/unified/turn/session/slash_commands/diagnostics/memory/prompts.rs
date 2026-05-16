use anyhow::Result;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_tui::app::{InlineListItem, InlineListSelection, WizardModalMode, WizardStep};

use crate::agent::runloop::unified::wizard_modal::{
    WizardModalOutcome, show_wizard_modal_and_wait,
};

use super::{MEMORY_PROMPT_QUESTION_ID, SlashCommandContext};

pub(super) async fn prompt_required_text(
    ctx: &mut SlashCommandContext<'_>,
    title: &str,
    question: &str,
    freeform_label: &str,
    placeholder: &str,
    default_value: Option<String>,
) -> Result<Option<String>> {
    let Some(value) = prompt_text(
        ctx,
        title,
        question,
        freeform_label,
        placeholder,
        default_value,
        false,
    )
    .await?
    else {
        return Ok(None);
    };
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        ctx.renderer
            .line(MessageStyle::Info, "Input was empty. Nothing changed.")?;
        return Ok(None);
    }
    Ok(Some(trimmed))
}

pub(super) async fn prompt_optional_text(
    ctx: &mut SlashCommandContext<'_>,
    title: &str,
    question: &str,
    freeform_label: &str,
    placeholder: &str,
    default_value: Option<String>,
) -> Result<Option<String>> {
    prompt_text(
        ctx,
        title,
        question,
        freeform_label,
        placeholder,
        default_value,
        true,
    )
    .await
}

async fn prompt_text(
    ctx: &mut SlashCommandContext<'_>,
    title: &str,
    question: &str,
    freeform_label: &str,
    placeholder: &str,
    default_value: Option<String>,
    allow_empty: bool,
) -> Result<Option<String>> {
    let step = build_diagnostics_prompt_step(question, freeform_label, placeholder, default_value);

    let outcome = show_wizard_modal_and_wait(
        ctx.handle,
        ctx.session,
        title.to_string(),
        vec![step],
        0,
        None,
        WizardModalMode::MultiStep,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
    )
    .await?;
    let value = match outcome {
        WizardModalOutcome::Submitted(selections) => {
            selections
                .into_iter()
                .find_map(|selection| match selection {
                    InlineListSelection::RequestUserInputAnswer {
                        question_id,
                        selected,
                        other,
                    } if question_id == MEMORY_PROMPT_QUESTION_ID => {
                        other.or_else(|| selected.first().cloned())
                    }
                    _ => None,
                })
        }
        WizardModalOutcome::Cancelled { .. } => None,
    };

    let Some(value) = value else {
        return Ok(None);
    };
    if allow_empty {
        return Ok(Some(value));
    }

    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(Some(trimmed))
}

pub(super) fn build_diagnostics_prompt_step(
    question: &str,
    freeform_label: &str,
    placeholder: &str,
    default_value: Option<String>,
) -> WizardStep {
    WizardStep {
        title: "Input".to_string(),
        question: question.to_string(),
        items: vec![InlineListItem {
            title: "Submit".to_string(),
            subtitle: Some("Press Tab to type text, then Enter to submit.".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: MEMORY_PROMPT_QUESTION_ID.to_string(),
                selected: vec![],
                other: Some(String::new()),
            }),
            search_value: Some("submit memory input".to_string()),
        }],
        completed: false,
        answer: None,
        allow_freeform: true,
        freeform_label: Some(freeform_label.to_string()),
        freeform_placeholder: Some(placeholder.to_string()),
        freeform_default: default_value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_prompt_step_keeps_placeholder_only_when_no_default_is_set() {
        let step = build_diagnostics_prompt_step(
            "Add an exclude glob.",
            "Pattern",
            "**/other-team/.vtcode/rules/**",
            None,
        );

        assert_eq!(
            step.freeform_placeholder.as_deref(),
            Some("**/other-team/.vtcode/rules/**")
        );
        assert_eq!(step.freeform_default, None);
    }

    #[test]
    fn diagnostics_prompt_step_uses_explicit_current_value_default() {
        let step = build_diagnostics_prompt_step(
            "Enter the byte budget.",
            "Bytes",
            "25600",
            Some("25600".to_string()),
        );

        assert_eq!(step.freeform_placeholder.as_deref(), Some("25600"));
        assert_eq!(step.freeform_default.as_deref(), Some("25600"));
    }
}
