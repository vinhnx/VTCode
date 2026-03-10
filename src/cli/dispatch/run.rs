use crate::startup::SessionResumeMode;
use anyhow::Result;
use vtcode_core::cli::args::AskCommandOptions;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;

use crate::cli::{analyze, sessions};

pub(crate) async fn handle_ask_single_command(
    core_cfg: CoreAgentConfig,
    prompt: Option<String>,
    options: AskCommandOptions,
) -> Result<()> {
    let prompt_vec = prompt.into_iter().collect::<Vec<_>>();
    vtcode_core::commands::ask::handle_ask_command(core_cfg, prompt_vec, options).await
}

pub(crate) async fn handle_chat_command(
    core_cfg: CoreAgentConfig,
    vt_cfg: VTCodeConfig,
    skip_confirmations: bool,
    full_auto_requested: bool,
    plan_mode: bool,
) -> Result<()> {
    crate::agent::agents::run_single_agent_loop(
        &core_cfg,
        Some(vt_cfg),
        skip_confirmations,
        full_auto_requested,
        plan_mode,
        None,
    )
    .await
}

pub(super) async fn handle_analyze_command(
    core_cfg: CoreAgentConfig,
    analysis_type: analyze::AnalysisType,
) -> Result<()> {
    vtcode_core::commands::analyze::handle_analyze_command(
        core_cfg,
        analysis_type.default_depth().to_string(),
        "text".to_string(),
    )
    .await
}

pub(crate) async fn handle_resume_session_command(
    core_cfg: &CoreAgentConfig,
    mode: SessionResumeMode,
    show_all: bool,
    custom_session_id: Option<String>,
    skip_confirmations: bool,
) -> Result<()> {
    sessions::handle_resume_session_command(
        core_cfg,
        mode,
        show_all,
        custom_session_id,
        skip_confirmations,
    )
    .await
}
