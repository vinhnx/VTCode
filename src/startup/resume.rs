use anyhow::{Result, bail};
use vtcode_core::cli::args::{Cli, Commands, ExecSubcommand};
use vtcode_core::utils::validation::validate_non_empty;

use super::SessionResumeMode;

fn validate_session_id_suffix(suffix: &str) -> Result<()> {
    validate_non_empty(suffix, "Custom session ID suffix")?;
    if suffix.len() > 64 {
        bail!("Custom session ID suffix too long (maximum 64 characters)");
    }
    if !suffix
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!(
            "Custom session ID suffix must contain only alphanumeric characters, dashes, or underscores"
        );
    }
    Ok(())
}

pub(super) fn resolve_session_resume(
    args: &Cli,
) -> Result<(Option<String>, Option<SessionResumeMode>)> {
    let custom_session_id = args.session_id.clone();
    if let Some(ref suffix) = custom_session_id {
        validate_session_id_suffix(suffix)?;
    }

    let session_resume = if let Some(fork_id) = args.fork_session.as_ref() {
        Some(SessionResumeMode::Fork(fork_id.clone()))
    } else if let Some(value) = args.resume_session.as_ref() {
        if value == "__interactive__" {
            Some(SessionResumeMode::Interactive)
        } else if custom_session_id.is_some() {
            Some(SessionResumeMode::Fork(value.clone()))
        } else {
            Some(SessionResumeMode::Specific(value.clone()))
        }
    } else if args.continue_latest {
        if custom_session_id.is_some() {
            Some(SessionResumeMode::Fork("__latest__".to_string()))
        } else {
            Some(SessionResumeMode::Latest)
        }
    } else {
        None
    };

    Ok((custom_session_id, session_resume))
}

pub(super) fn validate_resume_all_usage(
    args: &Cli,
    session_resume: Option<&SessionResumeMode>,
) -> Result<()> {
    if args.all
        && session_resume.is_none()
        && !matches!(
            args.command,
            Some(Commands::Exec {
                command: Some(ExecSubcommand::Resume(_)),
                ..
            })
        )
    {
        bail!("--all can only be used with resume, continue, fork-session, or exec resume");
    }

    Ok(())
}
