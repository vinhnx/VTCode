extern crate cfonts;

use anyhow::{Context, Result};
use cfonts::{Fonts, Options, render};
use pathdiff::diff_paths;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::tool_policy::{ToolPolicy, ToolPolicyManager};
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::AnsiRenderer;

use super::welcome::SessionBootstrap;
use crate::workspace_trust;

/// Render a fancy banner using cfonts
fn render_fancy_banner() -> String {
    let output = render(Options {
        text: String::from("VT Code"),
        font: Fonts::FontBlock,
        letter_spacing: 1,
        line_height: 1,
        spaceless: false,
        align: cfonts::Align::Left,
        ..Options::default()
    });
    output.text
}

pub(crate) fn render_session_banner(
    renderer: &mut AnsiRenderer,
    config: &CoreAgentConfig,
    session_bootstrap: &SessionBootstrap,
) -> Result<()> {
    // Render the fancy banner
    let banner_text = render_fancy_banner();
    for line in banner_text.lines() {
        renderer.line_with_style(theme::banner_style(), line)?;
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
