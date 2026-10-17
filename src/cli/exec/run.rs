use anyhow::{Context, Result};
use chrono::Utc;
use std::fs::File;
use std::io::{self, BufWriter};
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::task::spawn_blocking;
use vtcode_core::config::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::events::SessionStoreSink;
use vtcode_core::core::agent::runner::{AgentRunner, RunnerSettings};
use vtcode_core::core::agent::task::{ContextItem, Task};
use vtcode_core::core::agent::types::AgentType;
use vtcode_core::exec::events::{
    ThreadCompletedEvent, ThreadCompletionSubtype, ThreadEvent, ThreadStartedEvent, TurnCompletedEvent,
    TurnFailedEvent, TurnStartedEvent, Usage,
};
use vtcode_core::git_info::get_head_commit_hash_async;
use vtcode_core::llm::provider::{FinishReason, LLMResponse};
use vtcode_core::review::ReviewTarget;
use vtcode_core::utils::file_utils::write_file_with_context;
use vtcode_core::utils::session_archive::{SessionMessage, SessionProgressArgs};

use super::event_output::{ExecEventProcessor, exec_archive_transcript, lock_or_recover, open_events_writer};
use super::{ExecCommandKind, ExecCommandOptions, prep};
use crate::codex_app_server::{
    CODEX_PROVIDER, CodexNonInteractiveRun, CodexReviewTarget, run_codex_noninteractive_with_instructions,
};

const EXEC_TASK_ID: &str = "exec-task";
const EXEC_TASK_TITLE: &str = "Exec Task";
const EXEC_TASK_INSTRUCTIONS: &str = "You are running vtcode in non-interactive exec mode. Complete the task autonomously using the configured full-auto tool allowlist. Do not request additional user input, confirmations, or allowances—operate solely with the provided information and available tools. Provide a concise summary of the outcome when finished.";
const EXEC_TASK_INSTRUCTIONS_DRY_RUN: &str = "You are running vtcode as a non-interactive dry-run execution. Plan and validate the approach with read-only permissions, without mutating files, running mutating commands, or requesting additional user input. If the task requires mutations, explain what would be changed and why.";
pub(super) const REVIEW_TASK_ID: &str = "review-task";
const REVIEW_TASK_TITLE: &str = "Review Task";
const REVIEW_TASK_INSTRUCTIONS: &str = "You are running vtcode through the non-interactive review command. Review the requested target with read-only permissions. Do not modify files, do not run mutating commands, and do not request user input. Return findings first, ordered by severity, with concrete file and line references when possible. If there are no findings, state that explicitly.";

async fn open_events_writer_async(path: &Path) -> Result<BufWriter<File>> {
    let path = path.to_path_buf();
    spawn_blocking(move || open_events_writer(&path))
        .await
        .context("exec events writer setup task failed")?
}

async fn apply_exec_retention(workspace: &Path, session_id: &str) {
    let workspace = workspace.to_path_buf();
    let session_id = session_id.to_string();
    match spawn_blocking(move || {
        vtcode_memory::apply_retention_preserving(
            &workspace,
            vtcode_memory::RetentionPolicy::default(),
            Some(session_id.as_str()),
        )
    })
    .await
    {
        Ok(Ok(removed)) if removed > 0 => {
            tracing::debug!(target: "vtcode.exec", removed, "pruned closed canonical sessions");
        }
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            tracing::warn!(target: "vtcode.exec", error = %error, "canonical session retention failed");
        }
        Err(error) => {
            tracing::warn!(target: "vtcode.exec", error = %error, "canonical session retention task failed");
        }
    }
}

async fn emit_canonical_event(canonical: &SessionStoreSink, event: &ThreadEvent, label: &str) -> Result<()> {
    if let Err(error) = canonical.emit(event) {
        let close_result = canonical.close().await;
        let error = error.context(format!("failed to enqueue {label} canonical session event"));
        return match close_result {
            Ok(()) => Err(error),
            Err(close_error) => {
                Err(error.context(format!("failed to finalize canonical session events: {close_error}")))
            }
        };
    }
    Ok(())
}

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
        ExecCommandKind::Eval { .. } => TaskSpec {
            id: "eval-task",
            title: "Eval Task",
            instructions: EXEC_TASK_INSTRUCTIONS,
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
        harness_event_log_path
            .filter(|path| !path.trim().is_empty())
            .map(|path| resolve_exec_event_log_path(path, session_id))
    })
}

