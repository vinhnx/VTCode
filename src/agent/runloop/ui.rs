use anstyle::RgbColor;
use anyhow::{Context, Result};
use pathdiff::diff_paths;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color as RatColor, Modifier, Style as RatStyle},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::tool_policy::{ToolPolicy, ToolPolicyManager};
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::AnsiRenderer;

use super::welcome::SessionBootstrap;
use crate::workspace_trust;

const LOGO_PREFIX: &str = "> VT Code";
const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");
const PANEL_PADDING: u16 = 1;

fn logo_text() -> String {
    format!("{} v{}", LOGO_PREFIX, PACKAGE_VERSION)
}

fn ratatui_color_from_rgb(color: RgbColor) -> RatColor {
    let RgbColor(red, green, blue) = color;
    RatColor::Rgb(red, green, blue)
}

fn render_logo_panel_lines(
    model_label: &str,
    reasoning_label: &str,
    hitl_enabled: Option<bool>,
) -> Vec<String> {
    let accent_color = ratatui_color_from_rgb(theme::logo_accent_color());
    let header_style = RatStyle::default()
        .fg(accent_color)
        .add_modifier(Modifier::BOLD);
    let label_style = RatStyle::default()
        .fg(accent_color)
        .add_modifier(Modifier::BOLD);

    let mut body_lines: Vec<Line<'static>> = Vec::new();
    body_lines.push(Line::from(vec![
        Span::styled("Model:".to_string(), label_style),
        Span::raw(format!(" {}", model_label)),
    ]));
    body_lines.push(Line::from(vec![
        Span::styled("Reasoning:".to_string(), label_style),
        Span::raw(format!(" {}", reasoning_label)),
    ]));

    if let Some(enabled) = hitl_enabled {
        let status = if enabled {
            "HITL enabled (full text)"
        } else {
            "HITL disabled"
        };
        body_lines.push(Line::from(vec![
            Span::styled("Safeguards:".to_string(), label_style),
            Span::raw(format!(" {}", status)),
        ]));
    }

    let mut inner_max_width = body_lines.iter().map(Line::width).max().unwrap_or(0);
    let logo = logo_text();
    let title_width = UnicodeWidthStr::width(logo.as_str());
    inner_max_width = inner_max_width.max(title_width);

    let horizontal_padding = (PANEL_PADDING as usize) * 2;
    let total_width = (inner_max_width + horizontal_padding + 2) as u16; // borders add 2
    let total_height = (body_lines.len() as u16 + 2).max(3); // ensure room for borders

    let block = Block::default()
        .title(Line::from(vec![Span::styled(logo, header_style)]))
        .borders(Borders::ALL)
        .padding(Padding::horizontal(PANEL_PADDING));

    let paragraph = Paragraph::new(body_lines).block(block);
    let area = Rect::new(0, 0, total_width, total_height);
    let mut buffer = Buffer::empty(area);
    paragraph.render(area, &mut buffer);

    let mut rendered = Vec::new();
    for y in 0..area.height {
        let mut line = String::new();
        for x in 0..area.width {
            if let Some(cell) = buffer.cell((x, y)) {
                line.push_str(cell.symbol());
            }
        }
        let trimmed = line.trim_end().to_string();
        rendered.push(trimmed);
    }

    while matches!(rendered.last(), Some(last) if last.is_empty()) {
        rendered.pop();
    }

    rendered
}

pub(crate) fn render_session_banner(
    renderer: &mut AnsiRenderer,
    config: &CoreAgentConfig,
    session_bootstrap: &SessionBootstrap,
    model_label: &str,
    reasoning_label: &str,
) -> Result<()> {
    let banner_style = theme::banner_style();
    let panel_lines = render_logo_panel_lines(
        model_label,
        reasoning_label,
        session_bootstrap.human_in_the_loop,
    );
    for line in panel_lines {
        renderer.line_with_style(banner_style, &line)?;
    }

    let mut status_lines = Vec::new();

    let trust_summary = workspace_trust::workspace_trust_level(&config.workspace)
        .context("Failed to determine workspace trust level for banner")?
        .map(|level| format!("Trust: {}", level))
        .unwrap_or_else(|| "Trust: unavailable".to_string());
    status_lines.push(trust_summary);

    match ToolPolicyManager::new_with_workspace(&config.workspace) {
        Ok(manager) => {
            let summary = manager.get_policy_summary();
            let mut allow = 0usize;
            let mut prompt = 0usize;
            let mut deny = 0usize;
            for policy in summary.values() {
                match policy {
                    ToolPolicy::Allow => allow += 1,
                    ToolPolicy::Prompt => prompt += 1,
                    ToolPolicy::Deny => deny += 1,
                }
            }
            let policy_path = diff_paths(manager.config_path(), &config.workspace)
                .and_then(|p| p.to_str().map(|s| s.to_string()))
                .unwrap_or_else(|| manager.config_path().display().to_string());
            status_lines.push(format!(
                "Tools policy: allow {} · prompt {} · deny {} ({})",
                allow, prompt, deny, policy_path
            ));
        }
        Err(err) => {
            status_lines.push(format!("Tools policy: unavailable ({})", err));
        }
    }

    if let Some(summary) = session_bootstrap.language_summary.as_deref() {
        status_lines.push(format!("Stack: {}", summary));
    }

    // Add MCP status to welcome banner
    if let Some(ref mcp_error) = session_bootstrap.mcp_error {
        status_lines.push(format!("MCP: error - {}", mcp_error));
    } else if let Some(mcp_enabled) = session_bootstrap.mcp_enabled {
        if mcp_enabled && session_bootstrap.mcp_providers.is_some() {
            let providers = session_bootstrap.mcp_providers.as_ref().unwrap();
            let enabled_providers: Vec<&str> = providers
                .iter()
                .filter(|p| p.enabled)
                .map(|p| p.name.as_str())
                .collect();
            if enabled_providers.is_empty() {
                status_lines.push("MCP: enabled (no providers)".to_string());
            } else {
                status_lines.push(format!("MCP: enabled ({})", enabled_providers.join(", ")));
            }
        } else {
            let status = if mcp_enabled { "enabled" } else { "disabled" };
            status_lines.push(format!("MCP: {}", status));
        }
    }

    if !status_lines.is_empty() {
        renderer.line_with_style(banner_style, "")?;
    }

    for line in status_lines {
        renderer.line_with_style(banner_style, &format!("• {}", line))?;
    }

    renderer.line_with_style(banner_style, "")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logo_panel_contains_expected_details() {
        let lines = render_logo_panel_lines("x-ai/grok-4-fast:free", "A7 · P11 · D0", Some(true));
        assert!(lines.iter().any(|line| line.contains(&logo_text())));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("Model: x-ai/grok-4-fast:free"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("Reasoning: A7 · P11 · D0"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("Safeguards: HITL enabled"))
        );
    }
}
