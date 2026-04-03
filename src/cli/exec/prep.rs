use anyhow::{Context, Result, bail};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use vtcode_core::cli::args::{ExecResumeArgs, ExecSubcommand};
use vtcode_core::cli::input_hardening::validate_agent_safe_text;
use vtcode_core::config::VTCodeConfig;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::config::models::ModelId;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::threads::{
    ArchivedSessionIntent, SessionQueryScope, ThreadBootstrap, build_thread_archive_metadata,
    list_recent_sessions_in_scope, prepare_archived_session,
};
use vtcode_core::review::{ReviewSpec, build_review_prompt};
use vtcode_core::utils::session_archive::{
    SessionArchive, SessionListing, find_session_by_identifier,
    generate_session_archive_identifier, history_persistence_enabled,
    reserve_session_archive_identifier,
};
use vtcode_core::utils::tty::TtyExt;
use vtcode_core::utils::validation::validate_non_empty;

use crate::agent::agents::{apply_persisted_resume_metadata, apply_runtime_overrides};
use crate::startup::require_full_auto_workspace_trust;

use super::super::exec::ExecCommandOptions;

enum StdinPromptBehavior {
    RequiredIfPiped,
    Forced,
    OptionalAppend,
}

#[derive(Debug, Clone)]
pub enum ExecCommandKind {
    Run {
        prompt_arg: Option<String>,
    },
    Resume {
        prompt_arg: Option<String>,
        session_id: Option<String>,
        last: bool,
        all: bool,
    },
    Review {
        spec: ReviewSpec,
    },
}

pub(super) struct ExecPreparedRun {
    pub config: CoreAgentConfig,
    pub vt_cfg: VTCodeConfig,
    pub model_id: ModelId,
    pub prompt: String,
    pub session_id: String,
    pub archive: Option<SessionArchive>,
    pub thread_bootstrap: ThreadBootstrap,
}

pub(crate) fn resolve_exec_command(
    command: Option<ExecSubcommand>,
    prompt: Option<String>,
) -> Result<ExecCommandKind> {
    match command {
        Some(ExecSubcommand::Resume(resume)) => resolve_resume_command(resume),
        None => Ok(ExecCommandKind::Run { prompt_arg: prompt }),
    }
}

fn resolve_resume_command(resume: ExecResumeArgs) -> Result<ExecCommandKind> {
    if resume.last {
        if resume.prompt.is_some() {
            bail!(
                "Exec resume --last accepts a single prompt argument. Quote multi-word prompts or pipe stdin."
            );
        }

        return Ok(ExecCommandKind::Resume {
            prompt_arg: resume.session_or_prompt,
            session_id: None,
            last: true,
            all: resume.all,
        });
    }

    Ok(ExecCommandKind::Resume {
        prompt_arg: resume.prompt,
        session_id: resume.session_or_prompt,
        last: false,
        all: resume.all,
    })
}

