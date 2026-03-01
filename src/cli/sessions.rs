use anyhow::{Context, Result, anyhow};
use chrono::Local;
use dialoguer::{Select, theme::ColorfulTheme};
use std::path::PathBuf;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::provider::Message;
use vtcode_core::utils::colors::style;
use vtcode_core::utils::session_archive::{
    SessionListing, find_session_by_identifier, list_recent_sessions,
};

use crate::agent::agents::ResumeSession;
use vtcode::startup::SessionResumeMode;

const INTERACTIVE_SESSION_LIMIT: usize = 10;

pub async fn handle_resume_session_command(
    config: &CoreAgentConfig,
    mode: SessionResumeMode,
    custom_session_id: Option<String>,
    skip_confirmations: bool,
) -> Result<()> {
    let resume = match mode {
        SessionResumeMode::Latest => select_latest_session(false).await?,
        SessionResumeMode::Specific(identifier) => {
            Some(load_specific_session(&identifier, false).await?)
        }
        SessionResumeMode::Interactive => select_session_interactively(false).await?,
        SessionResumeMode::Fork(identifier) => {
            if identifier == "__latest__" {
                select_latest_session(true).await?
            } else {
                Some(load_specific_session(&identifier, true).await?)
            }
        }
    };

    let Some(mut resume) = resume else {
        println!("{}", style("No session selected. Exiting.").red());
        return Ok(());
    };

    // If custom_session_id is provided, mark as fork
    if let Some(suffix) = custom_session_id {
        resume = fork_session(resume, suffix);
        print_fork_summary(&resume);
    } else {
        print_resume_summary(&resume);
    }

    run_single_agent_loop(config, skip_confirmations, resume).await
}

async fn select_latest_session(is_fork: bool) -> Result<Option<ResumeSession>> {
    let mut listings = list_recent_sessions(1)
        .await
        .context("failed to load recent sessions")?;
    if let Some(listing) = listings.pop() {
        Ok(Some(convert_listing(&listing, is_fork)))
    } else {
        println!("{}", style("No archived sessions were found.").red());
        Ok(None)
    }
}

async fn load_specific_session(identifier: &str, is_fork: bool) -> Result<ResumeSession> {
    let listing = find_session_by_identifier(identifier)
        .await?
        .ok_or_else(|| anyhow!("No session with identifier '{}' was found.", identifier))?;
    Ok(convert_listing(&listing, is_fork))
}

async fn select_session_interactively(is_fork: bool) -> Result<Option<ResumeSession>> {
    let listings = list_recent_sessions(INTERACTIVE_SESSION_LIMIT)
        .await
        .context("failed to load recent sessions")?;
    if listings.is_empty() {
        println!("{}", style("No archived sessions were found.").red());
        return Ok(None);
    }

    let mut options = Vec::new();
    for listing in &listings {
        options.push(format_listing(listing));
    }

    let prompt_text = if is_fork {
        "Select a session to fork"
    } else {
        "Select a session to resume"
    };

    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt_text)
        .items(&options)
        .default(0)
        .interact_opt()
        .context("failed to read interactive selection (use --resume <id> in non-interactive environments)")?;

    let Some(index) = selection else {
        return Ok(None);
    };

    Ok(Some(convert_listing(&listings[index], is_fork)))
}

fn convert_listing(listing: &SessionListing, is_fork: bool) -> ResumeSession {
    // Prefer full archived messages; fall back to recent progress if the full log is absent.
    let history_source = if !listing.snapshot.messages.is_empty() {
        listing.snapshot.messages.iter()
    } else if let Some(progress) = &listing.snapshot.progress {
        progress.recent_messages.iter()
    } else {
        [].iter()
    };

    let history = history_source.map(Message::from).collect();

    ResumeSession {
        identifier: listing.identifier(),
        snapshot: listing.snapshot.clone(),
        history,
        path: listing.path.clone(),
        is_fork,
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

fn fork_session(mut original: ResumeSession, custom_suffix: String) -> ResumeSession {
    // Update identifier to reflect fork
    original.identifier = format!("forked-{}", custom_suffix);
    original.is_fork = true;
    original
}

fn print_fork_summary(resume: &ResumeSession) {
    let ended = resume
        .snapshot
        .ended_at
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M");
    println!(
        "{}",
        style(format!(
            "Forking session with {} messages (original ended {})",
            resume.message_count(),
            ended
        ))
        .green()
    );
    println!(
        "{}",
        style(format!("Original archive: {}", resume.path.display())).green()
    );
    println!("{}", style("Starting independent forked session").green());
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
                    .red()
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
                    .red()
                );
            }

            if !exists {
                println!(
                    "{}",
                    style(format!(
                        "Archived workspace {} could not be found on disk. Tools will operate relative to the archived path.",
                        path.display()
                    ))
                    .red()
                );
            }

            resume_config.workspace = path;
        }
    }

    crate::agent::agents::run_single_agent_loop(
        &resume_config,
        skip_confirmations,
        false,
        false,
        None,
        Some(resume),
    )
    .await
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use vtcode_core::llm::provider::MessageRole;
    use vtcode_core::utils::session_archive::{
        SessionArchiveMetadata, SessionMessage, SessionProgress, SessionSnapshot,
    };

    #[test]
    fn convert_listing_prefers_progress_messages() {
        let progress_msg = SessionMessage::new(MessageRole::Assistant, "progress");
        let snapshot = SessionSnapshot {
            metadata: SessionArchiveMetadata::new(
                "ws", "/tmp/ws", "model", "provider", "theme", "medium",
            ),
            started_at: Utc::now(),
            ended_at: Utc::now(),
            total_messages: 2,
            distinct_tools: vec!["tool_a".to_string()],
            transcript: Vec::new(),
            messages: vec![SessionMessage::new(MessageRole::User, "full")],
            progress: Some(SessionProgress {
                turn_number: 2,
                recent_messages: vec![progress_msg.clone()],
                tool_summaries: vec!["tool_a".to_string()],
                token_usage: None,
                max_context_tokens: Some(128),
                loaded_skills: Vec::new(),
            }),
            error_logs: Vec::new(),
        };

        let listing = SessionListing {
            path: PathBuf::new(),
            snapshot,
        };

        let resume = convert_listing(&listing, false);
        assert_eq!(resume.history.len(), 1);
        assert_eq!(resume.history[0].content.as_text(), "progress");
        assert!(!resume.is_fork); // Verify is_fork is false for non-forked sessions
    }
}
