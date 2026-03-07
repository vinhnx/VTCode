//! Agent runner for executing individual agent instances

use crate::config::VTCodeConfig;
use crate::config::constants::tools;
use crate::config::models::ModelId;
use crate::config::types::{ReasoningEffortLevel, VerbosityLevel};
use crate::core::agent::events::EventSink;
use crate::core::agent::features::FeatureSet;
use crate::core::agent::session_config::ResolvedSessionConfig;
use crate::core::agent::state::ApiFailureTracker;
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
use crate::llm::factory::create_provider_for_model;
use crate::llm::provider as uni_provider;
use crate::llm::{AnyClient, make_client};
use crate::prompts::PromptContext;
use crate::prompts::system::compose_system_instruction_text;
use crate::tools::ToolRegistry;

use anyhow::{Result, anyhow};
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tracing::{info, warn};

mod config_helpers;
mod constants;
mod execute;
mod execute_checks;
mod execute_tools;
mod helpers;
mod optimizer;
mod output;
mod provider_response;
mod retry;
mod summarize;
mod summary;
mod telemetry;
mod tool_access;
mod tool_args;
mod tool_exec;
mod types;
mod validation;

#[cfg(test)]
mod tests;

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

    /// API failure tracking for exponential backoff
    failure_tracker: Mutex<ApiFailureTracker>,
    /// Context optimizer for token budget management
    context_optimizer: tokio::sync::Mutex<ContextOptimizer>,
    /// Tracks recent streaming failures to avoid repeated double-requests
    streaming_failures: Mutex<u8>,
    /// Records when streaming last failed for cooldown-based re-enablement
    streaming_last_failure: Mutex<Option<Instant>>,
    /// Tracks the latest reasoning stage name for the current turn
    last_reasoning_stage: Mutex<Option<String>>,
    /// Receiver for steering messages (e.g., stop, pause)
    steering_receiver: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>>,
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
        Self::new_with_thread_bootstrap(
            agent_type,
            model,
            api_key,
            workspace,
            session_id,
            settings,
            steering_receiver,
            ThreadBootstrap::new(None),
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
        )
        .await
    }

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
    ) -> Result<Self> {
        // Create client based on model
        let client: AnyClient = make_client(api_key.clone(), model)?;

        // Create unified provider client for tool calling
        let provider_client =
            create_provider_for_model(model.as_str(), api_key.clone(), None, None)
                .map_err(|e| anyhow!("Failed to create provider client: {}", e))?;

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
        let max_repeated_tool_calls = session_config
            .effective()
            .tools
            .max_repeated_tool_calls
            .max(1);
        let tool_registry = ToolRegistry::new(workspace.clone()).await;
        tool_registry.set_harness_session(session_id.clone());
        tool_registry.set_agent_type(agent_type.to_string());
        tool_registry.initialize_async().await?;
        if let Err(err) = tool_registry
            .apply_session_runtime_config(
                &session_config.effective().commands,
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
            })
            .await
            .into_iter()
            .map(|tool| tool.function_name().to_string())
            .collect::<Vec<_>>();
        let prompt_context = PromptContext::from_workspace_tools(&workspace, available_tools);
        let system_prompt = compose_system_instruction_text(
            workspace.as_path(),
            Some(session_config.effective()),
            Some(&prompt_context),
        )
        .await;
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
            failure_tracker: Mutex::new(ApiFailureTracker::new()),
            context_optimizer: tokio::sync::Mutex::new(ContextOptimizer::new()),
            streaming_failures: Mutex::new(0),
            streaming_last_failure: Mutex::new(None),
            last_reasoning_stage: Mutex::new(None),
            steering_receiver: Mutex::new(steering_receiver),
        })
    }

    /// Check for pending steering messages
    pub fn check_steering(&self) -> Option<SteeringMessage> {
        let mut guard = self.steering_receiver.lock();
        if let Some(rx) = guard.as_mut() {
            rx.try_recv().ok()
        } else {
            None
        }
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