async fn checkpoint_exec_archive(
    archive: &vtcode_core::utils::session_archive::SessionArchive,
    session_messages: &[vtcode_core::llm::provider::Message],
) -> Result<()> {
    let messages: Vec<SessionMessage> = session_messages.iter().map(SessionMessage::from).collect();
    archive
        .persist_progress_async(SessionProgressArgs {
            total_messages: session_messages.len(),
            distinct_tools: Vec::new(),
            messages: messages.clone(),
            recent_messages: messages,
            turn_number: 1,
            token_usage: None,
            max_context_tokens: None,
            loaded_skills: None,
            turn_diagnostics: None,
        })
        .await
        .context("Failed to checkpoint exec session archive")?;
    Ok(())
}

pub(super) async fn handle_exec_command_impl(
    config: &CoreAgentConfig,
    vt_cfg: &VTCodeConfig,
    options: ExecCommandOptions,
) -> Result<()> {
    // Handle eval command separately — it runs multiple tasks, not a single one.
    if let ExecCommandKind::Eval { suite_path, output_path, format } = &options.command {
        return super::eval::handle_eval_command(config, vt_cfg, suite_path, output_path.as_deref(), *format).await;
    }

    if config.provider.eq_ignore_ascii_case(CODEX_PROVIDER) {
        return handle_codex_exec_command_impl(config, vt_cfg, options).await;
    }

    let prepared = prep::prepare_exec_run(config, vt_cfg, &options).await?;
    let prep::ExecPreparedRun {
        config: run_config,
        vt_cfg: run_vt_cfg,
        model_id,
        active_primary_agent,
        prompt,
        session_id,
        archive,
        thread_bootstrap,
    } = prepared;
    let task_spec = task_spec(&options.command, options.dry_run);
    let event_session_id = session_id.clone();

    let automation_cfg = &run_vt_cfg.automation.full_auto;
    let mut runner = AgentRunner::new_with_bootstrap(
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
        Some(run_vt_cfg.clone()),
        run_config.openai_chatgpt_auth.clone(),
    )
    .await?;
    runner.set_active_primary_agent(active_primary_agent);

    if let Some(archive) = archive.as_ref() {
        let initial_session_messages = runner.session_messages();
        checkpoint_exec_archive(archive, &initial_session_messages).await?;
    }

    let allowed_tools = match &options.command {
        ExecCommandKind::Review { .. } => runner.review_tool_allowlist(&automation_cfg.allowed_tools).await,
        _ => automation_cfg.allowed_tools.clone(),
    };
    runner.enable_full_auto(&allowed_tools).await;
    if options.dry_run {
        runner.enable_planning();
    }
    runner.set_quiet(true);
    let events_path = effective_exec_events_path(
        options.events_path.as_deref(),
        run_vt_cfg.agent.harness.event_log_path.as_deref(),
        &event_session_id,
    );

    let processor = Arc::new(Mutex::new(ExecEventProcessor::<io::Stdout, BufWriter<File>, io::Stderr>::new(
        options.json,
        !options.json && !run_config.quiet,
        options.json.then(io::stdout),
        match events_path.as_ref() {
            Some(path) => Some(open_events_writer_async(path).await?),
            None => None,
        },
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
    let result = match runner
        .execute_task_with_retry(&task, &[] as &[ContextItem], max_retries)
        .await
        .context("Failed to execute autonomous task after retries")
    {
        Ok(result) => result,
        Err(error) => {
            let mut processor = lock_or_recover(&processor);
            processor.finish_events_output();
            if let Err(export_error) = processor.take_error() {
                tracing::warn!(target: "vtcode.exec", phase = "events_export", error = %export_error, "explicit exec events export failed after task failure");
            }
            return Err(error);
        }
    };

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
    let session_archive_messages: Vec<SessionMessage> = session_messages.iter().map(SessionMessage::from).collect();
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
    processor.take_error().context("Failed to process exec event output")?;

    Ok(())
}

async fn handle_codex_exec_command_impl(
    config: &CoreAgentConfig,
    vt_cfg: &VTCodeConfig,
    options: ExecCommandOptions,
) -> Result<()> {
    let prepared = prep::prepare_exec_run(config, vt_cfg, &options).await?;
    let prep::ExecPreparedRun {
        config: run_config,
        vt_cfg: run_vt_cfg,
        model_id: _,
        active_primary_agent,
        prompt,
        session_id,
        archive,
        thread_bootstrap,
    } = prepared;
    let task_spec = task_spec(&options.command, options.dry_run);

    apply_exec_retention(&run_config.workspace, &session_id).await;

    let review_target = native_review_target(&options.command, run_config.workspace.as_path()).await?;

    let events_path = effective_exec_events_path(
        options.events_path.as_deref(),
        run_vt_cfg.agent.harness.event_log_path.as_deref(),
        &session_id,
    );
    let mut event_processor = ExecEventProcessor::<io::Stdout, BufWriter<File>, io::Stderr>::new(
        false,
        false,
        None,
        match events_path.as_ref() {
            Some(path) => Some(open_events_writer_async(path).await?),
            None => None,
        },
        None,
    );
    let canonical = SessionStoreSink::open(&run_config.workspace, &session_id).await?;
    let thread_started = ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: session_id.clone() });
    emit_canonical_event(&canonical, &thread_started, "thread.started").await?;
    event_processor.process_event(&thread_started);
    let mut execution_context = vtcode_core::core::agent::events::ExecutionContextTracker::default();
    execution_context.begin(
        &session_id,
        &format!("turn-{}", uuid::Uuid::new_v4()),
        &prompt,
        vtcode_core::exec::events::InputOrigin::User,
    );
    let mut turn_started = ThreadEvent::TurnStarted(TurnStartedEvent::default());
    execution_context.annotate(&mut turn_started);
    emit_canonical_event(&canonical, &turn_started, "turn.started").await?;
    event_processor.process_event(&turn_started);

    let codex_result = run_codex_noninteractive_with_instructions(
        &run_config,
        Some(&run_vt_cfg),
        CodexNonInteractiveRun {
            prompt,
            read_only: options.dry_run || matches!(options.command, ExecCommandKind::Review { .. }),
            skip_confirmations: true,
            ephemeral: archive.is_none(),
            resume_thread_id: external_thread_id_from_bootstrap(&thread_bootstrap),
            seed_messages: thread_bootstrap.messages.iter().map(SessionMessage::from).collect(),
            review_target,
        },
        codex_exec_turn_instructions(&active_primary_agent, task_spec.instructions),
    )
    .await;

    let completed = match codex_result {
        Ok(completed) => {
            let mut turn_completed = ThreadEvent::TurnCompleted(TurnCompletedEvent {
                completed_at: None,
                usage: Usage::default(),
                in_progress_exec_sessions: Vec::new(),
            });
            execution_context.annotate(&mut turn_completed);
            emit_canonical_event(&canonical, &turn_completed, "turn.completed").await?;
            event_processor.process_event(&turn_completed);
            let thread_completed = ThreadEvent::ThreadCompleted(Box::new(ThreadCompletedEvent {
                thread_id: completed.thread_id.clone(),
                completed_at: Some(Utc::now().to_rfc3339()),
                session_id: session_id.clone(),
                subtype: ThreadCompletionSubtype::Success,
                outcome_code: "completed".to_string(),
                result: Some(completed.output.clone()),
                stop_reason: None,
                usage: Usage::default(),
                total_cost_usd: None,
                num_turns: 1,
            }));
            emit_canonical_event(&canonical, &thread_completed, "thread.completed").await?;
            event_processor.process_event(&thread_completed);
            let persistence_result = canonical.close().await.context("failed to finalize Codex session events");
            event_processor.finish_events_output();
            if let Err(error) = event_processor.take_error() {
                tracing::warn!(target: "vtcode.exec", phase = "events_export", error = %error, "explicit Codex events export failed");
            }
            persistence_result?;
            completed
        }
        Err(error) => {
            let mut turn_failed = ThreadEvent::TurnFailed(TurnFailedEvent {
                completed_at: None,
                message: error.to_string(),
                usage: None,
            });
            execution_context.annotate(&mut turn_failed);
            if let Err(event_error) = canonical.emit(&turn_failed) {
                tracing::error!(target: "vtcode.exec", error = %event_error, "failed to enqueue turn.failed canonical session event");
            }
            event_processor.process_event(&turn_failed);
            let thread_completed = ThreadEvent::ThreadCompleted(Box::new(ThreadCompletedEvent {
                thread_id: session_id.clone(),
                completed_at: Some(Utc::now().to_rfc3339()),
                session_id: session_id.clone(),
                subtype: ThreadCompletionSubtype::ErrorDuringExecution,
                outcome_code: "failed".to_string(),
                result: None,
                stop_reason: Some(error.to_string()),
                usage: Usage::default(),
                total_cost_usd: None,
                num_turns: 1,
            }));
            if let Err(event_error) = canonical.emit(&thread_completed) {
                tracing::error!(target: "vtcode.exec", error = %event_error, "failed to enqueue thread.completed canonical session event");
            }
            event_processor.process_event(&thread_completed);
            let persistence_result = canonical
                .close()
                .await
                .context("failed to finalize failed Codex session events");
            event_processor.finish_events_output();
            if let Err(export_error) = event_processor.take_error() {
                tracing::warn!(target: "vtcode.exec", phase = "events_export", error = %export_error, "explicit Codex events export failed");
            }
            if let Err(persistence_error) = persistence_result {
                return Err(
                    error.context(format!("failed to finalize failed Codex session events: {persistence_error}"))
                );
            }
            return Err(error);
        }
    };

    if let Some(path) = &options.last_message_file {
        write_file_with_context(path, completed.output.as_str(), "last message file").await?;
    }

    if options.json {
        let response = LLMResponse {
            content: Some(completed.output.clone()),
            model: run_config.model.clone(),
            tool_calls: None,
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            organization_id: None,
            request_id: None,
            tool_references: Vec::new(),
            compaction: None,
        };
        let payload = serde_json::json!({
            "response": response,
            "provider": {
                "kind": CODEX_PROVIDER,
                "model": run_config.model,
            },
            "threadId": completed.thread_id,
        });
        serde_json::to_writer_pretty(&mut io::stdout().lock(), &payload)?;
        println!();
    } else {
        println!("{}", completed.output);
    }

    if let Some(archive) = archive {
        archive
            .finalize(
                completed
                    .messages
                    .iter()
                    .map(|message| {
                        let role = match message.role {
                            vtcode_core::llm::provider::MessageRole::User => "user",
                            vtcode_core::llm::provider::MessageRole::Assistant => "assistant",
                            vtcode_core::llm::provider::MessageRole::System => "system",
                            vtcode_core::llm::provider::MessageRole::Tool => "tool",
                        };
                        format!("{role}: {}", message.content.as_text())
                    })
                    .collect(),
                completed.messages.len(),
                Vec::new(),
                completed.messages,
            )
            .context("Failed to save codex exec session archive")?;
    }

    Ok(())
}

fn codex_exec_turn_instructions(
    active_primary_agent: &vtcode_core::ActivePrimaryAgent,
    task_instructions: &str,
) -> Option<String> {
    let mut sections = Vec::new();
    let active_instructions = active_primary_agent.instructions.trim();
    if !active_instructions.is_empty() {
        sections.push(active_instructions);
    }
    let task_instructions = task_instructions.trim();
    if !task_instructions.is_empty() {
        sections.push(task_instructions);
    }

    (!sections.is_empty()).then(|| sections.join("\n\n"))
}

fn external_thread_id_from_bootstrap(bootstrap: &vtcode_core::core::threads::ThreadBootstrap) -> Option<String> {
    bootstrap
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.external_thread_id.clone())
        .or_else(|| {
            bootstrap
                .archive_listing
                .as_ref()
                .and_then(|listing| listing.snapshot.metadata.external_thread_id.clone())
        })
}

