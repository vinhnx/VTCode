use crate::startup::SessionResumeMode;
use anyhow::Result;
use vtcode_core::cli::args::AskCommandOptions;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::interfaces::session::PlanModeEntrySource;

use crate::cli::{analyze, sessions};

pub(crate) async fn handle_ask_single_command(
    core_cfg: CoreAgentConfig,
    vt_cfg: Option<VTCodeConfig>,
    prompt: Option<String>,
    options: AskCommandOptions,
) -> Result<()> {
    let prompt_vec = prompt.into_iter().collect::<Vec<_>>();
    if core_cfg
        .provider
        .eq_ignore_ascii_case(crate::codex_app_server::CODEX_PROVIDER)
    {
        crate::codex_app_server::handle_codex_ask_command(
            core_cfg,
            prompt_vec,
            vt_cfg.as_ref(),
            options,
        )
        .await
    } else {
        vtcode_core::commands::ask::handle_ask_command(core_cfg, prompt_vec, options).await
    }
}

pub(crate) async fn handle_chat_command(
    core_cfg: CoreAgentConfig,
    vt_cfg: VTCodeConfig,
    skip_confirmations: bool,
    full_auto_requested: bool,
    plan_mode_entry_source: PlanModeEntrySource,
) -> Result<()> {
    crate::agent::agents::run_single_agent_loop(
        &core_cfg,
        Some(vt_cfg),
        skip_confirmations,
        full_auto_requested,
        plan_mode_entry_source,
        None,
    )
    .await
}

pub(super) async fn handle_analyze_command(
    core_cfg: CoreAgentConfig,
    vt_cfg: Option<VTCodeConfig>,
    analysis_type: analyze::AnalysisType,
) -> Result<()> {
    if core_cfg
        .provider
        .eq_ignore_ascii_case(crate::codex_app_server::CODEX_PROVIDER)
    {
        let prompt = codex_analyze_prompt(&analysis_type);
        let completed = crate::codex_app_server::run_codex_noninteractive(
            &core_cfg,
            vt_cfg.as_ref(),
            crate::codex_app_server::CodexNonInteractiveRun {
                prompt,
                read_only: true,
                plan_mode: false,
                skip_confirmations: true,
                ephemeral: true,
                resume_thread_id: None,
                seed_messages: Vec::new(),
                review_target: None,
            },
        )
        .await?;
        println!("{}", completed.output);
        return Ok(());
    }

    vtcode_core::commands::analyze::handle_analyze_command(
        core_cfg,
        analysis_type.default_depth().to_string(),
        "text".to_string(),
    )
    .await
}

fn codex_analyze_prompt(analysis_type: &analyze::AnalysisType) -> String {
    let focus = match analysis_type {
        analyze::AnalysisType::Full => {
            "architecture, main subsystems, risks, and the most important next investigation areas"
        }
        analyze::AnalysisType::Structure => {
            "project structure, entrypoints, crate/module boundaries, and code organization"
        }
        analyze::AnalysisType::Security => {
            "security-relevant trust boundaries, dangerous operations, auth, and likely security gaps"
        }
        analyze::AnalysisType::Performance => {
            "performance-sensitive paths, avoidable work, I/O hotspots, and likely bottlenecks"
        }
        analyze::AnalysisType::Dependencies => {
            "dependency shape, integration boundaries, and notable external coupling"
        }
        analyze::AnalysisType::Complexity => {
            "complex control flow, high-risk modules, and areas likely to be hard to change safely"
        }
    };

    format!(
        "Analyze the current workspace in read-only mode. Focus on {focus}. Ground the answer in the repository, include concise file references when useful, and do not modify files or request additional user input."
    )
}

pub(crate) async fn handle_resume_session_command(
    core_cfg: &CoreAgentConfig,
    mode: SessionResumeMode,
    show_all: bool,
    custom_session_id: Option<String>,
    summarize_fork: bool,
    skip_confirmations: bool,
) -> Result<()> {
    sessions::handle_resume_session_command(
        core_cfg,
        mode,
        show_all,
        custom_session_id,
        summarize_fork,
        skip_confirmations,
    )
    .await
}
