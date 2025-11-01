use std::io::{self, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anstyle::{Ansi256Color, Color, Effects, Style};
use anyhow::{Context, Result};
use tracing::warn;
use vtcode_core::utils::dot_config::{
    WorkspaceTrustLevel, WorkspaceTrustRecord, get_dot_manager, load_user_config,
};

const WARNING_RGB: (u8, u8, u8) = (166, 51, 51);
const INFO_RGB: (u8, u8, u8) = (217, 154, 78);

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceTrustGateResult {
    Trusted(WorkspaceTrustLevel),
    Aborted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrustSelection {
    FullAuto,
    ToolsPolicy,
    Quit,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceTrustSyncOutcome {
    AlreadyMatches(WorkspaceTrustLevel),
    Upgraded {
        previous: Option<WorkspaceTrustLevel>,
        new: WorkspaceTrustLevel,
    },
    SkippedDowngrade(WorkspaceTrustLevel),
}

#[allow(dead_code)]
pub async fn ensure_workspace_trust(
    workspace: &Path,
    full_auto_requested: bool,
) -> Result<WorkspaceTrustGateResult> {
    let workspace_key = canonicalize_workspace(workspace)?;
    let config = load_user_config()
        .await
        .context("Failed to load user configuration for trust check")?;
    let current_level = config
        .workspace_trust
        .entries
        .get(&workspace_key)
        .map(|record| record.level);

    if let Some(level) = current_level
        && (!full_auto_requested || level == WorkspaceTrustLevel::FullAuto)
    {
        return Ok(WorkspaceTrustGateResult::Trusted(level));
    }

    let require_full_auto_upgrade = full_auto_requested && current_level.is_some();
    render_prompt(workspace, require_full_auto_upgrade);

    match read_user_selection()? {
        TrustSelection::FullAuto => {
            persist_trust_decision(&workspace_key, WorkspaceTrustLevel::FullAuto).await?;
            let msg = "Workspace marked as trusted with full auto capabilities.";
            println!(
                "{}",
                Style::new()
                    .fg_color(Some(Color::Ansi(anstyle::AnsiColor::Green)))
                    .render()
                    .to_string()
                    + msg
                    + &Style::new().render_reset().to_string()
            );
            Ok(WorkspaceTrustGateResult::Trusted(
                WorkspaceTrustLevel::FullAuto,
            ))
        }
        TrustSelection::ToolsPolicy => {
            persist_trust_decision(&workspace_key, WorkspaceTrustLevel::ToolsPolicy).await?;
            let msg = "Workspace marked as trusted with tools policy safeguards.";
            println!(
                "{}",
                Style::new()
                    .fg_color(Some(Color::Ansi(anstyle::AnsiColor::Green)))
                    .render()
                    .to_string()
                    + msg
                    + &Style::new().render_reset().to_string()
            );
            if full_auto_requested {
                let msg1 = "Full-auto mode requires the full auto trust option.";
                println!(
                    "{}",
                    Style::new()
                        .fg_color(Some(Color::Ansi(anstyle::AnsiColor::Yellow)))
                        .render()
                        .to_string()
                        + msg1
                        + &Style::new().render_reset().to_string()
                );
                let msg2 =
                    "Rerun with --full-auto after upgrading trust or start without --full-auto.";
                println!(
                    "{}",
                    Style::new()
                        .fg_color(Some(Color::Ansi(anstyle::AnsiColor::Yellow)))
                        .render()
                        .to_string()
                        + msg2
                        + &Style::new().render_reset().to_string()
                );
                return Ok(WorkspaceTrustGateResult::Aborted);
            }
            Ok(WorkspaceTrustGateResult::Trusted(
                WorkspaceTrustLevel::ToolsPolicy,
            ))
        }
        TrustSelection::Quit => {
            let msg = "Workspace not trusted. Exiting chat session.";
            println!(
                "{}",
                Style::new()
                    .fg_color(Some(Color::Ansi(anstyle::AnsiColor::Yellow)))
                    .render()
                    .to_string()
                    + msg
                    + &Style::new().render_reset().to_string()
            );
            Ok(WorkspaceTrustGateResult::Aborted)
        }
    }
}

fn render_prompt(workspace: &Path, require_full_auto_upgrade: bool) {
    println!();
    print_prompt_line("⚠ Workspace Trust Required", PromptTone::Heading);
    println!();
    print_prompt_line(
        "VT Code can execute code and access files in your workspace.",
        PromptTone::Body,
    );
    print_prompt_line(
        "Trusting this workspace also trusts all MCP servers configured here.",
        PromptTone::Body,
    );
    println!();
    print_prompt_line(
        "Do you want to mark this workspace as trusted?",
        PromptTone::Body,
    );
    println!();

    let workspace_display = workspace.display();
    print_prompt_line(
        &format!("Workspace: {}", workspace_display),
        PromptTone::Body,
    );
    println!();

    let requirement_line = if require_full_auto_upgrade {
        "Full-auto mode requires upgrading this workspace to full auto."
    } else {
        "Select a trust level for this workspace:"
    };
    print_prompt_line(requirement_line, PromptTone::Body);
    println!();

    print_prompt_line("  [a] Trust with full auto capabilities", PromptTone::Body);
    print_prompt_line(
        "  [w] Trust with tools policy safeguards (recommended)",
        PromptTone::Body,
    );
    print_prompt_line("  [q] Quit without trusting", PromptTone::Body);
    println!();

    print_prompt_line("Enter your choice (a/w/q): ", PromptTone::Heading);
    io::stdout().flush().ok();
}

fn read_user_selection() -> Result<TrustSelection> {
    loop {
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .context("Failed to read workspace trust selection")?;
        match input.trim().to_lowercase().as_str() {
            "a" => return Ok(TrustSelection::FullAuto),
            "w" => return Ok(TrustSelection::ToolsPolicy),
            "q" => return Ok(TrustSelection::Quit),
            _ => {
                print_prompt_line(
                    "Invalid selection. Please enter 'a', 'w', or 'q'.",
                    PromptTone::Heading,
                );
            }
        }
    }
}

#[allow(dead_code)]
pub async fn workspace_trust_level(workspace: &Path) -> Result<Option<WorkspaceTrustLevel>> {
    let workspace_key = canonicalize_workspace(workspace)?;
    let config = load_user_config()
        .await
        .context("Failed to load user configuration for trust lookup")?;
    Ok(config
        .workspace_trust
        .entries
        .get(&workspace_key)
        .map(|record| record.level))
}

#[allow(dead_code)]
pub async fn ensure_workspace_trust_level_silent(
    workspace: &Path,
    desired_level: WorkspaceTrustLevel,
) -> Result<WorkspaceTrustSyncOutcome> {
    let workspace_key = canonicalize_workspace(workspace)?;
    let config = load_user_config()
        .await
        .context("Failed to load user configuration for trust sync")?;
    let current_level = config
        .workspace_trust
        .entries
        .get(&workspace_key)
        .map(|record| record.level);

    if let Some(level) = current_level {
        if level == desired_level {
            return Ok(WorkspaceTrustSyncOutcome::AlreadyMatches(level));
        }

        if level == WorkspaceTrustLevel::FullAuto
            && desired_level == WorkspaceTrustLevel::ToolsPolicy
        {
            return Ok(WorkspaceTrustSyncOutcome::SkippedDowngrade(level));
        }
    }

    persist_trust_decision(&workspace_key, desired_level).await?;

    Ok(WorkspaceTrustSyncOutcome::Upgraded {
        previous: current_level,
        new: desired_level,
    })
}

async fn persist_trust_decision(workspace_key: &str, level: WorkspaceTrustLevel) -> Result<()> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let manager = get_dot_manager().lock().unwrap().clone();

    manager
        .update_config(|cfg| {
            cfg.workspace_trust.entries.insert(
                workspace_key.to_string(),
                WorkspaceTrustRecord {
                    level,
                    trusted_at: timestamp,
                },
            );
        })
        .await
        .context("Failed to persist workspace trust decision")
}

fn canonicalize_workspace(workspace: &Path) -> Result<String> {
    match workspace.canonicalize() {
        Ok(canonical) => Ok(canonical.to_string_lossy().into_owned()),
        Err(error) => {
            warn!(
                workspace = %workspace.display(),
                error = ?error,
                "Failed to canonicalize workspace path; using provided path as workspace key"
            );
            Ok(workspace.to_string_lossy().into_owned())
        }
    }
}

enum PromptTone {
    Heading,
    Body,
}

fn print_prompt_line(message: &str, tone: PromptTone) {
    let style = match tone {
        PromptTone::Heading => Style::new()
            .fg_color(Some(Color::Ansi256(Ansi256Color(rgb_to_ansi256(
                WARNING_RGB.0,
                WARNING_RGB.1,
                WARNING_RGB.2,
            )))))
            .effects(Effects::BOLD),
        PromptTone::Body => Style::new().fg_color(Some(Color::Ansi256(Ansi256Color(
            rgb_to_ansi256(INFO_RGB.0, INFO_RGB.1, INFO_RGB.2),
        )))),
    };
    println!("{}{}{}", style.render(), message, style.render_reset());
}

fn rgb_to_ansi256(r: u8, g: u8, b: u8) -> u8 {
    if r == g && g == b {
        if r < 8 {
            return 16;
        }
        if r > 248 {
            return 231;
        }
        return ((r as u16 - 8) / 10) as u8 + 232;
    }

    let r_index = ((r as u16 * 5) / 255) as u8;
    let g_index = ((g as u16 * 5) / 255) as u8;
    let b_index = ((b as u16 * 5) / 255) as u8;

    16 + 36 * r_index + 6 * g_index + b_index
}
