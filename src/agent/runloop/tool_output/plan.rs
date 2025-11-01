use anyhow::{Context, Result};
use serde_json::Value;
use vtcode_core::tools::{PlanCompletionState, StepStatus, TaskPlan};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::panels::{PanelContentLine, clamp_panel_text, render_panel, wrap_text};

pub(crate) fn render_plan_update(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    if let Some(error) = val.get("error") {
        renderer.line(MessageStyle::Info, "[plan] Update failed")?;
        render_plan_error(renderer, error)?;
        return Ok(());
    }

    let plan_value = match val.get("plan").cloned() {
        Some(value) => value,
        None => {
            renderer.line(MessageStyle::Error, "[plan] No plan data returned")?;
            return Ok(());
        }
    };

    let plan: TaskPlan =
        serde_json::from_value(plan_value).context("Plan tool returned malformed plan payload")?;

    let heading = val
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or("Plan updated");

    renderer.line(MessageStyle::Info, &format!("[plan] {}", heading))?;

    if matches!(plan.summary.status, PlanCompletionState::Empty) {
        renderer.line(MessageStyle::Info, "  No tasks defined")?;
        return Ok(());
    }

    renderer.set_plan(&plan);

    render_plan_panel(renderer, &plan)?;
    Ok(())
}

fn render_plan_panel(renderer: &mut AnsiRenderer, plan: &TaskPlan) -> Result<()> {
    const PANEL_WIDTH: u16 = 100;
    let content_width = PANEL_WIDTH.saturating_sub(4) as usize;

    let mut lines = Vec::new();
    let progress = format!(
        "  Progress: {}/{} completed",
        plan.summary.completed_steps, plan.summary.total_steps
    );
    lines.push(PanelContentLine::new(
        clamp_panel_text(&progress, content_width),
        MessageStyle::Info,
    ));

    let explanation_line = plan
        .explanation
        .as_ref()
        .and_then(|text| text.lines().next())
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(|line| clamp_panel_text(line, content_width));

    if explanation_line.is_some() || !plan.steps.is_empty() {
        lines.push(PanelContentLine::new(String::new(), MessageStyle::Info));
    }

    if let Some(line) = explanation_line {
        lines.push(PanelContentLine::new(line, MessageStyle::Info));
        if !plan.steps.is_empty() {
            lines.push(PanelContentLine::new(String::new(), MessageStyle::Info));
        }
    }

    for (index, step) in plan.steps.iter().enumerate() {
        let (checkbox, _style) = match step.status {
            StepStatus::Pending => ("[ ]", MessageStyle::Info),
            StepStatus::InProgress => ("[>]", MessageStyle::Tool),
            StepStatus::Completed => ("[x]", MessageStyle::Response),
        };
        let step_text = step.step.trim();
        let step_number = index + 1;

        // Calculate prefix length (e.g., "1. [x] ")
        let prefix = format!("{step_number}. {checkbox} ");
        let prefix_len = prefix.chars().count();

        if prefix_len >= content_width {
            // If prefix is too long, just truncate the whole thing
            let content = format!("{step_number}. {checkbox} {step_text}");
            let truncated = clamp_panel_text(&content, content_width);
            lines.push(PanelContentLine::new(truncated, MessageStyle::Info));
        } else {
            // Wrap the step text to multiple lines
            let available_width = content_width - prefix_len;
            let wrapped_lines = wrap_text(step_text, available_width);

            for (line_idx, line) in wrapped_lines.into_iter().enumerate() {
                let content = if line_idx == 0 {
                    format!("{prefix}{line}")
                } else {
                    format!("{}{}", " ".repeat(prefix_len), line)
                };
                lines.push(PanelContentLine::new(content, MessageStyle::Info));
            }
        }
    }

    render_panel(renderer, None, lines, MessageStyle::Info)
}

fn render_plan_error(renderer: &mut AnsiRenderer, error: &Value) -> Result<()> {
    let error_message = error
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or("Plan update failed due to an unknown error.");
    let error_type = error
        .get("error_type")
        .and_then(|value| value.as_str())
        .unwrap_or("Unknown");

    renderer.line(
        MessageStyle::Error,
        &format!("  {} ({})", error_message, error_type),
    )?;

    if let Some(original_error) = error
        .get("original_error")
        .and_then(|value| value.as_str())
        .filter(|message| !message.is_empty())
    {
        renderer.line(
            MessageStyle::Info,
            &format!("  Details: {}", original_error),
        )?;
    }

    if let Some(suggestions) = error
        .get("recovery_suggestions")
        .and_then(|value| value.as_array())
    {
        let tips: Vec<_> = suggestions
            .iter()
            .filter_map(|suggestion| suggestion.as_str())
            .collect();
        if !tips.is_empty() {
            renderer.line(MessageStyle::Info, "  Recovery suggestions:")?;
            for tip in tips {
                renderer.line(MessageStyle::Info, &format!("    - {}", tip))?;
            }
        }
    }

    Ok(())
}
