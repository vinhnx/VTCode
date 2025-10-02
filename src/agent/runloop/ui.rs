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
use vtcode_core::config::constants::ui;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::tool_policy::{ToolPolicy, ToolPolicyManager};
use vtcode_core::ui::theme;
use vtcode_core::ui::tui::InlineHeaderContext;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_core::utils::dot_config::WorkspaceTrustLevel;

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

#[derive(Clone, Debug)]
enum ToolStatusSummary {
    Available {
        allow: usize,
        prompt: usize,
        deny: usize,
        policy_path: String,
    },
    Unavailable(String),
}

#[derive(Clone, Debug)]
enum McpStatusSummary {
    Enabled {
        active_providers: Vec<String>,
        configured: bool,
    },
    Disabled,
    Error(String),
    Unknown,
}

#[derive(Clone, Debug)]
struct InlineStatusDetails {
    workspace_trust: Option<WorkspaceTrustLevel>,
    tool_status: ToolStatusSummary,
    language_summary: Option<String>,
    mcp_status: McpStatusSummary,
}

fn gather_inline_status_details(
    config: &CoreAgentConfig,
    session_bootstrap: &SessionBootstrap,
) -> Result<InlineStatusDetails> {
    let workspace_trust = workspace_trust::workspace_trust_level(&config.workspace)
        .context("Failed to determine workspace trust level for banner")?;

    let tool_status = match ToolPolicyManager::new_with_workspace(&config.workspace) {
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
                .and_then(|path| path.to_str().map(|value| value.to_string()))
                .unwrap_or_else(|| manager.config_path().display().to_string());
            ToolStatusSummary::Available {
                allow,
                prompt,
                deny,
                policy_path,
            }
        }
        Err(err) => ToolStatusSummary::Unavailable(err.to_string()),
    };

    let language_summary = session_bootstrap
        .language_summary
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string());

    let mcp_status = if let Some(error) = session_bootstrap.mcp_error.clone() {
        McpStatusSummary::Error(error)
    } else if let Some(enabled) = session_bootstrap.mcp_enabled {
        if enabled {
            let configured = session_bootstrap.mcp_providers.is_some();
            let active_providers = session_bootstrap
                .mcp_providers
                .as_ref()
                .map(|providers| {
                    providers
                        .iter()
                        .filter(|provider| provider.enabled)
                        .map(|provider| provider.name.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            McpStatusSummary::Enabled {
                active_providers,
                configured,
            }
        } else {
            McpStatusSummary::Disabled
        }
    } else {
        McpStatusSummary::Unknown
    };

    Ok(InlineStatusDetails {
        workspace_trust,
        tool_status,
        language_summary,
        mcp_status,
    })
}

pub(crate) fn build_inline_header_context(
    config: &CoreAgentConfig,
    session_bootstrap: &SessionBootstrap,
    mode_label: String,
    reasoning_label: String,
) -> Result<InlineHeaderContext> {
    let InlineStatusDetails {
        workspace_trust,
        tool_status,
        language_summary,
        mcp_status,
    } = gather_inline_status_details(config, session_bootstrap)?;

    let version = env!("CARGO_PKG_VERSION").to_string();
    let trimmed_mode = mode_label.trim();
    let mode = if trimmed_mode.is_empty() {
        ui::HEADER_MODE_INLINE.to_string()
    } else {
        trimmed_mode.to_string()
    };

    let reasoning = if reasoning_label.trim().is_empty() {
        format!(
            "{}{}",
            ui::HEADER_REASONING_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        )
    } else {
        format!("{}{}", ui::HEADER_REASONING_PREFIX, reasoning_label.trim())
    };

    let trust_value = match workspace_trust {
        Some(level) => format!("{}{}", ui::HEADER_TRUST_PREFIX, level),
        None => format!(
            "{}{}",
            ui::HEADER_TRUST_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        ),
    };

    let tools_value = match tool_status {
        ToolStatusSummary::Available {
            allow,
            prompt,
            deny,
            ..
        } => format!(
            "{}allow {} · prompt {} · deny {}",
            ui::HEADER_TOOLS_PREFIX,
            allow,
            prompt,
            deny
        ),
        ToolStatusSummary::Unavailable(_) => format!(
            "{}{}",
            ui::HEADER_TOOLS_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        ),
    };

    let languages_value = language_summary
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("{}{}", ui::HEADER_LANGUAGES_PREFIX, value))
        .unwrap_or_else(|| {
            format!(
                "{}{}",
                ui::HEADER_LANGUAGES_PREFIX,
                ui::HEADER_UNKNOWN_PLACEHOLDER
            )
        });

    let mcp_value = match mcp_status {
        McpStatusSummary::Error(message) => {
            format!("{}error - {}", ui::HEADER_MCP_PREFIX, message)
        }
        McpStatusSummary::Enabled {
            active_providers,
            configured,
        } => {
            if !active_providers.is_empty() {
                format!(
                    "{}enabled ({})",
                    ui::HEADER_MCP_PREFIX,
                    active_providers.join(", ")
                )
            } else if configured {
                format!("{}enabled (no providers)", ui::HEADER_MCP_PREFIX)
            } else {
                format!("{}enabled", ui::HEADER_MCP_PREFIX)
            }
        }
        McpStatusSummary::Disabled => format!("{}disabled", ui::HEADER_MCP_PREFIX),
        McpStatusSummary::Unknown => format!(
            "{}{}",
            ui::HEADER_MCP_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        ),
    };

    Ok(InlineHeaderContext {
        version,
        mode,
        reasoning,
        workspace_trust: trust_value,
        tools: tools_value,
        languages: languages_value,
        mcp: mcp_value,
    })
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

    let InlineStatusDetails {
        workspace_trust,
        tool_status,
        language_summary,
        mcp_status,
    } = gather_inline_status_details(config, session_bootstrap)?;

    let trust_summary = workspace_trust
        .map(|level| format!("Trust: {}", level))
        .unwrap_or_else(|| "Trust: unavailable".to_string());
    status_lines.push(trust_summary);

    match tool_status {
        ToolStatusSummary::Available {
            allow,
            prompt,
            deny,
            policy_path,
        } => {
            status_lines.push(format!(
                "Tools policy: allow {} · prompt {} · deny {} ({})",
                allow, prompt, deny, policy_path
            ));
        }
        ToolStatusSummary::Unavailable(error) => {
            status_lines.push(format!("Tools policy: unavailable ({})", error));
        }
    }

    if let Some(summary) = language_summary {
        status_lines.push(format!("Stack: {}", summary));
    }

    match mcp_status {
        McpStatusSummary::Error(message) => {
            status_lines.push(format!("MCP: error - {}", message));
        }
        McpStatusSummary::Enabled {
            active_providers,
            configured,
        } => {
            if !active_providers.is_empty() {
                status_lines.push(format!("MCP: enabled ({})", active_providers.join(", ")));
            } else if configured {
                status_lines.push("MCP: enabled (no providers)".to_string());
            } else {
                status_lines.push("MCP: enabled".to_string());
            }
        }
        McpStatusSummary::Disabled => {
            status_lines.push("MCP: disabled".to_string());
        }
        McpStatusSummary::Unknown => {}
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
