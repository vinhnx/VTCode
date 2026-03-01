use std::path::{Path, PathBuf};

use anyhow::Result;
use vtcode_core::config::loader::VTCodeConfig;

mod acp;
mod skills_index;

/// Skills command options
#[derive(Debug)]
pub struct SkillsCommandOptions {
    pub workspace: PathBuf,
}

// Re-export the core CLI functions we need
pub use vtcode_core::mcp::cli::handle_mcp_command;

pub mod analyze;
pub mod benchmark;
pub mod exec;
pub mod skills;
pub mod skills_ref;
pub mod update;

pub use vtcode_core::cli::args::AskCommandOptions;

pub use benchmark::BenchmarkCommandOptions;
pub use exec::ExecCommandOptions;

mod sessions;
mod snapshots;

// Re-export the handle_acp_command from acp module
pub use self::acp::handle_acp_command;

// For the other functions, we'll use proper implementations that match the expected signatures

pub async fn handle_ask_single_command(
    core_cfg: vtcode_core::config::types::AgentConfig,
    prompt: Option<String>,
    _options: AskCommandOptions,
) -> Result<()> {
    let prompt_vec = if let Some(p) = prompt {
        vec![p]
    } else {
        vec![]
    };
    vtcode_core::commands::ask::handle_ask_command(core_cfg, prompt_vec, _options).await
}

pub async fn handle_chat_command(
    core_cfg: vtcode_core::config::types::AgentConfig,
    skip_confirmations: bool,
    full_auto_requested: bool,
    plan_mode: bool,
    team_context: Option<vtcode_core::agent_teams::TeamContext>,
) -> Result<()> {
    crate::agent::agents::run_single_agent_loop(
        &core_cfg,
        skip_confirmations,
        full_auto_requested,
        plan_mode,
        team_context,
        None,
    )
    .await
}

pub async fn handle_exec_command(
    core_cfg: vtcode_core::config::types::AgentConfig,
    cfg: &VTCodeConfig,
    options: ExecCommandOptions,
    prompt: Option<String>,
) -> Result<()> {
    exec::handle_exec_command(&core_cfg, cfg, options, prompt).await
}

pub async fn handle_analyze_command(
    core_cfg: vtcode_core::config::types::AgentConfig,
    analysis_type: analyze::AnalysisType,
) -> Result<()> {
    // Convert AnalysisType to string for the actual handler
    let depth = match analysis_type {
        analyze::AnalysisType::Full
        | analyze::AnalysisType::Structure
        | analyze::AnalysisType::Complexity => "deep",
        analyze::AnalysisType::Security
        | analyze::AnalysisType::Performance
        | analyze::AnalysisType::Dependencies => "standard",
    };

    // Use "text" as default format
    let format = "text";

    vtcode_core::commands::analyze::handle_analyze_command(
        core_cfg,
        depth.to_string(),
        format.to_string(),
    )
    .await
}

pub async fn handle_trajectory_logs_command(
    _core_cfg: vtcode_core::config::types::AgentConfig,
    _file: Option<PathBuf>,
    _top: Option<usize>,
) -> Result<()> {
    Err(anyhow::anyhow!(
        "Trajectory logs command not implemented in this stub"
    ))
}

pub async fn handle_create_project_command(
    _core_cfg: vtcode_core::config::types::AgentConfig,
    _name: &str,
    _features: &[String],
) -> Result<()> {
    Err(anyhow::anyhow!(
        "Create project command not implemented in this stub"
    ))
}

pub async fn handle_revert_command(
    _core_cfg: vtcode_core::config::types::AgentConfig,
    _turn: usize,
    _partial: Option<String>,
) -> Result<()> {
    Err(anyhow::anyhow!(
        "Revert command not implemented in this stub"
    ))
}

pub async fn handle_snapshots_command(
    core_cfg: vtcode_core::config::types::AgentConfig,
) -> Result<()> {
    snapshots::handle_snapshots_command(&core_cfg).await
}

