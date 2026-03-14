use anyhow::{Context, Result};
use chrono::Utc;
use std::fs::File;
use std::io::{self, BufWriter};
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use vtcode_core::config::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::runner::{AgentRunner, RunnerSettings};
use vtcode_core::core::agent::task::{ContextItem, Task};
use vtcode_core::core::agent::types::AgentType;
use vtcode_core::utils::file_utils::write_file_with_context;
use vtcode_core::utils::session_archive::SessionMessage;

use super::event_output::{
    ExecEventProcessor, exec_archive_transcript, lock_or_recover, open_events_writer,
};
use super::{ExecCommandKind, ExecCommandOptions, prep};

const EXEC_TASK_ID: &str = "exec-task";
const EXEC_TASK_TITLE: &str = "Exec Task";
const EXEC_TASK_INSTRUCTIONS: &str = "You are running vtcode in non-interactive exec mode. Complete the task autonomously using the configured full-auto tool allowlist. Do not request additional user input, confirmations, or allowances—operate solely with the provided information and available tools. Provide a concise summary of the outcome when finished.";
const EXEC_TASK_INSTRUCTIONS_DRY_RUN: &str = "You are running vtcode in non-interactive exec dry-run mode. Plan and validate the approach in read-only mode without mutating files, running mutating commands, or requesting additional user input. If the task requires mutations, explain what would be changed and why.";
pub(super) const REVIEW_TASK_ID: &str = "review-task";
const REVIEW_TASK_TITLE: &str = "Review Task";
const REVIEW_TASK_INSTRUCTIONS: &str = "You are running vtcode in non-interactive review mode. Review the requested target in read-only mode. Do not modify files, do not run mutating commands, and do not request user input. Return findings first, ordered by severity, with concrete file and line references when possible. If there are no findings, state that explicitly.";

pub(super) struct TaskSpec {
    pub(super) id: &'static str,
    pub(super) title: &'static str,
    pub(super) instructions: &'static str,
}

pub(super) fn task_instructions(dry_run: bool) -> &'static str {
    if dry_run {
        EXEC_TASK_INSTRUCTIONS_DRY_RUN
    } else {
        EXEC_TASK_INSTRUCTIONS
    }
}

pub(super) fn task_spec(command: &ExecCommandKind, dry_run: bool) -> TaskSpec {
    match command {
        ExecCommandKind::Review { .. } => TaskSpec {
            id: REVIEW_TASK_ID,
            title: REVIEW_TASK_TITLE,
            instructions: REVIEW_TASK_INSTRUCTIONS,
        },
        _ => TaskSpec {
            id: EXEC_TASK_ID,
            title: EXEC_TASK_TITLE,
            instructions: task_instructions(dry_run),
        },
    }
}

pub(super) fn resolve_exec_event_log_path(path: &str, session_id: &str) -> PathBuf {
    let mut base = PathBuf::from(path);
    if base.extension().is_none() {
        let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ");
        base = base.join(format!("harness-{session_id}-{timestamp}.jsonl"));
    }
    base
}

pub(super) fn effective_exec_events_path(
    cli_events_path: Option<&Path>,
    harness_event_log_path: Option<&str>,
    session_id: &str,
) -> Option<PathBuf> {
    cli_events_path.map(Path::to_path_buf).or_else(|| {
        let explicit = harness_event_log_path.filter(|path| !path.trim().is_empty());
        let effective = explicit
            .map(String::from)
            .or_else(|| default_harness_log_dir().map(|d| d.to_string_lossy().into_owned()));
        effective.map(|path| resolve_exec_event_log_path(&path, session_id))
    })
}

/// Returns the default harness log directory (`~/.vtcode/sessions/`).
fn default_harness_log_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".vtcode").join("sessions"))
}

pub(super) async fn handle_exec_command_impl(
    config: &CoreAgentConfig,
    vt_cfg: &VTCodeConfig,
    options: ExecCommandOptions,
) -> Result<()> {
    let prepared = prep::prepare_exec_run(config, vt_cfg, &options).await?;
    let prep::ExecPreparedRun {
        config: run_config,
        vt_cfg: run_vt_cfg,
        model_id,
        prompt,
        session_id,
        archive,
        thread_bootstrap,
    } = prepared;
    let task_spec = task_spec(&options.command, options.dry_run);
    let event_session_id = session_id.clone();

    let automation_cfg = &run_vt_cfg.automation.full_auto;
    let mut runner = AgentRunner::new_with_thread_bootstrap_and_config(
        AgentType::Single,
        model_id,
        run_config.api_key.clone(),
        run_config.workspace.clone(),
        session_id,
        RunnerSettings {
            reasoning_effort: Some(run_config.reasoning_effort),
            verbosity: None,
        },
        None,
        thread_bootstrap,
        run_vt_cfg.clone(),
    )
    .await?;

    let allowed_tools = match &options.command {
        ExecCommandKind::Review { .. } => {
            runner
                .review_tool_allowlist(&automation_cfg.allowed_tools)
                .await
        }
        _ => automation_cfg.allowed_tools.clone(),
    };
    runner.enable_full_auto(&allowed_tools).await;
    if options.dry_run {
        runner.enable_plan_mode();
    }
    runner.set_quiet(true);
    let events_path = effective_exec_events_path(
        options.events_path.as_deref(),
        run_vt_cfg.agent.harness.event_log_path.as_deref(),
        &event_session_id,
    );

    let processor = Arc::new(Mutex::new(ExecEventProcessor::<
        io::Stdout,
        BufWriter<File>,
        io::Stderr,
    >::new(
        options.json,
        !options.json && !run_config.quiet,
        options.json.then(io::stdout),
        events_path
            .as_ref()
            .map(|path| open_events_writer(path.as_path()))
            .transpose()?,
        (!options.json && !run_config.quiet).then(io::stderr),
    )));
    let event_processor = Arc::clone(&processor);
    runner.set_event_handler(move |event| {
        let mut processor = lock_or_recover(&event_processor);
        processor.process_event(event);
    });

    let task = Task {
        id: task_spec.id.into(),
        title: task_spec.title.into(),
        description: prompt.trim().to_string(),
        instructions: Some(task_spec.instructions.into()),
    };

    let max_retries = run_vt_cfg.agent.max_task_retries;
    let result = runner
        .execute_task_with_retry(&task, &[] as &[ContextItem], max_retries)
        .await
        .context("Failed to execute autonomous task after retries")?;

    let last_message = {
        let mut processor = lock_or_recover(&processor);
        let message = processor.final_message().unwrap_or_default().to_string();
        processor.finish_output(&result, options.dry_run);
        message
    };

    if let Some(path) = &options.last_message_file {
        let message = last_message.as_str();
        write_file_with_context(path, message, "last message file").await?;
        if message.is_empty() {
            let mut processor = lock_or_recover(&processor);
            processor.warn_empty_last_message(path);
        }
    }

    let session_messages = runner.session_messages();
    let session_archive_messages: Vec<SessionMessage> =
        session_messages.iter().map(SessionMessage::from).collect();
    if let Some(archive) = archive {
        archive
            .finalize(
                exec_archive_transcript(&session_messages),
                session_archive_messages.len(),
                result.executed_commands.clone(),
                session_archive_messages,
            )
            .context("Failed to save exec session archive")?;
    }

    let mut processor = lock_or_recover(&processor);
    processor
        .take_error()
        .context("Failed to process exec event output")?;

    Ok(())
}
