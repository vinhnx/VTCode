//! Agent runner for executing individual agent instances

use crate::config::VTCodeConfig;
use crate::config::constants::tools;
use crate::config::models::ModelId;
use crate::config::types::{ReasoningEffortLevel, VerbosityLevel};
use crate::core::agent::events::EventSink;
use crate::core::agent::features::FeatureSet;
use crate::core::agent::session_config::ResolvedSessionConfig;
use crate::core::agent::steering::SteeringMessage;
use crate::core::threads::{ThreadBootstrap, ThreadRuntimeHandle, build_thread_archive_metadata};

/// Settings for the agent runner
#[derive(Clone)]
pub struct RunnerSettings {
    /// Reasoning effort level for the agent
    pub reasoning_effort: Option<ReasoningEffortLevel>,
    /// Verbosity level for output text
    pub verbosity: Option<VerbosityLevel>,
}

use crate::core::agent::types::AgentType;
use crate::core::context_optimizer::ContextOptimizer;
use crate::core::loop_detector::LoopDetector;
use crate::exec::events::ThreadEvent;
use crate::llm::AnyClient;
use crate::llm::client::ProviderClientAdapter;
use crate::llm::factory::{ProviderConfig, create_provider_with_config, infer_provider_from_model};
use crate::llm::provider as uni_provider;
use crate::project_doc::build_instruction_appendix;
use crate::prompts::PromptContext;
use crate::prompts::system::compose_system_instruction_text;
use crate::tools::ToolRegistry;

use anyhow::{Result, anyhow};
use parking_lot::{Mutex, RwLock};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};
use vtcode_config::auth::OpenAIChatGptAuthHandle;

mod config_helpers;
mod constants;
mod continuation;
mod execute;
mod execute_checks;
mod execute_tools;
mod helpers;
mod optimizer;
mod orchestration;
mod output;
mod retry;
mod summarize;
mod summary;
mod telemetry;
mod tool_access;
mod tool_args;
mod tool_exec;
mod tool_execution_guard;
mod types;
mod validation;

#[cfg(test)]
mod tests;

type ToolArgTransform = Arc<dyn Fn(&str, serde_json::Value) -> serde_json::Value + Send + Sync>;

/// Individual agent runner for executing specialized agent tasks
pub struct AgentRunner {
    /// Agent type and configuration
    agent_type: AgentType,
    /// LLM client for this agent
    client: AnyClient,
    /// Unified provider client (OpenAI/Anthropic/Gemini) for tool-calling
    provider_client: Box<dyn uni_provider::LLMProvider>,
    /// Tool registry with restricted access
    tool_registry: ToolRegistry,
    /// System prompt content
    system_prompt: String,
    /// Session information
    session_id: String,
    /// Initial archived history used to seed the first task on this runner.
    bootstrap_messages: Vec<crate::llm::provider::Message>,
    /// Workspace path
    _workspace: PathBuf,
    /// Frozen session-scoped configuration snapshot
    session_config: Arc<ResolvedSessionConfig>,
    /// Model identifier
    model: String,
    /// API key (for provider client construction in future flows)
    _api_key: String,
    /// Reasoning effort level for models that support it
    reasoning_effort: Option<ReasoningEffortLevel>,
    /// Verbosity level for output text
    verbosity: Option<VerbosityLevel>,
    /// Suppress stdout output when emitting structured events
    quiet: bool,
    /// Optional sink for streaming structured events
    event_sink: Option<EventSink>,
    /// Shared thread runtime state for history/event ownership
    thread_handle: ThreadRuntimeHandle,
    /// Maximum number of autonomous turns before halting
    max_turns: usize,
    /// Loop detector to prevent infinite exploration
    loop_detector: Mutex<LoopDetector>,
    /// Cached shell policy patterns to avoid recompilation

    /// Context optimizer for token budget management
    context_optimizer: tokio::sync::Mutex<ContextOptimizer>,
    /// Receiver for steering messages (e.g., stop, pause)
    steering_receiver: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>>,
    /// Optional restricted tool definitions used instead of the default registry projection.
    tool_definitions_override: RwLock<Option<Vec<uni_provider::ToolDefinition>>>,
    /// Optional argument transformer applied before tool validation/execution.
    tool_arg_transform: Option<ToolArgTransform>,
}

impl AgentRunner {
    /// Get the selected model for the current turn.
    fn get_selected_model(&self) -> String {
        self.model.clone()
    }

    fn runner_println(&self, args: std::fmt::Arguments) {
        if !self.quiet {
            println!("{args}");
        }
    }