pub(super) async fn prepare_exec_run(
    config: &CoreAgentConfig,
    vt_cfg: &VTCodeConfig,
    options: &ExecCommandOptions,
) -> Result<ExecPreparedRun> {
    validate_resume_prompt_requirement(&options.command, io::stdin().is_tty_ext())?;

    let (prompt, resume_listing) = match &options.command {
        ExecCommandKind::Run { prompt_arg } => {
            (resolve_prompt(prompt_arg.clone(), config.quiet)?, None)
        }
        ExecCommandKind::Resume { prompt_arg, .. } => (
            resolve_prompt(prompt_arg.clone(), config.quiet)?,
            Some(resolve_resume_listing(options, config).await?),
        ),
        ExecCommandKind::Review { spec } => (build_review_prompt(spec), None),
    };

    let mut run_config = config.clone();
    let run_workspace = if let Some(listing) = &resume_listing {
        apply_persisted_resume_metadata(&mut run_config, Some(&listing.snapshot.metadata));
        resolve_resume_workspace(listing)?
    } else {
        config.workspace.clone()
    };
    run_config.workspace = run_workspace.clone();

    let mut run_vt_cfg = load_exec_vt_config(vt_cfg, &run_workspace, &config.workspace).await?;
    apply_runtime_overrides(Some(&mut run_vt_cfg), &run_config);

    require_full_auto_workspace_trust(&run_config.workspace, "exec runs", "exec").await?;

    let automation_cfg = &run_vt_cfg.automation.full_auto;
    if !automation_cfg.enabled {
        bail!(
            "Automation is disabled in configuration. Enable [automation.full_auto] to continue."
        );
    }

    let model_id = ModelId::from_str(&run_config.model).with_context(|| {
        format!(
            "Model '{}' is not recognized for exec command. Update vtcode.toml to a supported identifier.",
            run_config.model
        )
    })?;

    let metadata =
        build_exec_archive_metadata(run_workspace.as_path(), &model_id, &run_vt_cfg, &run_config);
    let history_enabled = history_persistence_enabled();
    let reserved_archive_id = crate::main_helpers::runtime_archive_session_id();

    let (session_id, archive, thread_bootstrap) = if let Some(listing) = resume_listing {
        if history_enabled {
            let prepared = prepare_archived_session(
                listing,
                run_workspace.clone(),
                metadata.clone(),
                ArchivedSessionIntent::ResumeInPlace,
                None,
            )
            .await
            .context("Failed to prepare archived exec session")?;
            (
                prepared.thread_id,
                Some(prepared.archive),
                prepared.bootstrap,
            )
        } else {
            let session_id = listing.identifier();
            let mut bootstrap = ThreadBootstrap::from_listing(listing);
            bootstrap.metadata = Some(metadata.clone());
            (session_id, None, bootstrap)
        }
    } else {
        let session_id = next_exec_session_id(
            run_workspace.as_path(),
            reserved_archive_id,
            history_enabled,
        )
        .await?;
        let archive = if history_enabled {
            Some(
                SessionArchive::new_with_identifier(metadata.clone(), session_id.clone())
                    .await
                    .context("Failed to create exec session archive")?,
            )
        } else {
            None
        };
        let bootstrap = ThreadBootstrap::new(Some(metadata));
        (session_id, archive, bootstrap)
    };

    Ok(ExecPreparedRun {
        config: run_config,
        vt_cfg: run_vt_cfg,
        model_id,
        prompt,
        session_id,
        archive,
        thread_bootstrap,
    })
}

pub(super) fn validate_resume_prompt_requirement(
    command: &ExecCommandKind,
    stdin_is_tty: bool,
) -> Result<()> {
    if let ExecCommandKind::Resume { prompt_arg, .. } = command
        && prompt_arg.is_none()
        && stdin_is_tty
    {
        bail!(
            "Exec resume requires a follow-up prompt. Pass one explicitly, use '-' to read stdin, or pipe input."
        );
    }

    Ok(())
}

fn resolve_prompt(prompt_arg: Option<String>, quiet: bool) -> Result<String> {
    let prompt = match prompt_arg {
        Some(prompt) if prompt != "-" => {
            if let Some(stdin_text) =
                read_prompt_from_stdin(StdinPromptBehavior::OptionalAppend, quiet)?
            {
                prompt_with_stdin_context(&prompt, &stdin_text)
            } else {
                prompt
            }
        }
        maybe_dash => {
            let behavior = if matches!(maybe_dash.as_deref(), Some("-")) {
                StdinPromptBehavior::Forced
            } else {
                StdinPromptBehavior::RequiredIfPiped
            };
            read_prompt_from_stdin(behavior, quiet)?
                .expect("required stdin prompt should produce content")
        }
    };

    validate_agent_safe_text("prompt", &prompt)?;
    Ok(prompt)
}

fn read_prompt_from_stdin(behavior: StdinPromptBehavior, quiet: bool) -> Result<Option<String>> {
    let stdin_is_tty = io::stdin().is_tty_ext();

    match behavior {
        StdinPromptBehavior::RequiredIfPiped if stdin_is_tty => {
            bail!(
                "No prompt provided. Pass a prompt argument, pipe input, or use '-' to read from stdin."
            );
        }
        StdinPromptBehavior::RequiredIfPiped => {
            if !quiet {
                eprintln!("Reading prompt from stdin...");
            }
        }
        StdinPromptBehavior::Forced => {}
        StdinPromptBehavior::OptionalAppend if stdin_is_tty => return Ok(None),
        StdinPromptBehavior::OptionalAppend => {
            if !quiet {
                eprintln!("Reading additional input from stdin...");
            }
        }
    }

    let mut buffer = String::with_capacity(1024);
    io::stdin()
        .read_to_string(&mut buffer)
        .context("Failed to read prompt from stdin")?;

    if buffer.trim().is_empty() {
        return match behavior {
            StdinPromptBehavior::OptionalAppend => Ok(None),
            StdinPromptBehavior::RequiredIfPiped | StdinPromptBehavior::Forced => {
                validate_non_empty(&buffer, "Prompt via stdin")?;
                Ok(None)
            }
        };
    }

    Ok(Some(buffer))
}

