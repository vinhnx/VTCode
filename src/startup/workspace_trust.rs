use std::env;
use std::io::{self, Write};
use std::path::Path;

use anstyle::Ansi256Color;
use anyhow::{Context, Result, bail};
use vtcode_commons::color256_theme::rgb_to_ansi256_for_theme;
use vtcode_core::utils::dot_config::{
    WorkspaceTrustLevel, load_workspace_trust_level, update_workspace_trust,
};
use vtcode_core::utils::style_helpers::{ColorPalette, render_styled};
use vtcode_core::utils::tty::TtyExt;
use vtcode_core::utils::{ansi_capabilities, ansi_capabilities::ColorScheme};

const WARNING_RGB: (u8, u8, u8) = (166, 51, 51);
const INFO_RGB: (u8, u8, u8) = (217, 154, 78);
const CHOICE_PROMPT: &str = "Enter your choice (a/q): ";
const INVALID_CHOICE_MESSAGE: &str = "Invalid selection. Please enter 'a' or 'q'.";

/// Environment variable that allows non-interactive (CI / scripted) callers
/// to grant or deny workspace trust without launching the interactive
/// VT Code shell. Recognised values (case-insensitive):
///
/// * `full-auto`, `full_auto`, `fullauto`, `trust`, `trusted`, `1`, `yes`,
///   `on` — grant full-auto trust for this workspace.
/// * `deny`, `0`, `no`, `off` — explicitly refuse trust; the calling command
///   will abort with a clear error instead of prompting.
pub(crate) const TRUST_OVERRIDE_ENV: &str = "VTCODE_TRUST_WORKSPACE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrustSelection {
    FullAuto,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvTrustOverride {
    FullAuto,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrustOutcome {
    Granted,
    UserDeclined,
    EnvDenied,
    NonInteractive,
}

/// Best-effort path used by `vtcode benchmark`: prompt the user interactively,
/// fall back to env-var overrides, otherwise return `Ok(false)` so the caller
/// can exit gracefully without a hard error.
pub(crate) async fn ensure_full_auto_workspace_trust(workspace: &Path) -> Result<bool> {
    let outcome = resolve_full_auto_workspace_trust(workspace, "benchmark").await?;
    let palette = ColorPalette::default();
    match outcome {
        TrustOutcome::Granted => Ok(true),
        TrustOutcome::UserDeclined => {
            let msg = "Workspace not trusted. Exiting benchmark command.";
            println!("{}", render_styled(msg, palette.warning, None));
            Ok(false)
        }
        TrustOutcome::EnvDenied => {
            let msg = format!(
                "Workspace trust denied via {TRUST_OVERRIDE_ENV}=deny. Exiting benchmark command."
            );
            println!("{}", render_styled(&msg, palette.warning, None));
            Ok(false)
        }
        TrustOutcome::NonInteractive => {
            let msg = format!(
                "Workspace is not trusted and stdin is not a TTY. Re-run benchmark interactively or set {TRUST_OVERRIDE_ENV}=full-auto."
            );
            println!("{}", render_styled(&msg, palette.warning, None));
            Ok(false)
        }
    }
}

/// Strict path used by `vtcode exec` and `--auto/--full-auto`: returns Ok only
/// when trust has been granted (either previously persisted, via env-var, or
/// freshly accepted at an interactive prompt). Any other outcome surfaces an
/// actionable error.
pub(crate) async fn require_full_auto_workspace_trust(
    workspace: &Path,
    denied_action: &str,
    command_name: &str,
) -> Result<()> {
    let outcome = resolve_full_auto_workspace_trust(workspace, command_name).await?;
    match outcome {
        TrustOutcome::Granted => Ok(()),
        TrustOutcome::UserDeclined => bail!(
            "Workspace trust declined. {denied_action} requires full-auto trust before continuing."
        ),
        TrustOutcome::EnvDenied => bail!(
            "Workspace trust denied via {TRUST_OVERRIDE_ENV}=deny; {denied_action} cannot continue. Unset the variable or set it to full-auto."
        ),
        TrustOutcome::NonInteractive => {
            let workspace_display = workspace.display();
            bail!(
                "Workspace '{workspace_display}' is not trusted; {denied_action} requires full-auto workspace trust.\nGrant trust by one of:\n  - Re-run {command_name} from a terminal (TTY) and accept the trust prompt\n  - Set {TRUST_OVERRIDE_ENV}=full-auto and re-run {command_name}\n  - Start VT Code interactively once and mark this workspace as full auto"
            );
        }
    }
}

async fn resolve_full_auto_workspace_trust(
    workspace: &Path,
    command_name: &str,
) -> Result<TrustOutcome> {
    let current_level = load_workspace_trust_level(workspace)
        .await
        .with_context(|| format!("Failed to determine workspace trust level for {command_name}"))?;

    if current_level == Some(WorkspaceTrustLevel::FullAuto) {
        return Ok(TrustOutcome::Granted);
    }

    if let Some(env_override) = parse_env_trust_override()? {
        return match env_override {
            EnvTrustOverride::FullAuto => {
                update_workspace_trust(workspace, WorkspaceTrustLevel::FullAuto)
                    .await
                    .context("Failed to persist full-auto workspace trust")?;
                if !quiet_env_overrides() {
                    let palette = ColorPalette::default();
                    let msg = format!(
                        "Workspace trusted via {TRUST_OVERRIDE_ENV}=full-auto for {command_name}."
                    );
                    println!("{}", render_styled(&msg, palette.success, None));
                }
                Ok(TrustOutcome::Granted)
            }
            EnvTrustOverride::Deny => Ok(TrustOutcome::EnvDenied),
        };
    }

    if !prompt_capable() {
        return Ok(TrustOutcome::NonInteractive);
    }

    let require_full_auto_upgrade = current_level.is_some();
    render_prompt(workspace, require_full_auto_upgrade);

    match read_user_selection()? {
        TrustSelection::FullAuto => {
            update_workspace_trust(workspace, WorkspaceTrustLevel::FullAuto)
                .await
                .context("Failed to persist full-auto workspace trust")?;
            let palette = ColorPalette::default();
            let msg = "Workspace marked as trusted with full auto capabilities.";
            println!("{}", render_styled(msg, palette.success, None));
            Ok(TrustOutcome::Granted)
        }
        TrustSelection::Quit => Ok(TrustOutcome::UserDeclined),
    }
}

fn prompt_capable() -> bool {
    io::stdin().is_tty_ext() && io::stdout().is_tty_ext()
}

fn quiet_env_overrides() -> bool {
    matches!(
        env::var("VTCODE_TRUST_WORKSPACE_QUIET").ok().as_deref(),
        Some("1") | Some("true") | Some("yes")
    )
}

fn parse_env_trust_override() -> Result<Option<EnvTrustOverride>> {
    let raw = match env::var(TRUST_OVERRIDE_ENV) {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => return Ok(None),
        Err(env::VarError::NotUnicode(_)) => {
            bail!("{TRUST_OVERRIDE_ENV} must be valid UTF-8");
        }
    };

    classify_env_trust_value(&raw)
        .map(Some)
        .with_context(|| format!("Invalid value for {TRUST_OVERRIDE_ENV}: '{raw}'"))
}

fn classify_env_trust_value(raw: &str) -> Result<EnvTrustOverride> {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "" => bail!(
            "value is empty. Use 'full-auto' to grant trust or 'deny' to refuse without prompting."
        ),
        "full-auto" | "full_auto" | "fullauto" | "trust" | "trusted" | "1" | "yes" | "on" => {
            Ok(EnvTrustOverride::FullAuto)
        }
        "deny" | "denied" | "0" | "no" | "off" => Ok(EnvTrustOverride::Deny),
        _ => bail!(
            "unsupported value. Use 'full-auto' to grant trust or 'deny' to refuse without prompting."
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
        "Full-auto, exec, and autonomous runs require workspace trust.",
        PromptTone::Body,
    );
    print_prompt_line(
        &format!(
            "Set {TRUST_OVERRIDE_ENV}=full-auto to skip this prompt in CI / non-interactive runs."
        ),
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
        "Full-auto permission review requires upgrading this workspace to full auto."
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

    #[test]
    fn classify_env_trust_value_recognises_grant_aliases() {
        for value in [
            "full-auto",
            "Full_Auto",
            "FULLAUTO",
            "trust",
            "trusted",
            "1",
            "yes",
            "on",
        ] {
            assert_eq!(
                classify_env_trust_value(value).expect("grant alias should parse"),
                EnvTrustOverride::FullAuto,
                "{value} should grant trust"
            );
        }
    }

    #[test]
    fn classify_env_trust_value_recognises_deny_aliases() {
        for value in ["deny", "DENIED", "0", "no", "off"] {
            assert_eq!(
                classify_env_trust_value(value).expect("deny alias should parse"),
                EnvTrustOverride::Deny,
                "{value} should deny trust"
            );
        }
    }

    #[test]
    fn classify_env_trust_value_rejects_unknown_values() {
        let err = classify_env_trust_value("maybe").expect_err("unknown value should fail");
        assert!(err.to_string().contains("unsupported value"));
    }

    #[test]
    fn classify_env_trust_value_rejects_empty() {
        let err = classify_env_trust_value("   ").expect_err("empty value should fail");
        assert!(err.to_string().contains("empty"));
    }
}