async fn native_review_target(command: &ExecCommandKind, workspace: &Path) -> Result<Option<CodexReviewTarget>> {
    let ExecCommandKind::Review { spec } = command else {
        return Ok(None);
    };

    // Style and free-form instructions need the full prompt review so the
    // model sees focus guidance; only bare targets use the native fast path.
    if spec.style.is_some() || spec.instructions.is_some() {
        return Ok(None);
    }

    Ok(match &spec.target {
        ReviewTarget::CurrentDiff => Some(CodexReviewTarget::UncommittedChanges),
        ReviewTarget::LastDiff => get_head_commit_hash_async(workspace.to_path_buf())
            .await?
            .map(|sha| CodexReviewTarget::Commit { sha, title: None }),
        ReviewTarget::Custom(target) => Some(CodexReviewTarget::Custom { instructions: target.clone() }),
        ReviewTarget::Files(_) => None,
    })
}

#[cfg(test)]
mod tests {
    use super::{checkpoint_exec_archive, native_review_target};
    use anyhow::{Context, Result};
    use chrono::Utc;
    use std::path::Path;
    use vtcode_core::llm::provider::Message;
    use vtcode_core::review::{ReviewSpec, ReviewTarget};
    use vtcode_core::utils::session_archive::{
        SessionArchive, SessionArchiveMetadata, SessionListing, SessionSnapshot,
    };

