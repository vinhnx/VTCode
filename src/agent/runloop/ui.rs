use anstyle::{Color as AnsiColor, RgbColor as AnsiRgbColor, Style as AnsiStyle};
use anyhow::{Context, Result};
use pathdiff::diff_paths;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    prelude::Alignment,
    text::Line,
    widgets::{Block, BorderType, Borders, Padding, Paragraph, Widget},
};
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::tool_policy::{ToolPolicy, ToolPolicyManager};
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::AnsiRenderer;

use ratatui::style::{Color as TuiColor, Modifier as TuiModifier, Style as TuiStyle};
use ratatui::text::Span;

use super::welcome::SessionBootstrap;
use crate::workspace_trust;

#[derive(Clone, Copy)]
enum BannerLine {
    Title,
}

impl BannerLine {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Title => "> VT Code",
        }
    }
}

pub(crate) fn render_session_banner(
    renderer: &mut AnsiRenderer,
    config: &CoreAgentConfig,
    session_bootstrap: &SessionBootstrap,
) -> Result<()> {
    let trust_summary = workspace_trust::workspace_trust_level(&config.workspace)
        .context("Failed to determine workspace trust level for banner")?
        .map(|level| level.to_string())
        .unwrap_or_else(|| "unavailable".to_string());

    let mut entries = Vec::new();
    entries.push(PanelEntry::normal("Workspace", trust_summary));

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
            entries.push(PanelEntry::normal(
                "Tools",
                format!("A{allow} · P{prompt} · D{deny} ({policy_path})"),
            ));
        }
        Err(err) => {
            entries.push(PanelEntry::alert("Tools", format!("unavailable ({err})")));
        }
    }

    if let Some(summary) = session_bootstrap.language_summary.as_deref() {
        entries.push(PanelEntry::normal("Languages", summary.to_string()));
    }

    if let Some(hitl_enabled) = session_bootstrap.human_in_the_loop {
        let status = if hitl_enabled { "enabled" } else { "disabled" };
        entries.push(PanelEntry::normal("Safeguards", format!("HITL {status}")));
    }

    if let Some(ref mcp_error) = session_bootstrap.mcp_error {
        entries.push(PanelEntry::alert("MCP", format!("ERROR - {mcp_error}")));
    } else if let Some(mcp_enabled) = session_bootstrap.mcp_enabled {
        if mcp_enabled && session_bootstrap.mcp_providers.is_some() {
            let providers = session_bootstrap.mcp_providers.as_ref().unwrap();
            let enabled_providers: Vec<&str> = providers
                .iter()
                .filter(|p| p.enabled)
                .map(|p| p.name.as_str())
                .collect();
            let summary = if enabled_providers.is_empty() {
                "enabled (no providers)".to_string()
            } else {
                format!("enabled ({})", enabled_providers.join(", "))
            };
            entries.push(PanelEntry::normal("MCP", summary));
        } else {
            let status = if mcp_enabled { "enabled" } else { "disabled" };
            entries.push(PanelEntry::normal("MCP", status.to_string()));
        }
    }

    let panel = build_session_panel(&entries);
    let banner_style = theme::banner_style();
    let error_style = theme::active_styles().error;
    let highlight_style = logo_banner_style();

    for (index, line) in panel.lines.iter().enumerate() {
        let style = if panel.alert_indices.contains(&index) {
            error_style
        } else {
            banner_style
        };
        let final_style = if index == 0 { highlight_style } else { style };
        renderer.line_with_style(final_style, line)?;
    }

    renderer.line_with_style(banner_style, "")?;

    Ok(())
}

const PANEL_MIN_WIDTH: u16 = 48;
const PANEL_HORIZONTAL_PADDING: u16 = 1;

#[derive(Clone)]
struct PanelEntry {
    label: &'static str,
    value: String,
    severity: EntrySeverity,
}

impl PanelEntry {
    fn normal(label: &'static str, value: String) -> Self {
        Self {
            label,
            value,
            severity: EntrySeverity::Normal,
        }
    }

    fn alert(label: &'static str, value: String) -> Self {
        Self {
            label,
            value,
            severity: EntrySeverity::Alert,
        }
    }
}

#[derive(Clone, Copy)]
enum EntrySeverity {
    Normal,
    Alert,
}

#[derive(Default)]
struct PanelRender {
    lines: Vec<String>,
    alert_indices: std::collections::HashSet<usize>,
}

fn build_session_panel(entries: &[PanelEntry]) -> PanelRender {
    let label_width = entries
        .iter()
        .map(|entry| entry.label.chars().count())
        .max()
        .unwrap_or(0);

    let mut lines = Vec::new();
    let mut content_width = 0usize;

    for entry in entries {
        let formatted_label = if label_width == 0 {
            entry.label.to_string()
        } else {
            format!("{:<width$}", entry.label, width = label_width)
        };
        let text = format!("{formatted_label}  {}", entry.value);
        let line = Line::from(text.clone());
        content_width = content_width.max(line.width());
        lines.push(Line::from(text));
    }

    if lines.is_empty() {
        let line = Line::from(String::new());
        content_width = content_width.max(line.width());
        lines.push(line);
    }

    let horizontal_padding = (PANEL_HORIZONTAL_PADDING * 2) as usize;
    let width = (content_width + horizontal_padding + 2)
        .clamp(PANEL_MIN_WIDTH as usize, u16::MAX as usize) as u16;
    let height = (lines.len() + 2).clamp(3, u16::MAX as usize) as u16;

    let accent_color = theme::banner_color();
    let accent_style = TuiStyle::default().fg(tui_color(accent_color));
    let title_style = TuiStyle::default()
        .fg(tui_color(theme::logo_accent_color()))
        .add_modifier(TuiModifier::BOLD);

    let paragraph = Paragraph::new(lines).alignment(Alignment::Left).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(accent_style)
            .title(Span::styled(BannerLine::Title.as_str(), title_style))
            .title_alignment(Alignment::Center)
            .padding(Padding::horizontal(PANEL_HORIZONTAL_PADDING)),
    );

    let area = Rect::new(0, 0, width, height);
    let mut buffer = Buffer::empty(area);
    paragraph.render(area, &mut buffer);

    let panel_lines = buffer_to_lines(&buffer);
    let mut alert_indices = std::collections::HashSet::new();

    for (idx, entry) in entries.iter().enumerate() {
        if matches!(entry.severity, EntrySeverity::Alert) {
            alert_indices.insert(idx + 1);
        }
    }

    PanelRender {
        lines: panel_lines,
        alert_indices,
    }
}

fn buffer_to_lines(buffer: &Buffer) -> Vec<String> {
    let area = buffer.area();
    let mut lines = Vec::new();
    for y in 0..area.height {
        let mut line = String::new();
        for x in 0..area.width {
            let cell = &buffer[(x, y)];
            line.push_str(cell.symbol());
        }
        lines.push(line.trim_end().to_string());
    }
    lines
}

fn tui_color(rgb: AnsiRgbColor) -> TuiColor {
    TuiColor::Rgb(rgb.0, rgb.1, rgb.2)
}

fn logo_banner_style() -> AnsiStyle {
    let accent = theme::logo_accent_color();
    AnsiStyle::new()
        .fg_color(Some(AnsiColor::Rgb(accent)))
        .bold()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_panel_renders_logo_header() {
        let entries = vec![PanelEntry::normal("Workspace", "full auto".to_string())];
        let panel = build_session_panel(&entries);
        let header = panel.lines.first().expect("header line");
        assert!(header.contains(BannerLine::Title.as_str()));
        assert!(!panel.lines.is_empty());
    }
}
