use anyhow::{Context, Result};
use pathdiff::diff_paths;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::tool_policy::{ToolPolicy, ToolPolicyManager};
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::AnsiRenderer;

use super::welcome::SessionBootstrap;
use crate::workspace_trust;

/// Build the VT Code banner using Ratatui's logo example style.
fn vtcode_ratatui_logo() -> Vec<String> {
    const LOGO: [[&str; 3]; 7] = [
        ["█   █", "█   █", " ▀▄▀ "], // V
        ["▄▄▄▄▄", "  █  ", "  █  "], // T
        ["     ", "     ", "     "], // space
        [" ▄▄▄ ", "█    ", "█▄▄▄ "], // C
        [" ▄▄▄ ", "█   █", "█▄▄▄█"], // O
        ["▄▄▄  ", "█  █ ", "█▄▄█ "], // D
        ["▄▄▄▄▄", "█▄▄  ", "█▄▄▄▄"], // E
    ];

    let mut rows = vec![String::new(); 3];
    for (index, letter) in LOGO.iter().enumerate() {
        for (row, pattern) in letter.iter().enumerate() {
            if index > 0 {
                rows[row].push(' ');
            }
            rows[row].push_str(pattern);
        }
    }
    rows
}

pub(crate) fn render_session_banner(
    renderer: &mut AnsiRenderer,
    config: &CoreAgentConfig,
    session_bootstrap: &SessionBootstrap,
) -> Result<()> {
    // Render the Ratatui-styled banner
    let banner_lines = vtcode_ratatui_logo();
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
    bullets.push(format!("* Model: {}", config.model));
    bullets.push(format!("* Reasoning effort: {}", config.reasoning_effort));
    if let Some(hitl) = session_bootstrap.human_in_the_loop {
        let status = if hitl { "enabled" } else { "disabled" };
        bullets.push(format!("* Human-in-the-loop: {}", status));
    }

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

    for line in bullets {
        renderer.line_with_style(theme::banner_style(), &line)?;
    }

    renderer.line_with_style(theme::banner_style(), "")?;

    Ok(())
}