pub async fn handle_cleanup_snapshots_command(
    core_cfg: vtcode_core::config::types::AgentConfig,
    max_snapshots: Option<usize>,
) -> Result<()> {
    snapshots::handle_cleanup_snapshots_command(&core_cfg, max_snapshots).await
}

pub async fn handle_init_command(_workspace: &PathBuf, _force: bool, _migrate: bool) -> Result<()> {
    Err(anyhow::anyhow!("Init command not implemented in this stub"))
}

pub async fn handle_config_command(_output: Option<PathBuf>, _global: bool) -> Result<()> {
    Err(anyhow::anyhow!(
        "Config command not implemented in this stub"
    ))
}

pub async fn handle_init_project_command(
    _name: Option<String>,
    _force: bool,
    _migrate: bool,
) -> Result<()> {
    Err(anyhow::anyhow!(
        "Init project command not implemented in this stub"
    ))
}

pub async fn handle_benchmark_command(
    core_cfg: vtcode_core::config::types::AgentConfig,
    cfg: &VTCodeConfig,
    options: BenchmarkCommandOptions,
    full_auto_requested: bool,
) -> Result<()> {
    benchmark::handle_benchmark_command(&core_cfg, cfg, options, full_auto_requested).await
}

pub async fn handle_man_command(_command: Option<String>, _output: Option<PathBuf>) -> Result<()> {
    Err(anyhow::anyhow!("Man command not implemented in this stub"))
}

pub async fn handle_resume_session_command(
    core_cfg: &vtcode_core::config::types::AgentConfig,
    mode: vtcode::startup::SessionResumeMode,
    custom_session_id: Option<String>,
    skip_confirmations: bool,
) -> Result<()> {
    sessions::handle_resume_session_command(core_cfg, mode, custom_session_id, skip_confirmations)
        .await
}

pub async fn handle_skills_list(skills_options: &SkillsCommandOptions) -> Result<()> {
    // Import and delegate to the actual implementation in the skills module
    use crate::cli::skills::handle_skills_list as actual_handler;
    actual_handler(skills_options).await
}

pub async fn handle_skills_load(
    skills_options: &SkillsCommandOptions,
    name: &str,
    path: PathBuf,
) -> Result<()> {
    // Import and delegate to the actual implementation in the skills module
    use crate::cli::skills::handle_skills_load as actual_handler;
    actual_handler(skills_options, name, Some(path)).await
}

pub async fn handle_skills_info(skills_options: &SkillsCommandOptions, name: &str) -> Result<()> {
    // Import and delegate to the actual implementation in the skills module
    use crate::cli::skills::handle_skills_info as actual_handler;
    actual_handler(skills_options, name).await
}

pub async fn handle_skills_create(path: &Path) -> Result<()> {
    // Import and delegate to the actual implementation in the skills module
    use crate::cli::skills::handle_skills_create as actual_handler;
    actual_handler(path).await
}

pub async fn handle_skills_validate(path: &Path) -> Result<()> {
    // Import and delegate to the actual implementation in the skills module
    use crate::cli::skills::handle_skills_validate as actual_handler;
    actual_handler(path).await
}

pub async fn handle_skills_validate_all(skills_options: &SkillsCommandOptions) -> Result<()> {
    // Import and delegate to the actual implementation in the skills module
    use crate::cli::skills::handle_skills_validate_all as actual_handler;
    actual_handler(skills_options).await
}

pub async fn handle_skills_config(skills_options: &SkillsCommandOptions) -> Result<()> {
    // Import and delegate to the actual implementation in the skills module
    use crate::cli::skills::handle_skills_config as actual_handler;
    actual_handler(skills_options).await
}

pub async fn handle_skills_regenerate_index(skills_options: &SkillsCommandOptions) -> Result<()> {
    // Import and delegate to the actual implementation in the skills module
    use crate::cli::skills::handle_skills_regenerate_index as actual_handler;
    actual_handler(skills_options).await
}

pub async fn handle_auto_task_command(
    _core_cfg: &vtcode_core::config::types::AgentConfig,
    _cfg: &VTCodeConfig,
    _prompt: &str,
) -> Result<()> {
    Err(anyhow::anyhow!(
        "Auto task command not implemented in this stub"
    ))
}

