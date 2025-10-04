use std::io::{self, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use console::{Color, style};
use vtcode_core::utils::dot_config::{
    WorkspaceTrustLevel, WorkspaceTrustRecord, get_dot_manager, load_user_config,
};

const WARNING_RGB: (u8, u8, u8) = (166, 51, 51);
const INFO_RGB: (u8, u8, u8) = (217, 154, 78);

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

/// Result of synchronizing workspace trust without interactive prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceTrustSyncOutcome {
    /// The requested level was already stored.
    AlreadyMatches(WorkspaceTrustLevel),
    /// The workspace trust level was updated (optionally upgrading from a previous value).
    Upgraded {
        previous: Option<WorkspaceTrustLevel>,
        new: WorkspaceTrustLevel,
    },
    /// A downgrade request was ignored because the workspace is already fully trusted.
    SkippedDowngrade(WorkspaceTrustLevel),
}

pub fn ensure_workspace_trust(
    workspace: &Path,
    full_auto_requested: bool,
) -> Result<WorkspaceTrustGateResult> {
    let workspace_key = canonicalize_workspace(workspace)?;
    let config = load_user_config().context("Failed to load user configuration for trust check")?;
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
            persist_trust_decision(&workspace_key, WorkspaceTrustLevel::FullAuto)?;
            println!(
                "{}",
                style("Workspace marked as trusted with full auto capabilities.").green()
            );
            Ok(WorkspaceTrustGateResult::Trusted(
                WorkspaceTrustLevel::FullAuto,
            ))
        }
        TrustSelection::ToolsPolicy => {
            persist_trust_decision(&workspace_key, WorkspaceTrustLevel::ToolsPolicy)?;
            println!(
                "{}",
                style("Workspace marked as trusted with tools policy safeguards.").green()
            );
            if full_auto_requested {
                println!(
                    "{}",
                    style("Full-auto mode requires the full auto trust option.").yellow()
                );
                println!(
                    "{}",
                    style(
                        "Rerun with --full-auto after upgrading trust or start without --full-auto."
                    )
                    .yellow()
                );
                return Ok(WorkspaceTrustGateResult::Aborted);
            }
            Ok(WorkspaceTrustGateResult::Trusted(
                WorkspaceTrustLevel::ToolsPolicy,
            ))
        }
        TrustSelection::Quit => {
            println!(
                "{}",
                style("Workspace not trusted. Exiting chat session.").yellow()
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
    print_prompt_line(&workspace.display().to_string(), PromptTone::Body);
    println!();
    if require_full_auto_upgrade {
        print_prompt_line(
            "Full-auto mode requested. Choose full auto trust to continue this session.",
            PromptTone::Body,
        );
        println!();
    }
    print_prompt_line(
        "▶ [a] Trust this workspace with full auto",
        PromptTone::Body,
    );
    print_prompt_line(
        "[w] Trust this workspace with tools policy",
        PromptTone::Body,
    );
    print_prompt_line("[q] Quit", PromptTone::Body);
    println!();
    print_prompt_line(
        "Press a, w, or q then Enter to choose an option.",
        PromptTone::Body,
    );
    println!();
}

fn read_user_selection() -> Result<TrustSelection> {
    loop {
        let mut input = String::new();
        print!("Selection [a/w/q]: ");
        io::stdout()
            .flush()
            .context("Failed to flush stdout for trust prompt")?;
        let bytes_read = io::stdin()
            .read_line(&mut input)
            .context("Failed to read user selection for trust prompt")?;
        if bytes_read == 0 {
            print_prompt_line(
                "No selection received (EOF). Aborting workspace trust flow.",
                PromptTone::Heading,
            );
            return Ok(TrustSelection::Quit);
        }
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

pub fn workspace_trust_level(workspace: &Path) -> Result<Option<WorkspaceTrustLevel>> {
    let workspace_key = canonicalize_workspace(workspace)?;
    let config =
        load_user_config().context("Failed to load user configuration for trust lookup")?;
    Ok(config
        .workspace_trust
        .entries
        .get(&workspace_key)
        .map(|record| record.level))
}

/// Ensure the workspace trust level matches the desired configuration without prompting.
pub fn ensure_workspace_trust_level_silent(
    workspace: &Path,
    desired_level: WorkspaceTrustLevel,
) -> Result<WorkspaceTrustSyncOutcome> {
    let workspace_key = canonicalize_workspace(workspace)?;
    let config = load_user_config().context("Failed to load user configuration for trust sync")?;
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

    persist_trust_decision(&workspace_key, desired_level)?;

    Ok(WorkspaceTrustSyncOutcome::Upgraded {
        previous: current_level,
        new: desired_level,
    })
}

fn persist_trust_decision(workspace_key: &str, level: WorkspaceTrustLevel) -> Result<()> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let manager = get_dot_manager();
    let guard = manager
        .lock()
        .expect("Workspace trust manager mutex poisoned");
    guard
        .update_config(|cfg| {
            cfg.workspace_trust.entries.insert(
                workspace_key.to_string(),
                WorkspaceTrustRecord {
                    level,
                    trusted_at: timestamp,
                },
            );
        })
        .context("Failed to persist workspace trust decision")
}

fn canonicalize_workspace(workspace: &Path) -> Result<String> {
    let canonical = workspace.canonicalize().with_context(|| {
        format!(
            "Failed to canonicalize workspace path {} for trust evaluation",
            workspace.display()
        )
    })?;
    Ok(canonical.to_string_lossy().into_owned())
}

enum PromptTone {
    Heading,
    Body,
}

fn print_prompt_line(message: &str, tone: PromptTone) {
    let styled = match tone {
        PromptTone::Heading => style(message.to_owned())
            .fg(Color::Color256(rgb_to_ansi256(
                WARNING_RGB.0,
                WARNING_RGB.1,
                WARNING_RGB.2,
            )))
            .bold(),
        PromptTone::Body => style(message.to_owned()).fg(Color::Color256(rgb_to_ansi256(
            INFO_RGB.0, INFO_RGB.1, INFO_RGB.2,
        ))),
    };
    println!("{}", styled);
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