    /// Create a new agent runner
    pub async fn new(
        agent_type: AgentType,
        model: ModelId,
        api_key: String,
        workspace: PathBuf,
        session_id: String,
        settings: RunnerSettings,
        steering_receiver: Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
    ) -> Result<Self> {
        Self::new_with_openai_auth(
            agent_type,
            model,
            api_key,
            workspace,
            session_id,
            settings,
            steering_receiver,
            None,
        )
        .await
    }

    pub async fn new_with_openai_auth(
        agent_type: AgentType,
        model: ModelId,
        api_key: String,
        workspace: PathBuf,
        session_id: String,
        settings: RunnerSettings,
        steering_receiver: Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
        openai_chatgpt_auth: Option<OpenAIChatGptAuthHandle>,
    ) -> Result<Self> {
        Self::new_with_thread_bootstrap_and_openai_auth(
            agent_type,
            model,
            api_key,
            workspace,
            session_id,
            settings,
            steering_receiver,
            ThreadBootstrap::new(None),
            openai_chatgpt_auth,
        )
        .await
    }

    /// Create an agent runner with a prebuilt thread bootstrap (for resumed sessions).
    pub async fn new_with_thread_bootstrap(
        agent_type: AgentType,
        model: ModelId,
        api_key: String,
        workspace: PathBuf,
        session_id: String,
        settings: RunnerSettings,
        steering_receiver: Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
        bootstrap: ThreadBootstrap,
    ) -> Result<Self> {
        Self::new_with_thread_bootstrap_and_openai_auth(
            agent_type,
            model,
            api_key,
            workspace,
            session_id,
            settings,
            steering_receiver,
            bootstrap,
            None,
        )
        .await
    }

    /// Create an agent runner with a prebuilt thread bootstrap (for resumed sessions).
    pub async fn new_with_thread_bootstrap_and_openai_auth(
        agent_type: AgentType,
        model: ModelId,
        api_key: String,
        workspace: PathBuf,
        session_id: String,
        settings: RunnerSettings,
        steering_receiver: Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
        bootstrap: ThreadBootstrap,
        openai_chatgpt_auth: Option<OpenAIChatGptAuthHandle>,
    ) -> Result<Self> {
        Self::new_with_thread_bootstrap_internal(
            agent_type,
            model,
            api_key,
            workspace,
            session_id,
            settings,
            steering_receiver,
            bootstrap,
            None,
            openai_chatgpt_auth,
        )
        .await
    }

    /// Create an agent runner with a prebuilt thread bootstrap and preloaded config.
    pub async fn new_with_thread_bootstrap_and_config(
        agent_type: AgentType,
        model: ModelId,
        api_key: String,
        workspace: PathBuf,
        session_id: String,
        settings: RunnerSettings,
        steering_receiver: Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
        bootstrap: ThreadBootstrap,
        vt_cfg: VTCodeConfig,
    ) -> Result<Self> {
        Self::new_with_thread_bootstrap_and_config_with_openai_auth(
            agent_type,
            model,
            api_key,
            workspace,
            session_id,
            settings,
            steering_receiver,
            bootstrap,
            vt_cfg,
            None,
        )
        .await
    }

