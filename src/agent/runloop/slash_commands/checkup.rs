use super::*;

#[derive(Debug)]
pub(in crate::agent::runloop::slash_commands) enum CheckupCommand {
    Interactive,
    Run { quick: bool },
}

#[derive(Debug)]
pub(in crate::agent::runloop::slash_commands) enum DoctorCommand {
    Checkup(CheckupCommand),
    PromptAudit,
}

pub(in crate::agent::runloop::slash_commands) fn parse_checkup_args(
    args: &str,
    supports_inline_ui: bool,
) -> std::result::Result<CheckupCommand, String> {
    parse_checkup_flags(
        args,
        supports_inline_ui,
        "Usage: /checkup [--quick|--full]\nExamples: /checkup, /checkup --quick",
    )
}

pub(in crate::agent::runloop::slash_commands) fn parse_doctor_args(
    args: &str,
    supports_inline_ui: bool,
) -> std::result::Result<DoctorCommand, String> {
    const PROMPT_AUDIT_USAGE: &str = "Usage: /doctor prompt-audit";
    let trimmed = args.trim();
    let mut tokens = trimmed.split_whitespace();
    let first = tokens.next();

    if first.is_some_and(is_prompt_audit_token) {
        return if tokens.next().is_none() {
            Ok(DoctorCommand::PromptAudit)
        } else {
            Err(PROMPT_AUDIT_USAGE.to_string())
        };
    }
    if tokens.any(is_prompt_audit_token) {
        return Err(PROMPT_AUDIT_USAGE.to_string());
    }

    parse_checkup_flags(
        trimmed,
        supports_inline_ui,
        "Usage: /doctor [--quick|--full|prompt-audit]\nExamples: /doctor, /doctor --quick, /doctor prompt-audit",
    )
    .map(DoctorCommand::Checkup)
}

fn is_prompt_audit_token(token: &str) -> bool {
    token.eq_ignore_ascii_case("prompt-audit")
}

pub(super) fn parse_checkup_flags(
    args: &str,
    supports_inline_ui: bool,
    usage: &str,
) -> std::result::Result<CheckupCommand, String> {
    let mut quick = false;
    let mut full = false;

    parsing::for_each_token(args, |token| {
        match token {
            "--quick" | "-q" | "quick" => quick = true,
            "--full" | "full" => full = true,
            _ => {
                return Err(usage.to_string());
            }
        }
        Ok(())
    })?;

    if quick && full {
        return Err("Use either --quick or --full, not both.".to_string());
    }

    if !quick && !full && supports_inline_ui {
        return Ok(CheckupCommand::Interactive);
    }

    Ok(CheckupCommand::Run { quick })
}