    #[tokio::test]
    #[expect(clippy::panic_in_result_fn, reason = "test function, assertions are expected")]
    async fn checkpoint_exec_archive_writes_initial_snapshot() -> Result<()> {
        let temp_dir = tempfile::tempdir().context("tempdir")?;
        let archive_path = temp_dir.path().join("session-vtcode-test-archive.json");
        let metadata = SessionArchiveMetadata::new("vtcode", "/tmp/vtcode", "gpt-5", "openai", "mono", "low");
        let listing = SessionListing {
            path: archive_path.clone(),
            snapshot: SessionSnapshot {
                metadata: metadata.clone(),
                started_at: Utc::now(),
                ended_at: Utc::now(),
                total_messages: 0,
                distinct_tools: Vec::new(),
                transcript: Vec::new(),
                messages: Vec::new(),
                progress: None,
                error_logs: Vec::new(),
            },
        };
        let archive = SessionArchive::resume_from_listing(&listing, metadata);

        let messages = vec![Message::user("hello".to_string())];
        checkpoint_exec_archive(&archive, &messages).await?;

        let snapshot: SessionSnapshot = serde_json::from_str(&std::fs::read_to_string(archive_path)?)?;
        assert_eq!(snapshot.total_messages, 1);
        assert_eq!(snapshot.messages.len(), 1);
        assert!(snapshot.progress.is_some());

        Ok(())
    }

