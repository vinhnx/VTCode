use anyhow::{Context, Result, bail};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use vtcode_core::cli::args::{ExecResumeArgs, ExecSubcommand};
use vtcode_core::cli::input_hardening::validate_agent_safe_text;
use vtcode_core::config::VTCodeConfig;
use vtcode_core::config::WorkspaceTrustLevel;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::config::models::ModelId;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::threads::{ThreadBootstrap, build_thread_archive_metadata};
use vtcode_core::review::{ReviewSpec, build_review_prompt};
use vtcode_core::utils::session_archive::{
    SessionArchive, SessionListing, find_session_by_identifier, list_recent_sessions,
    reserve_session_archive_identifier,
};
use vtcode_core::utils::tty::TtyExt;
use vtcode_core::utils::validation::validate_non_empty;

use crate::agent::agents::apply_runtime_overrides;
use crate::workspace_trust::workspace_trust_level;

use super::ExecCommandOptions;

#[derive(Debug, Clone)]
pub enum ExecCommandKind {
    Run {
        prompt_arg: Option<String>,
    },
    Resume {
        prompt_arg: Option<String>,
        session_id: Option<String>,
        last: bool,
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
    pub archive: SessionArchive,
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
        });
    }

    Ok(ExecCommandKind::Resume {
        prompt_arg: resume.prompt,
        session_id: resume.session_or_prompt,
        last: false,
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
            Some(resolve_resume_listing(options).await?),
        ),
        ExecCommandKind::Review { spec } => (build_review_prompt(spec), None),
    };

    let mut run_config = config.clone();
    let run_workspace = if let Some(listing) = &resume_listing {
        resolve_resume_workspace(listing)?
    } else {
        config.workspace.clone()
    };
    run_config.workspace = run_workspace.clone();

    let mut run_vt_cfg = load_exec_vt_config(vt_cfg, &run_workspace, &config.workspace).await?;
    apply_runtime_overrides(Some(&mut run_vt_cfg), &run_config);

    let trust_level = workspace_trust_level(&run_config.workspace)
        .await
        .context("Failed to determine workspace trust level")?;

    match trust_level {
        Some(WorkspaceTrustLevel::FullAuto) => {}
        Some(level) => {
            bail!(
                "Workspace trust level '{level}' does not permit exec runs. Upgrade trust to full auto."
            );
        }
        None => {
            bail!(
                "Workspace is not trusted. Start vtcode interactively once and mark it as full auto before using exec."
            );
        }
    }

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

    let (session_id, archive, thread_bootstrap) = if let Some(listing) = resume_listing {
        let session_id = listing.identifier();
        let archive = SessionArchive::resume_from_listing(&listing, metadata.clone());
        let mut bootstrap = ThreadBootstrap::from_listing(listing);
        bootstrap.metadata = Some(metadata);
        (session_id, archive, bootstrap)
    } else {
        let workspace_label = exec_workspace_label(run_workspace.as_path());
        let session_id = reserve_session_archive_identifier(&workspace_label, None)
            .await
            .context("Failed to reserve exec session archive identifier")?;
        let archive = SessionArchive::new_with_identifier(metadata.clone(), session_id.clone())
            .await
            .context("Failed to create exec session archive")?;
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
        Some(p) if p != "-" => p,
        maybe_dash => {
            let force_stdin = matches!(maybe_dash.as_deref(), Some("-"));
            if io::stdin().is_tty_ext() && !force_stdin {
                bail!(
                    "No prompt provided. Pass a prompt argument, pipe input, or use '-' to read from stdin."
                );
            }
            if !force_stdin && !quiet {
                eprintln!("Reading prompt from stdin...");
            }
            let mut buffer = String::with_capacity(1024);
            io::stdin()
                .read_to_string(&mut buffer)
                .context("Failed to read prompt from stdin")?;
            validate_non_empty(&buffer, "Prompt via stdin")?;
            buffer
        }
    };

    validate_agent_safe_text("prompt", &prompt)?;
    Ok(prompt)
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

async fn resolve_resume_listing(options: &ExecCommandOptions) -> Result<SessionListing> {
    let ExecCommandKind::Resume {
        session_id, last, ..
    } = &options.command
    else {
        bail!("Internal error: resume listing requested for non-resume exec command");
    };

    if *last {
        return list_recent_sessions(1)
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
    use super::{ExecCommandKind, resolve_exec_command, validate_resume_prompt_requirement};
    use vtcode_core::cli::args::{ExecResumeArgs, ExecSubcommand};
    use vtcode_core::review::{ReviewTarget, build_review_spec};

    #[test]
    fn resolve_resume_last_uses_first_positional_as_prompt() {
        let command = resolve_exec_command(
            Some(ExecSubcommand::Resume(ExecResumeArgs {
                last: true,
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
            } if prompt == "continue"
        ));
    }

    #[test]
    fn resolve_resume_last_rejects_second_positional() {
        let err = resolve_exec_command(
            Some(ExecSubcommand::Resume(ExecResumeArgs {
                last: true,
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
}
