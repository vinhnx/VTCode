use anyhow::{Context, Result, anyhow};
use chrono::Local;
use dialoguer::{Select, theme::ColorfulTheme};
use std::path::PathBuf;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::interfaces::session::PlanModeEntrySource;
use vtcode_core::core::threads::{
    ArchivedSessionIntent, SessionQueryScope, list_recent_sessions_in_scope,
};
use vtcode_core::utils::colors::style;
use vtcode_core::utils::session_archive::{
    SessionContinuationMetadata, SessionContinuationRecommendedAction, SessionListing,
    session_workspace_path,
};

use crate::agent::agents::{ResumeSession, SessionContinuation};
use crate::startup::SessionResumeMode;

const INTERACTIVE_SESSION_LIMIT: usize = 10;

enum ResumeExecutionMode {
    Resume(Box<ResumeSession>),
    StartFresh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BudgetResumeAction {
    ContinueFromSummary,
    ContinueFullHistory,
    StartFresh,
    Cancel,
}

struct BudgetResumeMenu {
    actions: Vec<BudgetResumeAction>,
    options: Vec<String>,
    default_index: usize,
}

pub async fn handle_resume_session_command(
    config: &CoreAgentConfig,
    mode: SessionResumeMode,
    show_all: bool,
    custom_session_id: Option<String>,
    summarize_fork: bool,
    skip_confirmations: bool,
) -> Result<()> {
    let interactive_intent = match &mode {
        SessionResumeMode::Fork(_) => ArchivedSessionIntent::ForkNewArchive {
            custom_suffix: custom_session_id.clone(),
            summarize: summarize_fork,
        },
        _ if custom_session_id.is_some() => ArchivedSessionIntent::ForkNewArchive {
            custom_suffix: custom_session_id.clone(),
            summarize: summarize_fork,
        },
        _ => ArchivedSessionIntent::ResumeInPlace,
    };
    let scope = if show_all {
        SessionQueryScope::All
    } else {
        SessionQueryScope::CurrentWorkspace(config.workspace.clone())
    };

    let resume = match mode {
        SessionResumeMode::Latest => {
            select_latest_session(&scope, interactive_intent.clone()).await?
        }
        SessionResumeMode::Specific(identifier) => {
            Some(load_specific_session(&identifier, interactive_intent.clone()).await?)
        }
        SessionResumeMode::Interactive => {
            select_session_interactively(&scope, interactive_intent.clone()).await?
        }
        SessionResumeMode::Fork(identifier) => {
            if identifier == "__latest__" {
                select_latest_session(&scope, interactive_intent.clone()).await?
            } else {
                Some(load_specific_session(&identifier, interactive_intent.clone()).await?)
            }
        }
    };

    let Some(resume) = resume else {
        println!("{}", style("No session selected. Exiting.").red());
        return Ok(());
    };

    let Some(execution_mode) = maybe_choose_budget_limited_resume_mode(resume)? else {
        println!("{}", style("No session selected. Exiting.").red());
        return Ok(());
    };

    let resume = match execution_mode {
        ResumeExecutionMode::Resume(resume) => *resume,
        ResumeExecutionMode::StartFresh => {
            return crate::agent::agents::run_single_agent_loop(
                config,
                None,
                skip_confirmations,
                false,
                PlanModeEntrySource::None,
                None,
            )
            .await;
        }
    };

    if resume.is_fork() {
        print_fork_summary(&resume);
    } else {
        print_resume_summary(&resume);
    }

    run_single_agent_loop(config, skip_confirmations, resume).await
}

async fn select_latest_session(
    scope: &SessionQueryScope,
    intent: ArchivedSessionIntent,
) -> Result<Option<SessionContinuation>> {
    let mut listings = list_recent_sessions_in_scope(1, scope)
        .await
        .context("failed to load recent sessions")?;
    if let Some(listing) = listings.pop() {
        Ok(Some(convert_listing(&listing, intent)))
    } else {
        println!("{}", style("No archived sessions were found.").red());
        Ok(None)
    }
}

async fn load_specific_session(
    identifier: &str,
    intent: ArchivedSessionIntent,
) -> Result<SessionContinuation> {
    crate::agent::agents::load_resume_session(identifier, intent)
        .await?
        .ok_or_else(|| anyhow!("No session with identifier '{}' was found.", identifier))
}

async fn select_session_interactively(
    scope: &SessionQueryScope,
    intent: ArchivedSessionIntent,
) -> Result<Option<SessionContinuation>> {
    let listings = list_recent_sessions_in_scope(INTERACTIVE_SESSION_LIMIT, scope)
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

    let prompt_text = if matches!(intent, ArchivedSessionIntent::ForkNewArchive { .. }) {
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

    let Some(intent) = maybe_choose_fork_mode(intent)? else {
        return Ok(None);
    };
    Ok(Some(convert_listing(&listings[index], intent)))
}

fn maybe_choose_fork_mode(intent: ArchivedSessionIntent) -> Result<Option<ArchivedSessionIntent>> {
    let ArchivedSessionIntent::ForkNewArchive {
        custom_suffix,
        summarize,
    } = intent
    else {
        return Ok(Some(intent));
    };

    if summarize {
        return Ok(Some(ArchivedSessionIntent::ForkNewArchive {
            custom_suffix,
            summarize: true,
        }));
    }

    let options = vec![
        "Copy full history".to_string(),
        "Start summarized fork".to_string(),
        "Cancel".to_string(),
    ];
    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Choose how the forked session should start.")
        .items(&options)
        .default(0)
        .interact()
        .context("failed to select fork mode")?;

    match selection {
        0 => Ok(Some(ArchivedSessionIntent::ForkNewArchive {
            custom_suffix,
            summarize: false,
        })),
        1 => Ok(Some(ArchivedSessionIntent::ForkNewArchive {
            custom_suffix,
            summarize: true,
        })),
        _ => Ok(None),
    }
}

fn maybe_choose_budget_limited_resume_mode(
    resume: ResumeSession,
) -> Result<Option<ResumeExecutionMode>> {
    let Some(continuation) = resume.budget_limit_continuation() else {
        return Ok(Some(ResumeExecutionMode::Resume(Box::new(resume))));
    };
    if resume.is_fork() {
        return Ok(Some(ResumeExecutionMode::Resume(Box::new(resume))));
    }

    println!(
        "{}",
        style("This session stopped after reaching the local budget limit.").yellow()
    );
    if let (Some(actual_cost_usd), Some(max_budget_usd)) = (
        continuation.actual_cost_usd(),
        continuation.max_budget_usd(),
    ) {
        println!(
            "{}",
            style(format!(
                "Prior spend: ${actual_cost_usd:.2} on a ${max_budget_usd:.2} session budget."
            ))
            .yellow()
        );
    }

    let menu = build_budget_resume_menu(continuation);
    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Choose how to continue.")
        .items(&menu.options)
        .default(menu.default_index)
        .interact()
        .context("failed to select a budget-limit resume mode")?;

    let Some(action) = menu.actions.get(selection).copied() else {
        return Ok(None);
    };

    Ok(resolve_budget_resume_action(resume, action))
}

fn build_budget_resume_menu(continuation: &SessionContinuationMetadata) -> BudgetResumeMenu {
    let mut actions = Vec::new();
    let mut options = Vec::new();

    if continuation.summary_available {
        actions.push(BudgetResumeAction::ContinueFromSummary);
        options.push("Continue from saved summary (recommended, lower cost)".to_string());
    }

    actions.push(BudgetResumeAction::ContinueFullHistory);
    options.push("Continue with full history (higher cost)".to_string());

    actions.push(BudgetResumeAction::StartFresh);
    options.push("Start fresh in a new session".to_string());

    actions.push(BudgetResumeAction::Cancel);
    options.push("Cancel".to_string());

    let default_action = preferred_budget_resume_action(continuation);
    let default_index = actions
        .iter()
        .position(|action| *action == default_action)
        .unwrap_or(0);

    BudgetResumeMenu {
        actions,
        options,
        default_index,
    }
}

fn preferred_budget_resume_action(
    continuation: &SessionContinuationMetadata,
) -> BudgetResumeAction {
    match continuation.recommended_action {
        Some(SessionContinuationRecommendedAction::ContinueFromSummary)
            if continuation.summary_available =>
        {
            BudgetResumeAction::ContinueFromSummary
        }
        Some(SessionContinuationRecommendedAction::StartFresh) => BudgetResumeAction::StartFresh,
        Some(SessionContinuationRecommendedAction::ContinueFullHistory) => {
            BudgetResumeAction::ContinueFullHistory
        }
        _ if continuation.summary_available => BudgetResumeAction::ContinueFromSummary,
        _ => BudgetResumeAction::ContinueFullHistory,
    }
}

fn resolve_budget_resume_action(
    resume: ResumeSession,
    action: BudgetResumeAction,
) -> Option<ResumeExecutionMode> {
    match action {
        BudgetResumeAction::ContinueFromSummary => {
            Some(ResumeExecutionMode::Resume(Box::new(convert_listing(
                resume.listing(),
                ArchivedSessionIntent::ForkNewArchive {
                    custom_suffix: None,
                    summarize: true,
                },
            ))))
        }
        BudgetResumeAction::ContinueFullHistory => {
            Some(ResumeExecutionMode::Resume(Box::new(resume)))
        }
        BudgetResumeAction::StartFresh => Some(ResumeExecutionMode::StartFresh),
        BudgetResumeAction::Cancel => None,
    }
}

fn convert_listing(listing: &SessionListing, intent: ArchivedSessionIntent) -> ResumeSession {
    ResumeSession::from_listing(listing, intent)
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
        .snapshot()
        .ended_at
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M");
    println!(
        "{}",
        style(format!(
            "Resuming session {} ({} messages, ended {})",
            resume.identifier(),
            resume.message_count(),
            ended
        ))
        .green()
    );
    println!(
        "{}",
        style(format!("Archive: {}", resume.path().display())).green()
    );
}

fn print_fork_summary(resume: &ResumeSession) {
    let ended = resume
        .snapshot()
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
        style(format!("Original archive: {}", resume.path().display())).green()
    );
    if resume.summarize_fork() {
        println!("{}", style("Fork mode: summarized history").green());
    } else {
        println!("{}", style("Fork mode: full history copy").green());
    }
    println!("{}", style("Starting independent forked session").green());
}

async fn run_single_agent_loop(
    config: &CoreAgentConfig,
    skip_confirmations: bool,
    resume: ResumeSession,
) -> Result<()> {
    let mut resume_config = config.clone();
    match resolve_resume_workspace(&resume, config)? {
        ParsedWorkspace::Cancelled => {
            println!("{}", style("No session selected. Exiting.").red());
            return Ok(());
        }
        ParsedWorkspace::Missing => {
            println!(
                "{}",
                style("Archived session is missing workspace metadata; continuing with the current workspace.")
                    .red()
            );
        }
        ParsedWorkspace::Provided { path } => {
            resume_config.workspace = path;
        }
    }

    crate::agent::agents::run_single_agent_loop(
        &resume_config,
        None,
        skip_confirmations,
        false,
        PlanModeEntrySource::None,
        Some(resume),
    )
    .await
}

enum ParsedWorkspace {
    Cancelled,
    Missing,
    Provided { path: PathBuf },
}

fn resolve_resume_workspace(
    resume: &ResumeSession,
    config: &CoreAgentConfig,
) -> Result<ParsedWorkspace> {
    let Some(archived_path) = session_workspace_path(resume.listing()) else {
        return Ok(ParsedWorkspace::Missing);
    };

    if !archived_path.exists() {
        return Err(anyhow!(
            "Archived workspace '{}' could not be found on disk.",
            archived_path.display()
        ));
    }

    if archived_path == config.workspace {
        return Ok(ParsedWorkspace::Provided {
            path: archived_path,
        });
    }

    let action = if resume.is_fork() { "fork" } else { "resume" };
    let options = vec![
        format!("Use archived workspace ({})", archived_path.display()),
        format!("Use current workspace ({})", config.workspace.display()),
        "Cancel".to_string(),
    ];
    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(format!(
            "Session workspace differs from the current workspace. Choose the workspace to use for this {action}."
        ))
        .items(&options)
        .default(0)
        .interact()
        .context("failed to resolve workspace for resume or fork")?;

    let path = match selection {
        0 => archived_path,
        1 => config.workspace.clone(),
        _ => return Ok(ParsedWorkspace::Cancelled),
    };

    Ok(ParsedWorkspace::Provided { path })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use vtcode_core::llm::provider::MessageRole;
    use vtcode_core::utils::session_archive::{
        SessionArchiveMetadata, SessionContinuationMetadata, SessionMessage, SessionProgress,
        SessionSnapshot,
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

        let resume = convert_listing(&listing, ArchivedSessionIntent::ResumeInPlace);
        assert_eq!(resume.history().len(), 1);
        assert_eq!(resume.history()[0].content.as_text(), "progress");
        assert!(!resume.is_fork());
    }

    #[test]
    fn budget_resume_menu_prefers_saved_summary_when_available() {
        let menu =
            build_budget_resume_menu(&SessionContinuationMetadata::budget_limit(2.5, 2.7, true));

        assert_eq!(menu.default_index, 0);
        assert_eq!(
            menu.actions,
            vec![
                BudgetResumeAction::ContinueFromSummary,
                BudgetResumeAction::ContinueFullHistory,
                BudgetResumeAction::StartFresh,
                BudgetResumeAction::Cancel,
            ]
        );
    }

    #[test]
    fn budget_resume_menu_falls_back_to_full_history_without_saved_summary() {
        let menu =
            build_budget_resume_menu(&SessionContinuationMetadata::budget_limit(2.5, 2.7, false));

        assert_eq!(menu.default_index, 0);
        assert_eq!(
            menu.actions,
            vec![
                BudgetResumeAction::ContinueFullHistory,
                BudgetResumeAction::StartFresh,
                BudgetResumeAction::Cancel,
            ]
        );
    }

    #[test]
    fn resolve_budget_resume_action_converts_summary_choice_into_summarized_fork() {
        let listing = SessionListing {
            path: PathBuf::new(),
            snapshot: SessionSnapshot {
                metadata: SessionArchiveMetadata::new(
                    "ws", "/tmp/ws", "model", "provider", "theme", "medium",
                )
                .with_continuation_metadata(Some(
                    SessionContinuationMetadata::budget_limit(2.5, 2.7, true),
                )),
                started_at: Utc::now(),
                ended_at: Utc::now(),
                total_messages: 1,
                distinct_tools: Vec::new(),
                transcript: Vec::new(),
                messages: vec![SessionMessage::new(MessageRole::User, "full")],
                progress: None,
                error_logs: Vec::new(),
            },
        };
        let resume = convert_listing(&listing, ArchivedSessionIntent::ResumeInPlace);

        let outcome = resolve_budget_resume_action(resume, BudgetResumeAction::ContinueFromSummary)
            .expect("summary action should continue");

        match outcome {
            ResumeExecutionMode::Resume(resume) => {
                let resume = *resume;
                assert!(resume.is_fork());
                assert!(resume.summarize_fork());
            }
            ResumeExecutionMode::StartFresh => panic!("expected summarized fork resume"),
        }
    }
}
