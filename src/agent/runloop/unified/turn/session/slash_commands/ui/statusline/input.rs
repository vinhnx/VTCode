use anyhow::{Context, Result};
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_ui::tui::app::{InlineListItem, InlineListSelection, WizardModalMode, WizardStep};

use crate::agent::runloop::unified::turn::session::slash_commands::SlashCommandContext;
use crate::agent::runloop::unified::wizard_modal::{WizardModalOutcome, show_wizard_modal_and_wait};

const STATUSLINE_INPUT_ID: &str = "statusline.input";

pub(super) async fn prompt_statusline_input(
    ctx: &mut SlashCommandContext<'_>,
    title: &str,
    question: &str,
    freeform_label: &str,
    placeholder: &str,
    default_value: Option<String>,
    allow_empty: bool,
) -> Result<Option<String>> {
    let step = build_statusline_prompt_step(question, freeform_label, placeholder, default_value);

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
        WizardModalOutcome::Submitted(selections) => selections.into_iter().find_map(|selection| match selection {
            InlineListSelection::RequestUserInputAnswer { question_id, selected, other }
                if question_id == STATUSLINE_INPUT_ID =>
            {
                other.or_else(|| selected.first().cloned())
            }
            _ => None,
        }),
        WizardModalOutcome::Cancelled { .. } => None,
    };
    let Some(value) = value else {
        return Ok(None);
    };

    let trimmed = value.trim().to_string();
    if trimmed.is_empty() && !allow_empty {
        ctx.renderer.line(MessageStyle::Info, "Input was empty. Nothing changed.")?;
        return Ok(None);
    }
    if trimmed.is_empty() {
        return Ok(Some(String::new()));
    }
    Ok(Some(trimmed))
}

fn build_statusline_prompt_step(
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
            subtitle: Some("Press Enter to accept the default, or Tab to type a custom value.".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: STATUSLINE_INPUT_ID.to_string(),
                selected: vec![],
                other: Some(String::new()),
            }),
            search_value: Some("submit statusline input".to_string()),
            ..Default::default()
        }],
        completed: false,
        answer: None,
        allow_freeform: true,
        freeform_label: Some(freeform_label.to_string()),
        freeform_placeholder: Some(placeholder.to_string()),
        freeform_default: default_value,
    }
}

pub(super) fn parse_statusline_millis(value: &str, label: &str) -> Result<u64> {
    value
        .trim()
        .parse::<u64>()
        .with_context(|| format!("Failed to parse {label} as milliseconds"))
}
