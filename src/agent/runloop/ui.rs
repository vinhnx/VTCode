use anyhow::Result;
use pathdiff::diff_paths;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::tool_policy::{ToolPolicy, ToolPolicyManager};
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::AnsiRenderer;

use super::welcome::SessionBootstrap;

pub(crate) fn render_session_banner(
    renderer: &mut AnsiRenderer,
    config: &CoreAgentConfig,
    session_bootstrap: &SessionBootstrap,
) -> Result<()> {
    let banner_style = theme::banner_style();
    renderer.line_with_style(banner_style, "Welcome to VTCode!")?;

    let mut bullets = Vec::new();
    bullets.push(format!("- Model: {}", config.model));
    bullets.push(format!("- Workspace: {}", config.workspace.display()));

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
                "- Tool policy: allow {} · prompt {} · deny {} ({})",
                allow, prompt, deny, policy_path
            ));
        }
        Err(err) => {
            bullets.push(format!("- Tool policy: unavailable ({})", err));
        }
    }

    if let Some(summary) = session_bootstrap.language_summary.as_deref() {
        bullets.push(format!("- Languages: {}", summary));
    }

    for line in bullets {
        renderer.line_with_style(banner_style, &line)?;
    }

    renderer.line_with_style(banner_style, "")?;

    Ok(())
}