pub fn set_workspace_env(workspace: &PathBuf) {
    unsafe {
        std::env::set_var("VTCODE_WORKSPACE", workspace);
    }
}

pub fn set_additional_dirs_env(additional_dirs: &[PathBuf]) {
    let dirs_str = additional_dirs
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(":");
    unsafe {
        std::env::set_var("VTCODE_ADDITIONAL_DIRS", dirs_str);
    }
}

#[cfg(feature = "anthropic-api")]
pub async fn handle_anthropic_api_command(
    core_cfg: vtcode_core::config::types::AgentConfig,
    port: u16,
    host: String,
) -> Result<()> {
    use std::net::SocketAddr;
    use vtcode_core::anthropic_api::server::{AnthropicApiServerState, create_router};

    // Create the LLM provider based on the configuration
    let provider = vtcode_core::llm::factory::create_provider_for_model(
        &core_cfg.model,
        core_cfg.api_key.clone(),
        None,
    )
    .map_err(|e| anyhow::anyhow!("Failed to create LLM provider: {}", e))?;

    // Create server state with the provider
    let state =
        AnthropicApiServerState::new(std::sync::Arc::from(provider), core_cfg.model.clone());

    // Create the router
    let app = create_router(state);

    // Bind to the specified address
    let addr = format!("{}:{}", host, port)
        .parse::<SocketAddr>()
        .map_err(|e| anyhow::anyhow!("Invalid address {}: {}", format!("{}:{}", host, port), e))?;

    println!("Anthropic API server starting on http://{}", addr);
    println!("Compatible with Anthropic Messages API at /v1/messages");
    println!("Press Ctrl+C to stop the server");

    // Run the server with graceful shutdown
    ::axum::serve(
        tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to bind to address {}: {}", addr, e))?,
        app,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Server error: {}", e))?;

    Ok(())
}

#[cfg(not(feature = "anthropic-api"))]
pub async fn handle_anthropic_api_command(
    _core_cfg: vtcode_core::config::types::AgentConfig,
    _port: u16,
    _host: String,
) -> Result<()> {
    Err(anyhow::anyhow!(
        "Anthropic API server is not enabled. Recompile with --features anthropic-api"
    ))
}

#[cfg(test)]
mod tests {
    use super::handle_resume_session_command;
    use std::collections::BTreeMap;
    use std::time::{SystemTime, UNIX_EPOCH};
    use vtcode::startup::SessionResumeMode;
    use vtcode_core::config::core::PromptCachingConfig;
    use vtcode_core::config::models::Provider;
    use vtcode_core::config::types::{
        AgentConfig as CoreAgentConfig, ModelSelectionSource, ReasoningEffortLevel,
        UiSurfacePreference,
    };
    use vtcode_core::core::agent::snapshots::{
        DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS,
    };

    fn runtime_config() -> CoreAgentConfig {
        CoreAgentConfig {
            model: vtcode_core::config::constants::models::google::GEMINI_3_FLASH_PREVIEW
                .to_string(),
            api_key: "test-key".to_string(),
            provider: "gemini".to_string(),
            api_key_env: Provider::Gemini.default_api_key_env().to_string(),
            workspace: std::env::current_dir().expect("current_dir"),
            verbose: false,
            quiet: false,
            theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
            reasoning_effort: ReasoningEffortLevel::default(),
            ui_surface: UiSurfacePreference::default(),
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: BTreeMap::new(),
            checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
            checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
            max_conversation_turns: 1000,
            model_behavior: None,
        }
    }

    #[tokio::test]
    async fn resume_session_command_is_wired_to_sessions_handler() {
        let cfg = runtime_config();
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("unix time")
            .as_nanos();
        let fake_id = format!("nonexistent-session-{unique_suffix}");

        let result =
            handle_resume_session_command(&cfg, SessionResumeMode::Specific(fake_id), None, true)
                .await;

        let err = result.expect_err("expected missing session error");
        assert!(
            err.to_string().contains("No session with identifier"),
            "unexpected error: {err:#}"
        );
    }
}
