use anyhow::{Context, Result, anyhow};

use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::llm::{factory::create_provider_with_config, provider as uni};
use vtcode_core::models::ModelId;
use vtcode_core::tools::ToolRegistry;
use vtcode_core::tools::build_function_declarations;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::prompts::read_system_prompt;
use crate::agent::runloop::context::ContextTrimConfig;
use crate::agent::runloop::context::load_context_trim_config;
use crate::agent::runloop::telemetry::build_trajectory_logger;
use crate::agent::runloop::ui::render_session_banner;
use crate::agent::runloop::welcome::prepare_session_bootstrap;

pub(crate) struct SessionState {
    pub renderer: AnsiRenderer,
    pub placeholder_hint: Option<String>,
    pub placeholder_shown: bool,
    pub provider_client: Box<dyn uni::LLMProvider>,
    pub tool_registry: ToolRegistry,
    pub tools: Vec<uni::ToolDefinition>,
    pub trim_config: ContextTrimConfig,
    pub conversation_history: Vec<uni::Message>,
    pub ledger: DecisionTracker,
    pub trajectory: TrajectoryLogger,
    pub base_system_prompt: String,
}

pub(crate) async fn initialize_session(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    full_auto: bool,
) -> Result<SessionState> {
    let session_bootstrap = prepare_session_bootstrap(config, vt_cfg);
    let mut renderer = AnsiRenderer::stdout();
    render_session_banner(&mut renderer, config, &session_bootstrap)?;

    if let Some(text) = session_bootstrap.welcome_text.as_ref() {
        renderer.line(MessageStyle::Response, text)?;
        renderer.line(MessageStyle::Output, "")?;
    }

    let placeholder_hint = session_bootstrap.placeholder.clone();
    let provider_name = if config.provider.trim().is_empty() {
        config
            .model
            .parse::<ModelId>()
            .ok()
            .map(|model| model.provider().to_string())
            .unwrap_or_else(|| "gemini".to_string())
    } else {
        config.provider.to_lowercase()
    };
    let provider_client = create_provider_with_config(
        &provider_name,
        Some(config.api_key.clone()),
        None,
        Some(config.model.clone()),
    )
    .context("Failed to initialize provider client")?;

    let mut tool_registry = ToolRegistry::new(config.workspace.clone());
    tool_registry.initialize_async().await?;
    if let Some(cfg) = vt_cfg {
        if let Err(err) = tool_registry.apply_config_policies(&cfg.tools) {
            eprintln!(
                "Warning: Failed to apply tool policies from config: {}",
                err
            );
        }
    }

    if full_auto {
        let automation_cfg = vt_cfg
            .map(|cfg| cfg.automation.full_auto.clone())
            .ok_or_else(|| anyhow!("Full-auto configuration unavailable"))?;

        tool_registry.enable_full_auto_mode(&automation_cfg.allowed_tools);
        let allowlist = tool_registry
            .current_full_auto_allowlist()
            .unwrap_or_default();
        if allowlist.is_empty() {
            renderer.line(
                MessageStyle::Info,
                "Full-auto mode enabled with no tool permissions; tool calls will be skipped.",
            )?;
        } else {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "Full-auto mode enabled. Permitted tools: {}",
                    allowlist.join(", ")
                ),
            )?;
        }
    }

    let declarations = build_function_declarations();
    let tools: Vec<uni::ToolDefinition> = declarations
        .into_iter()
        .map(|decl| uni::ToolDefinition::function(decl.name, decl.description, decl.parameters))
        .collect();

    let trim_config = load_context_trim_config(vt_cfg);
    let conversation_history: Vec<uni::Message> = vec![];
    let ledger = DecisionTracker::new();
    let trajectory = build_trajectory_logger(&config.workspace, vt_cfg);
    let base_system_prompt = read_system_prompt(
        &config.workspace,
        session_bootstrap.prompt_addendum.as_deref(),
    );

    Ok(SessionState {
        renderer,
        placeholder_hint,
        placeholder_shown: false,
        provider_client,
        tool_registry,
        tools,
        trim_config,
        conversation_history,
        ledger,
        trajectory,
        base_system_prompt,
    })
}