    #[tokio::test]
    async fn native_review_target_uses_uncommitted_changes_when_style_is_absent() {
        let command = super::ExecCommandKind::Review {
            spec: ReviewSpec {
                target: ReviewTarget::CurrentDiff,
                style: None,
                instructions: None,
            },
        };

        let target = native_review_target(&command, Path::new("/tmp"))
            .await
            .expect("target resolution should succeed");

        assert_eq!(target, Some(super::CodexReviewTarget::UncommittedChanges));
    }

    #[tokio::test]
    async fn native_review_target_preserves_style_by_falling_back_to_prompt_review() {
        let command = super::ExecCommandKind::Review {
            spec: ReviewSpec {
                target: ReviewTarget::CurrentDiff,
                style: Some("security".to_string()),
                instructions: None,
            },
        };

        let target = native_review_target(&command, Path::new("/tmp"))
            .await
            .expect("target resolution should succeed");

        assert!(target.is_none());
    }

    #[tokio::test]
    async fn native_review_target_falls_back_when_instructions_present() {
        let command = super::ExecCommandKind::Review {
            spec: ReviewSpec {
                target: ReviewTarget::CurrentDiff,
                style: None,
                instructions: Some("Review the full diff for correctness".to_string()),
            },
        };

        let target = native_review_target(&command, Path::new("/tmp"))
            .await
            .expect("target resolution should succeed");

        assert!(target.is_none());
    }

    #[tokio::test]
    async fn native_review_target_maps_custom_review_targets() {
        let command = super::ExecCommandKind::Review {
            spec: ReviewSpec {
                target: ReviewTarget::Custom("HEAD~1..HEAD".to_string()),
                style: None,
                instructions: None,
            },
        };

        let target = native_review_target(&command, Path::new("/tmp"))
            .await
            .expect("target resolution should succeed");

        assert_eq!(target, Some(super::CodexReviewTarget::Custom { instructions: "HEAD~1..HEAD".to_string() }));
    }
}
