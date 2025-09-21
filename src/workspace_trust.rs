use std::io::{self, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use console::style;
use vtcode_core::utils::dot_config::get_dot_manager;
use vtcode_core::{WorkspaceTrustLevel, WorkspaceTrustRecord, load_user_config};

const PROMPT_BORDER_TOP: &str =
    "╭───────────────────────────────────────────────────────────────────────────────╮";
const PROMPT_BORDER_BOTTOM: &str =
    "╰───────────────────────────────────────────────────────────────────────────────╯";
const PROMPT_EMPTY_LINE: &str =
    "│                                                                               │";
const PROMPT_CONTENT_WIDTH: usize = 79;

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
    println!("{}", PROMPT_BORDER_TOP);
    println!("{}", PROMPT_EMPTY_LINE);
    println!("{}", format_line("⚠ Workspace Trust Required"));
    println!("{}", PROMPT_EMPTY_LINE);
    println!(
        "{}",
        format_line("VT Code can execute code and access files in your workspace."),
    );
    println!(
        "{}",
        format_line("Trusting this workspace also trusts all MCP servers configured here."),
    );
    println!("{}", PROMPT_EMPTY_LINE);
    println!(
        "{}",
        format_line("Do you want to mark this workspace as trusted?"),
    );
    println!("{}", PROMPT_EMPTY_LINE);
    println!("{}", format_line(&format!("{}", workspace.display())),);
    println!("{}", PROMPT_EMPTY_LINE);
    if require_full_auto_upgrade {
        println!(
            "{}",
            format_line(
                "Full-auto mode requested. Choose full auto trust to continue this session.",
            ),
        );
        println!("{}", PROMPT_EMPTY_LINE);
    }
    println!(
        "{}",
        format_line("▶ [a] Trust this workspace with full auto"),
    );
    println!(
        "{}",
        format_line("[w] Trust this workspace with tools policy"),
    );
    println!("{}", format_line("[q] Quit"));
    println!("{}", PROMPT_EMPTY_LINE);
    println!(
        "{}",
        format_line("Press a, w, or q then Enter to choose an option."),
    );
    println!("{}", PROMPT_EMPTY_LINE);
    println!("{}", PROMPT_BORDER_BOTTOM);
}

fn read_user_selection() -> Result<TrustSelection> {
    loop {
        let mut input = String::new();
        print!("Selection [a/w/q]: ");
        io::stdout()
            .flush()
            .context("Failed to flush stdout for trust prompt")?;
        io::stdin()
            .read_line(&mut input)
            .context("Failed to read user selection for trust prompt")?;
        match input.trim().to_lowercase().as_str() {
            "a" => return Ok(TrustSelection::FullAuto),
            "w" => return Ok(TrustSelection::ToolsPolicy),
            "q" => return Ok(TrustSelection::Quit),
            _ => {
                println!(
                    "{}",
                    style("Invalid selection. Please enter 'a', 'w', or 'q'.").red()
                );
            }
        }
    }
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

fn format_line(content: &str) -> String {
    let mut display = String::new();
    if content.chars().count() > PROMPT_CONTENT_WIDTH {
        display = content
            .chars()
            .take(PROMPT_CONTENT_WIDTH.saturating_sub(1))
            .collect();
        display.push('…');
    } else {
        display.push_str(content);
    }
    format!("│ {:<width$} │", display, width = PROMPT_CONTENT_WIDTH)
}
