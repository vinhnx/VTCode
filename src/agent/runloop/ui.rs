use anstyle::RgbColor;
use anyhow::{Context, Result};
use cfonts::{Align, BgColors, Colors, Fonts, Options, Rgb, render};
use pathdiff::diff_paths;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::tool_policy::{ToolPolicy, ToolPolicyManager};
use vtcode_core::ui::theme::{self, logo_accent_color};
use vtcode_core::utils::ansi::AnsiRenderer;

use super::welcome::SessionBootstrap;
use crate::workspace_trust;

/// Build the VT Code banner using a cfonts rendered logo that adapts to the active theme.
fn vtcode_inline_logo() -> Vec<String> {
    let accent = logo_accent_color();
    let RgbColor(r, g, b) = accent;
    let rendered = render(Options {
        text: "VT Code".to_string(),
        font: Fonts::FontTiny,
        align: Align::Left,
        colors: vec![Colors::Rgb(Rgb::Val(r, g, b))],
        background: BgColors::Transparent,
        spaceless: true,
        raw_mode: true,
        max_length: 30, // Significantly reduced max length to make it smaller
        ..Options::default()
    });

    rendered
        .vec
        .into_iter()
        .map(|line| line.trim_matches('\n').to_string())
        .filter(|line| !line.trim().is_empty())
        .collect()
}

pub(crate) fn render_session_banner(
    renderer: &mut AnsiRenderer,
    config: &CoreAgentConfig,
    session_bootstrap: &SessionBootstrap,
) -> Result<()> {
    // Render the inline UI banner
    let banner_lines = vtcode_inline_logo();
    for line in &banner_lines {
        renderer.line_with_style(theme::banner_style(), line.as_str())?;
    }

    // Add a separator line
    renderer.line_with_style(theme::banner_style(), "")?;

    let mut bullets = Vec::new();

    let trust_summary = workspace_trust::workspace_trust_level(&config.workspace)
        .context("Failed to determine workspace trust level for banner")?
        .map(|level| format!("* Workspace trust: {}", level))
        .unwrap_or_else(|| "* Workspace trust: unavailable".to_string());
    bullets.push(trust_summary);
    bullets.push(format!(
        "* Model: {} | reasoning effort: {}",
        config.model, config.reasoning_effort
    ));

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
            bullets.push(format!(
                "* Tools policy: Allow {} · Prompt {} · Deny {} ({})",
                allow, prompt, deny, policy_path
            ));
        }
        Err(err) => {
            bullets.push(format!("- Tool policy: unavailable ({})", err));
        }
    }

    if let Some(summary) = session_bootstrap.language_summary.as_deref() {
        bullets.push(format!("* Workspace languages: {}", summary));
    }

    if let Some(hitl_enabled) = session_bootstrap.human_in_the_loop {
        let status = if hitl_enabled { "enabled" } else { "disabled" };
        bullets.push(format!("* Human-in-the-loop safeguards: {}", status));
    }

    // Add MCP status to welcome banner
    if let Some(mcp_enabled) = session_bootstrap.mcp_enabled {
        if mcp_enabled && session_bootstrap.mcp_providers.is_some() {
            let providers = session_bootstrap.mcp_providers.as_ref().unwrap();
            let enabled_providers: Vec<&str> = providers
                .iter()
                .filter(|p| p.enabled)
                .map(|p| p.name.as_str())
                .collect();
            if enabled_providers.is_empty() {
                bullets.push("* MCP (Model Context Protocol): enabled (no providers)".to_string());
            } else {
                bullets.push(format!(
                    "* MCP (Model Context Protocol): enabled ({})",
                    enabled_providers.join(", ")
                ));
            }
        } else {
            let status = if mcp_enabled { "enabled" } else { "disabled" };
            bullets.push(format!("* MCP (Model Context Protocol): {}", status));
        }
    }

    for line in bullets {
        renderer.line_with_style(theme::banner_style(), &line)?;
    }

    renderer.line_with_style(theme::banner_style(), "")?;

    Ok(())
}
