use std::io::{self, Write};
use std::path::Path;

use anstyle::Ansi256Color;
use anyhow::{Context, Result, bail};
use vtcode_commons::color256_theme::rgb_to_ansi256_for_theme;
use vtcode_core::utils::dot_config::{
    WorkspaceTrustLevel, load_workspace_trust_level, update_workspace_trust,
};
use vtcode_core::utils::style_helpers::{ColorPalette, render_styled};
use vtcode_core::utils::{ansi_capabilities, ansi_capabilities::ColorScheme};

const WARNING_RGB: (u8, u8, u8) = (166, 51, 51);
const INFO_RGB: (u8, u8, u8) = (217, 154, 78);
const CHOICE_PROMPT: &str = "Enter your choice (a/q): ";
const INVALID_CHOICE_MESSAGE: &str = "Invalid selection. Please enter 'a' or 'q'.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrustSelection {
    FullAuto,
    Quit,
}

pub(crate) async fn ensure_full_auto_workspace_trust(workspace: &Path) -> Result<bool> {
    let current_level = load_workspace_trust_level(workspace)
        .await
        .context("Failed to determine workspace trust level for benchmark command")?;

    if current_level == Some(WorkspaceTrustLevel::FullAuto) {
        return Ok(true);
    }

    let require_full_auto_upgrade = current_level.is_some();
    let palette = ColorPalette::default();
    render_prompt(workspace, require_full_auto_upgrade);

    match read_user_selection()? {
        TrustSelection::FullAuto => {
            update_workspace_trust(workspace, WorkspaceTrustLevel::FullAuto)
                .await
                .context("Failed to persist full-auto workspace trust")?;
            let msg = "Workspace marked as trusted with full auto capabilities.";
            println!("{}", render_styled(msg, palette.success, None));
            Ok(true)
        }
        TrustSelection::Quit => {
            let msg = "Workspace not trusted. Exiting benchmark command.";
            println!("{}", render_styled(msg, palette.warning, None));
            Ok(false)
        }
    }
}

pub(crate) async fn require_full_auto_workspace_trust(
    workspace: &Path,
    denied_action: &str,
    command_name: &str,
) -> Result<()> {
    let trust_level = load_workspace_trust_level(workspace)
        .await
        .context("Failed to determine workspace trust level")?;

    match trust_level {
        Some(WorkspaceTrustLevel::FullAuto) => Ok(()),
        Some(level) => bail!(
            "Workspace trust level '{level}' does not permit {denied_action}. Upgrade trust to full auto."
        ),
        None => bail!(
            "Workspace is not trusted. Start VT Code interactively once and mark it as full auto before using {command_name}."
        ),
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
        "VT Code benchmark execution requires full auto workspace trust.",
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
        "Trust this workspace with full auto capabilities to continue:"
    };
    print_prompt_line(requirement_line, PromptTone::Body);
    println!();

    print_prompt_line("  [a] Trust with full auto capabilities", PromptTone::Body);
    print_prompt_line("  [q] Quit without trusting", PromptTone::Body);
    println!();
}

fn read_user_selection() -> Result<TrustSelection> {
    loop {
        print_prompt(CHOICE_PROMPT, PromptTone::Heading)?;
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .context("Failed to read workspace trust selection")?;

        if let Some(selection) = parse_trust_selection(input.trim()) {
            return Ok(selection);
        }

        print_prompt_line(INVALID_CHOICE_MESSAGE, PromptTone::Heading);
    }
}

fn parse_trust_selection(input: &str) -> Option<TrustSelection> {
    if input.eq_ignore_ascii_case("a") {
        Some(TrustSelection::FullAuto)
    } else if input.eq_ignore_ascii_case("q") {
        Some(TrustSelection::Quit)
    } else {
        None
    }
}

enum PromptTone {
    Heading,
    Body,
}

fn print_prompt(message: &str, tone: PromptTone) -> Result<()> {
    print!("{}", styled_prompt_message(message, tone));
    io::stdout()
        .flush()
        .context("Failed to flush workspace trust prompt")
}

fn print_prompt_line(message: &str, tone: PromptTone) {
    println!("{}", styled_prompt_message(message, tone));
}

fn styled_prompt_message(message: &str, tone: PromptTone) -> String {
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
    if is_heading {
        render_styled(message, color, Some("bold".to_string()))
    } else {
        render_styled(message, color, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_trust_selection_accepts_expected_choices() {
        assert_eq!(parse_trust_selection("a"), Some(TrustSelection::FullAuto));
        assert_eq!(parse_trust_selection("q"), Some(TrustSelection::Quit));
    }

    #[test]
    fn parse_trust_selection_rejects_unknown_choices() {
        assert_eq!(parse_trust_selection(""), None);
        assert_eq!(parse_trust_selection("w"), None);
    }
}
