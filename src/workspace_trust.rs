use std::io::{self, Write};
use std::path::Path;

use anstyle::Ansi256Color;
use anyhow::{Context, Result};
use vtcode_commons::color256_theme::rgb_to_ansi256_for_theme;
use vtcode_core::utils::dot_config::{
    WorkspaceTrustLevel, load_workspace_trust_level, update_workspace_trust,
};
use vtcode_core::utils::style_helpers::{ColorPalette, render_styled};
use vtcode_core::utils::{ansi_capabilities, ansi_capabilities::ColorScheme};

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

#[allow(dead_code)] // Used by the library ACP workspace synchronizer path, not the bin target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceTrustSyncOutcome {
    AlreadyMatches(WorkspaceTrustLevel),
    Upgraded {
        previous: Option<WorkspaceTrustLevel>,
        new: WorkspaceTrustLevel,
    },
    SkippedDowngrade(WorkspaceTrustLevel),
}

pub async fn ensure_workspace_trust(
    workspace: &Path,
    full_auto_requested: bool,
) -> Result<WorkspaceTrustGateResult> {
    let current_level = workspace_trust_level(workspace).await?;

    if let Some(level) = current_level
        && (!full_auto_requested || level == WorkspaceTrustLevel::FullAuto)
    {
        return Ok(WorkspaceTrustGateResult::Trusted(level));
    }

    let require_full_auto_upgrade = full_auto_requested && current_level.is_some();
    render_prompt(workspace, require_full_auto_upgrade);

    match read_user_selection()? {
        TrustSelection::FullAuto => {
            update_workspace_trust(workspace, WorkspaceTrustLevel::FullAuto)
                .await
                .context("Failed to persist full-auto workspace trust")?;
            let msg = "Workspace marked as trusted with full auto capabilities.";
            let palette = ColorPalette::default();
            println!("{}", render_styled(msg, palette.success, None));
            Ok(WorkspaceTrustGateResult::Trusted(
                WorkspaceTrustLevel::FullAuto,
            ))
        }
        TrustSelection::ToolsPolicy => {
            update_workspace_trust(workspace, WorkspaceTrustLevel::ToolsPolicy)
                .await
                .context("Failed to persist tools-policy workspace trust")?;
            let msg = "Workspace marked as trusted with tools policy safeguards.";
            let palette = ColorPalette::default();
            println!("{}", render_styled(msg, palette.success, None));
            if full_auto_requested {
                let msg1 = "Full-auto mode requires the full auto trust option.";
                println!("{}", render_styled(msg1, palette.warning, None));
                let msg2 =
                    "Rerun with --full-auto after upgrading trust or start without --full-auto.";
                println!("{}", render_styled(msg2, palette.warning, None));
                return Ok(WorkspaceTrustGateResult::Aborted);
            }
            Ok(WorkspaceTrustGateResult::Trusted(
                WorkspaceTrustLevel::ToolsPolicy,
            ))
        }
        TrustSelection::Quit => {
            let msg = "Workspace not trusted. Exiting chat session.";
            let palette = ColorPalette::default();
            println!("{}", render_styled(msg, palette.warning, None));
            Ok(WorkspaceTrustGateResult::Aborted)
        }
    }
}

fn render_prompt(workspace: &Path, require_full_auto_upgrade: bool) {
    println!();
    print_prompt_line("[WARN] Workspace Trust Required", PromptTone::Heading);
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

pub async fn workspace_trust_level(workspace: &Path) -> Result<Option<WorkspaceTrustLevel>> {
    load_workspace_trust_level(workspace)
        .await
        .context("Failed to load user configuration for trust lookup")
}

#[allow(dead_code)] // Used by the library ACP workspace synchronizer path, not the bin target.
pub async fn ensure_workspace_trust_level_silent(
    workspace: &Path,
    desired_level: WorkspaceTrustLevel,
) -> Result<WorkspaceTrustSyncOutcome> {
    let current_level = load_workspace_trust_level(workspace)
        .await
        .context("Failed to load user configuration for trust sync")?;

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

    update_workspace_trust(workspace, desired_level)
        .await
        .context("Failed to persist workspace trust sync")?;

    Ok(WorkspaceTrustSyncOutcome::Upgraded {
        previous: current_level,
        new: desired_level,
    })
}

enum PromptTone {
    Heading,
    Body,
}

fn print_prompt_line(message: &str, tone: PromptTone) {
    use anstyle::Color;

    let is_light_theme = matches!(ansi_capabilities::detect_color_scheme(), ColorScheme::Light);
    let (rgb, is_heading) = match tone {
        PromptTone::Heading => (WARNING_RGB, true),
        PromptTone::Body => (INFO_RGB, false),
    };

    let color = Color::Ansi256(Ansi256Color(rgb_to_ansi256_for_theme(
        rgb.0,
        rgb.1,
        rgb.2,
        is_light_theme,
    )));
    let styled = if is_heading {
        render_styled(message, color, Some("bold".to_string()))
    } else {
        render_styled(message, color, None)
    };
    println!("{}", styled);
}