fn prompt_with_stdin_context(prompt: &str, stdin_text: &str) -> String {
    let mut combined = format!("{prompt}\n\n<stdin>\n{stdin_text}");
    if !stdin_text.ends_with('\n') {
        combined.push('\n');
    }
    combined.push_str("</stdin>");
    combined
}

fn exec_workspace_label(workspace: &Path) -> String {
    workspace
        .file_name()
        .and_then(|component| component.to_str())
        .map(|value| value.to_string())
        .unwrap_or_else(|| "workspace".to_string())
}

fn resolve_resume_workspace(listing: &SessionListing) -> Result<PathBuf> {
    let workspace = listing.snapshot.metadata.workspace_path.trim();
    if workspace.is_empty() {
        bail!(
            "Archived exec session '{}' is missing workspace metadata.",
            listing.identifier()
        );
    }

    let path = PathBuf::from(workspace);
    if !path.exists() {
        bail!(
            "Archived exec workspace '{}' no longer exists on disk.",
            path.display()
        );
    }

    Ok(path)
}

fn build_exec_archive_metadata(
    workspace: &Path,
    model_id: &ModelId,
    vt_cfg: &VTCodeConfig,
    config: &CoreAgentConfig,
) -> vtcode_core::utils::session_archive::SessionArchiveMetadata {
    build_thread_archive_metadata(
        workspace,
        model_id.as_str(),
        &vt_cfg.agent.provider,
        &vt_cfg.agent.theme,
        config.reasoning_effort.as_str(),
    )
    .with_debug_log_path(
        crate::main_helpers::runtime_debug_log_path()
            .map(|path| path.to_string_lossy().to_string()),
    )
}

async fn next_exec_session_id(
    workspace: &Path,
    reserved_archive_id: Option<String>,
    history_enabled: bool,
) -> Result<String> {
    if let Some(identifier) = reserved_archive_id {
        return Ok(identifier);
    }

    let workspace_label = exec_workspace_label(workspace);
    if history_enabled {
        reserve_session_archive_identifier(&workspace_label, None)
            .await
            .context("Failed to reserve exec session archive identifier")
    } else {
        Ok(generate_session_archive_identifier(&workspace_label, None))
    }
}

async fn load_exec_vt_config(
    base: &VTCodeConfig,
    workspace: &Path,
    original_workspace: &Path,
) -> Result<VTCodeConfig> {
    if workspace == original_workspace {
        return Ok(base.clone());
    }

    let manager = ConfigManager::load_from_workspace(workspace).with_context(|| {
        format!(
            "Failed to load VT Code configuration for archived workspace '{}'",
            workspace.display()
        )
    })?;
    Ok(manager.config().clone())
}

async fn resolve_resume_listing(
    options: &ExecCommandOptions,
    config: &CoreAgentConfig,
) -> Result<SessionListing> {
    let ExecCommandKind::Resume {
        session_id,
        last,
        all,
        ..
    } = &options.command
    else {
        bail!("Internal error: resume listing requested for non-resume exec command");
    };

    if *last {
        let scope = if *all {
            SessionQueryScope::All
        } else {
            SessionQueryScope::CurrentWorkspace(config.workspace.clone())
        };
        return list_recent_sessions_in_scope(1, &scope)
            .await?
            .into_iter()
            .next()
            .context("No archived exec sessions were found.");
    }

    let identifier = session_id
        .as_deref()
        .context("Session id is required when --last is not used.")?;
    find_session_by_identifier(identifier)
        .await?
        .with_context(|| {
            format!("No archived exec session with identifier '{identifier}' was found.")
        })
}

#[cfg(test)]
mod tests {
    use super::{
        ExecCommandKind, build_exec_archive_metadata, next_exec_session_id,
        prompt_with_stdin_context, resolve_exec_command, validate_resume_prompt_requirement,
    };
    use std::path::{Path, PathBuf};
    use std::str::FromStr;
    use vtcode_core::cli::args::{ExecResumeArgs, ExecSubcommand};
    use vtcode_core::config::loader::VTCodeConfig;
    use vtcode_core::config::models::ModelId;
    use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
    use vtcode_core::review::{ReviewTarget, build_review_spec};
    use vtcode_core::utils::session_debug::{
        configure_runtime_debug_context, runtime_debug_log_path, set_runtime_debug_log_path,
    };

