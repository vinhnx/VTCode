use anyhow::{Context, Result, anyhow};
use chrono::Local;
use console::style;
use dialoguer::{Select, theme::ColorfulTheme};
use std::path::PathBuf;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::provider::Message;
use vtcode_core::utils::session_archive::{
    SessionListing, find_session_by_identifier, list_recent_sessions,
};

use crate::agent::runloop::{self, ResumeSession};
use vtcode::startup::SessionResumeMode;

const INTERACTIVE_SESSION_LIMIT: usize = 10;

pub async fn handle_resume_session_command(
    config: &CoreAgentConfig,
    mode: SessionResumeMode,
    skip_confirmations: bool,
) -> Result<()> {
    let resume = match mode {
        SessionResumeMode::Latest => select_latest_session()?,
        SessionResumeMode::Specific(identifier) => Some(load_specific_session(&identifier)?),
        SessionResumeMode::Interactive => select_session_interactively()?,
    };

    let Some(resume) = resume else {
        println!("{}", style("No session selected. Exiting.").yellow());
        return Ok(());
    };

    print_resume_summary(&resume);

    run_single_agent_loop(config, skip_confirmations, resume).await
}

fn select_latest_session() -> Result<Option<ResumeSession>> {
    let mut listings = list_recent_sessions(1).context("failed to load recent sessions")?;
    if let Some(listing) = listings.pop() {
        Ok(Some(convert_listing(&listing)))
    } else {
        println!("{}", style("No archived sessions were found.").yellow());
        Ok(None)
    }
}

fn load_specific_session(identifier: &str) -> Result<ResumeSession> {
    let listing = find_session_by_identifier(identifier)?
        .ok_or_else(|| anyhow!("No session with identifier '{}' was found.", identifier))?;
    Ok(convert_listing(&listing))
}

fn select_session_interactively() -> Result<Option<ResumeSession>> {
    let listings = list_recent_sessions(INTERACTIVE_SESSION_LIMIT)
        .context("failed to load recent sessions")?;
    if listings.is_empty() {
        println!("{}", style("No archived sessions were found.").yellow());
        return Ok(None);
    }

    let mut options = Vec::new();
    for listing in &listings {
        options.push(format_listing(listing));
    }

    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select a session to resume")
        .items(&options)
        .default(0)
        .interact_opt()
        .context("failed to read interactive selection (use --resume <id> in non-interactive environments)")?;

    let Some(index) = selection else {
        return Ok(None);
    };

    Ok(Some(convert_listing(&listings[index])))
}

fn convert_listing(listing: &SessionListing) -> ResumeSession {
    let history = listing
        .snapshot
        .messages
        .iter()
        .map(Message::from)
        .collect();

    ResumeSession {
        identifier: listing.identifier(),
        snapshot: listing.snapshot.clone(),
        history,
        path: listing.path.clone(),
    }
}

fn format_listing(listing: &SessionListing) -> String {
    let ended = listing
        .snapshot
        .ended_at
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M");
    let mut summary = format!(
        "{} · {} · {} msgs",
        ended, listing.snapshot.metadata.model, listing.snapshot.total_messages
    );
    if let Some(prompt) = listing.first_prompt_preview() {
        summary.push_str(&format!("\n  prompt: {}", prompt));
    }
    if let Some(reply) = listing.first_reply_preview() {
        summary.push_str(&format!("\n  reply: {}", reply));
    }
    summary
}

fn print_resume_summary(resume: &ResumeSession) {
    let ended = resume
        .snapshot
        .ended_at
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M");
    println!(
        "{}",
        style(format!(
            "Resuming session {} ({} messages, ended {})",
            resume.identifier,
            resume.message_count(),
            ended
        ))
        .green()
    );
    println!(
        "{}",
        style(format!("Archive: {}", resume.path.display())).green()
    );
}

async fn run_single_agent_loop(
    config: &CoreAgentConfig,
    skip_confirmations: bool,
    resume: ResumeSession,
) -> Result<()> {
    let mut resume_config = config.clone();
    match parse_archived_workspace(&resume) {
        ParsedWorkspace::Missing => {
            println!(
                "{}",
                style("Archived session is missing workspace metadata; continuing with the current workspace.")
                    .yellow()
            );
        }
        ParsedWorkspace::Provided { path, exists } => {
            if path != config.workspace {
                println!(
                    "{}",
                    style(format!(
                        "Archived workspace {} differs from the current CLI workspace {}. Switching to archived location.",
                        path.display(),
                        config.workspace.display()
                    ))
                    .yellow()
                );
            }

            if !exists {
                println!(
                    "{}",
                    style(format!(
                        "Archived workspace {} could not be found on disk. Tools will operate relative to the archived path.",
                        path.display()
                    ))
                    .yellow()
                );
            }

            resume_config.workspace = path;
        }
    }

    runloop::run_single_agent_loop(&resume_config, skip_confirmations, false, Some(resume)).await
}

enum ParsedWorkspace {
    Missing,
    Provided { path: PathBuf, exists: bool },
}

fn parse_archived_workspace(resume: &ResumeSession) -> ParsedWorkspace {
    let raw_path = resume.snapshot.metadata.workspace_path.trim();
    if raw_path.is_empty() {
        return ParsedWorkspace::Missing;
    }

    let archived_path = PathBuf::from(raw_path);
    let exists = archived_path.exists();
    ParsedWorkspace::Provided {
        path: archived_path,
        exists,
    }
}