    /// Create an agent runner with a prebuilt thread bootstrap, preloaded config, and runtime auth.
    #[allow(clippy::too_many_arguments)]
    pub async fn new_with_thread_bootstrap_and_config_with_openai_auth(
        agent_type: AgentType,
        model: ModelId,
        api_key: String,
        workspace: PathBuf,
        session_id: String,
        settings: RunnerSettings,
        steering_receiver: Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
        bootstrap: ThreadBootstrap,
        vt_cfg: VTCodeConfig,
        openai_chatgpt_auth: Option<OpenAIChatGptAuthHandle>,
    ) -> Result<Self> {
        Self::new_with_thread_bootstrap_internal(
            agent_type,
            model,
            api_key,
            workspace,
            session_id,
            settings,
            steering_receiver,
            bootstrap,
            Some(vt_cfg),
            openai_chatgpt_auth,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn new_with_thread_bootstrap_internal(
        agent_type: AgentType,
        model: ModelId,
        api_key: String,
        workspace: PathBuf,
        session_id: String,
        settings: RunnerSettings,
        steering_receiver: Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
        bootstrap: ThreadBootstrap,
        vt_cfg: Option<VTCodeConfig>,
        openai_chatgpt_auth: Option<OpenAIChatGptAuthHandle>,
    ) -> Result<Self> {
        // Load configuration once to seed system prompt and runtime policies
        let session_config = if let Some(vt_cfg) = vt_cfg {
            ResolvedSessionConfig::from_config(vt_cfg)
        } else {
            match ResolvedSessionConfig::load_from_workspace(&workspace) {
                Ok(session_config) => session_config,
                Err(err) => {
                    warn!(
                        "Failed to load vtcode configuration for system prompt composition: {err:#}"
                    );
                    ResolvedSessionConfig::from_config(VTCodeConfig::default())
                }
            }
        };
        let session_config = Arc::new(session_config);
        let provider_name = {
            let configured = session_config.effective().agent.provider.trim();
            if configured.is_empty() {
                infer_provider_from_model(model.as_str())
                    .map(|provider| provider.to_string())
                    .ok_or_else(|| anyhow!("Failed to determine provider for model {}", model))?
            } else {
                configured.to_lowercase()
            }
        };
        let provider_config = ProviderConfig {
            api_key: Some(api_key.clone()),
            openai_chatgpt_auth: openai_chatgpt_auth.clone(),
            copilot_auth: Some(session_config.effective().auth.copilot.clone()),
            base_url: None,
            model: Some(model.to_string()),
            prompt_cache: Some(session_config.effective().prompt_cache.clone()),
            timeouts: None,
            openai: Some(session_config.effective().provider.openai.clone()),
            anthropic: Some(session_config.effective().provider.anthropic.clone()),
            model_behavior: Some(session_config.effective().model.clone()),
            workspace_root: Some(workspace.clone()),
        };

        let client: AnyClient = Box::new(ProviderClientAdapter::new(
            create_provider_with_config(&provider_name, provider_config.clone())
                .map_err(|e| anyhow!("Failed to create client provider: {}", e))?,
            model.to_string(),
        ));
        let provider_client = create_provider_with_config(&provider_name, provider_config)
            .map_err(|e| anyhow!("Failed to create provider client: {}", e))?;
        if std::env::var_os("VTCODE_DEBUG_PROVIDER").is_some() {
            eprintln!(
                "vtcode-debug: runner provider={} client_provider={} model={}",
                provider_name,
                provider_client.name(),
                model
            );
        }
        let max_repeated_tool_calls = session_config
            .effective()
            .tools
            .max_repeated_tool_calls
            .max(1);
        let deferred_tool_policy = crate::tools::handlers::deferred_tool_policy_for_runtime(
            crate::llm::factory::infer_provider(
                Some(&session_config.effective().agent.provider),
                model.as_str(),
            ),
            provider_client.supports_responses_compaction(model.as_str()),
            Some(session_config.effective()),
        );
        let tool_registry = ToolRegistry::new(workspace.clone()).await;
        tool_registry.set_harness_session(session_id.clone());
        tool_registry.set_agent_type(agent_type.to_string());
        tool_registry.initialize_async().await?;
        if let Err(err) = tool_registry
            .apply_session_runtime_config(
                &session_config.effective().commands,
                &session_config.effective().permissions,
                &session_config.effective().sandbox,
                &session_config.effective().timeouts,
                &session_config.effective().tools,
            )
            .await
        {
            warn!("Failed to apply tool policies from config: {}", err);
        }
        if session_config.effective().mcp.enabled {
            if let Err(err) = crate::mcp::validate_mcp_config(&session_config.effective().mcp) {
                warn!("MCP configuration validation error: {err}");
            }
            info!("Deferring MCP client initialization to on-demand activation");
        }
        if session_config.effective().context.dynamic.enabled
            && let Err(err) = crate::context::initialize_dynamic_context(
                &workspace,
                &session_config.effective().context.dynamic,
            )
            .await
        {
            warn!("Failed to initialize dynamic context directories: {}", err);
        }
        let available_tools = tool_registry
            .model_tools(crate::tools::handlers::SessionToolsConfig {
                surface: crate::tools::handlers::SessionSurface::AgentRunner,
                capability_level: crate::config::types::CapabilityLevel::CodeSearch,
                documentation_mode: session_config.effective().agent.tool_documentation_mode,
                plan_mode: tool_registry.is_plan_mode(),
                request_user_input_enabled: false,
                model_capabilities: crate::tools::handlers::ToolModelCapabilities::for_model_name(
                    model.as_str(),
                ),
                deferred_tool_policy,
            })
            .await
            .into_iter()
            .map(|tool| tool.function_name().to_string())
            .collect::<Vec<_>>();
        let mut prompt_context = PromptContext::from_workspace_tools(&workspace, available_tools);
        prompt_context.load_available_skills();
        let mut system_prompt = compose_system_instruction_text(
            workspace.as_path(),
            Some(session_config.effective()),
            Some(&prompt_context),
        )
        .await;
        if let Some(appendix) =
            build_instruction_appendix(&session_config.effective().agent, workspace.as_path()).await
        {
            system_prompt.push_str("\n\n# INSTRUCTIONS\n");
            system_prompt.push_str(&appendix);
        }
        let loop_detector = LoopDetector::with_max_repeated_calls(max_repeated_tool_calls);
        let bootstrap_messages = bootstrap.messages.clone();
        let mut bootstrap = bootstrap;
        if bootstrap.metadata.is_none() {
            bootstrap.metadata = Some(build_thread_archive_metadata(
                workspace.as_path(),
                model.as_str(),
                &session_config.effective().agent.provider,
                &session_config.effective().agent.theme,
                settings
                    .reasoning_effort
                    .unwrap_or(session_config.effective().agent.reasoning_effort)
                    .as_str(),
            ));
        }
        let thread_handle = crate::core::threads::ThreadManager::new()
            .start_thread_with_identifier(session_id.clone(), bootstrap);
        let max_turns = session_config
            .effective()
            .automation
            .full_auto
            .max_turns
            .max(1);

        Ok(Self {
            agent_type,
            client,
            provider_client,
            tool_registry,
            system_prompt,
            session_id,
            bootstrap_messages,
            _workspace: workspace,
            session_config,
            model: model.to_string(),
            _api_key: api_key,
            reasoning_effort: settings.reasoning_effort,
            verbosity: settings.verbosity,
            quiet: false,
            event_sink: None,
            thread_handle,
            max_turns,
            loop_detector: Mutex::new(loop_detector),
            context_optimizer: tokio::sync::Mutex::new(ContextOptimizer::new()),
            steering_receiver: Mutex::new(steering_receiver),
            tool_definitions_override: RwLock::new(None),
            tool_arg_transform: None,
        })
    }

    /// Enable or disable console output for this runner.
    pub fn set_quiet(&mut self, quiet: bool) {
        self.quiet = quiet;
    }

    /// Snapshot the runner-owned conversation messages for archive persistence.
    pub fn session_messages(&self) -> Vec<crate::llm::provider::Message> {
        self.thread_handle.messages()
    }

    /// Enable read-only plan mode for the underlying tool registry.
    pub fn enable_plan_mode(&self) {
        self.tool_registry.enable_plan_mode();
        self.tool_registry.plan_mode_state().enable();
    }

    pub fn disable_plan_mode(&self) {
        self.tool_registry.disable_plan_mode();
        self.tool_registry.plan_mode_state().disable();
    }

    /// Attach a callback that will be invoked for each structured event as it is recorded.
    pub fn set_event_handler<F>(&mut self, handler: F)
    where
        F: FnMut(&ThreadEvent) + Send + 'static,
    {
        self.event_sink = Some(Arc::new(Mutex::new(Box::new(handler))));
    }

    /// Remove any previously registered structured event callback.
    pub fn clear_event_handler(&mut self) {
        self.event_sink = None;
    }

    pub fn set_tool_definitions_override(
        &mut self,
        definitions: Vec<uni_provider::ToolDefinition>,
    ) {
        *self.tool_definitions_override.write() = Some(definitions);
    }

    pub fn clear_tool_definitions_override(&mut self) {
        *self.tool_definitions_override.write() = None;
    }

    pub fn set_tool_arg_transform(&mut self, transform: ToolArgTransform) {
        self.tool_arg_transform = Some(transform);
    }

    pub fn clear_tool_arg_transform(&mut self) {
        self.tool_arg_transform = None;
    }

    /// Enable full-auto execution with the provided allow-list.
    pub async fn enable_full_auto(&mut self, allowed_tools: &[String]) {
        self.tool_registry
            .enable_full_auto_mode(allowed_tools)
            .await;
    }

    /// Restrict an allow-list to tools suitable for strict review-only runs.
    pub async fn review_tool_allowlist(&self, allowed_tools: &[String]) -> Vec<String> {
        let review_candidates = if allowed_tools
            .iter()
            .any(|tool| tool.trim() == tools::WILDCARD_ALL)
        {
            self.tool_registry.available_tools().await
        } else {
            allowed_tools.to_vec()
        };

        review_candidates
            .iter()
            .filter(|tool_name| {
                let canonical = crate::tools::names::canonical_tool_name(tool_name);
                let canonical = canonical.as_ref();

                !matches!(
                    canonical,
                    tools::REQUEST_USER_INPUT
                        | tools::TASK_TRACKER
                        | tools::PLAN_TASK_TRACKER
                        | tools::ENTER_PLAN_MODE
                        | tools::EXIT_PLAN_MODE
                ) && (canonical == tools::UNIFIED_FILE
                    || !self.tool_registry.is_mutating_tool(tool_name))
            })
            .cloned()
            .collect()
    }

    pub(crate) fn features(&self) -> FeatureSet {
        self.session_config.features().clone()
    }
}