    #[test]
    fn resolve_resume_last_uses_first_positional_as_prompt() {
        let command = resolve_exec_command(
            Some(ExecSubcommand::Resume(ExecResumeArgs {
                last: true,
                all: false,
                session_or_prompt: Some("continue".to_string()),
                prompt: None,
            })),
            None,
        )
        .expect("resume command should resolve");

        assert!(matches!(
            command,
            ExecCommandKind::Resume {
                last: true,
                session_id: None,
                prompt_arg: Some(ref prompt),
                all: _,
            } if prompt == "continue"
        ));
    }

    #[test]
    fn resolve_resume_last_rejects_second_positional() {
        let err = resolve_exec_command(
            Some(ExecSubcommand::Resume(ExecResumeArgs {
                last: true,
                all: false,
                session_or_prompt: Some("continue".to_string()),
                prompt: Some("extra".to_string()),
            })),
            None,
        )
        .expect_err("second positional should be rejected");

        assert!(err.to_string().contains("accepts a single prompt argument"));
    }

    #[test]
    fn resume_without_prompt_is_rejected_on_tty() {
        let err = validate_resume_prompt_requirement(
            &ExecCommandKind::Resume {
                prompt_arg: None,
                session_id: Some("session-1".to_string()),
                last: false,
                all: false,
            },
            true,
        )
        .expect_err("resume without prompt should fail");

        assert!(
            err.to_string()
                .contains("Exec resume requires a follow-up prompt")
        );
    }

    #[test]
    fn review_command_does_not_require_resume_prompt() {
        let spec = build_review_spec(false, None, Vec::new(), None).expect("review spec");
        let result = validate_resume_prompt_requirement(
            &ExecCommandKind::Review { spec: spec.clone() },
            true,
        );

        assert!(result.is_ok());
        assert!(matches!(spec.target, ReviewTarget::CurrentDiff));
    }

    #[test]
    fn prompt_with_stdin_context_wraps_stdin_block() {
        let combined = prompt_with_stdin_context("Summarize this concisely", "my output");

        assert_eq!(
            combined,
            "Summarize this concisely\n\n<stdin>\nmy output\n</stdin>"
        );
    }

    #[test]
    fn prompt_with_stdin_context_preserves_trailing_newline() {
        let combined = prompt_with_stdin_context("Summarize this concisely", "my output\n");

        assert_eq!(
            combined,
            "Summarize this concisely\n\n<stdin>\nmy output\n</stdin>"
        );
    }

    #[tokio::test]
    async fn next_exec_session_id_prefers_reserved_identifier() {
        let session_id =
            next_exec_session_id(Path::new("."), Some("session-reserved".to_string()), true)
                .await
                .expect("reserved session id should win");

        assert_eq!(session_id, "session-reserved");
    }

    #[test]
    fn build_exec_archive_metadata_includes_runtime_debug_log_path() {
        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.provider = "openai".to_string();
        vt_cfg.agent.theme = "mono".to_string();

        let config = CoreAgentConfig {
            model: "gpt-5".to_string(),
            api_key: "test-key".to_string(),
            provider: "openai".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            workspace: PathBuf::from("."),
            verbose: false,
            quiet: false,
            theme: "mono".to_string(),
            reasoning_effort: Default::default(),
            ui_surface: Default::default(),
            prompt_cache: Default::default(),
            model_source: Default::default(),
            custom_api_keys: Default::default(),
            checkpointing_enabled: true,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: 50,
            checkpointing_max_age_days: Some(30),
            max_conversation_turns: 1000,
            model_behavior: None,
            openai_chatgpt_auth: None,
        };

        configure_runtime_debug_context("debug-session".to_string(), Some("session-1".to_string()));
        let path = PathBuf::from("/tmp/debug-session.log");
        set_runtime_debug_log_path(&path);
        let model_id = ModelId::from_str("gpt-5").expect("model id");
        let metadata = build_exec_archive_metadata(Path::new("."), &model_id, &vt_cfg, &config);

        assert_eq!(runtime_debug_log_path().as_deref(), Some(path.as_path()));
        assert_eq!(
            metadata.debug_log_path.as_deref(),
            Some("/tmp/debug-session.log")
        );
    }
}
