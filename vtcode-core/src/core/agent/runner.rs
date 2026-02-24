//! Agent runner for executing individual agent instances

use crate::config::VTCodeConfig;
use crate::config::constants::defaults;
use crate::config::loader::ConfigManager;
use crate::config::models::ModelId;
use crate::config::types::{ReasoningEffortLevel, VerbosityLevel};
use crate::core::agent::events::EventSink;
use crate::core::agent::state::ApiFailureTracker;
use crate::core::agent::steering::SteeringMessage;

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
use crate::gemini::Tool;
use crate::llm::factory::create_provider_for_model;
use crate::llm::provider as uni_provider;
use crate::llm::{AnyClient, make_client};
use crate::prompts::system::compose_system_instruction_text;
use crate::tools::{ToolRegistry, build_function_declarations_cached};

use anyhow::{Result, anyhow};
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tracing::warn;

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
mod workspace_config;

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
    /// Workspace path
    _workspace: PathBuf,
    /// Cached vtcode configuration
    config: Arc<VTCodeConfig>,
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
    /// Maximum number of autonomous turns before halting
    max_turns: usize,
    /// Loop detector to prevent infinite exploration
    loop_detector: parking_lot::Mutex<LoopDetector>,
    /// Cached shell policy patterns to avoid recompilation

    /// API failure tracking for exponential backoff
    failure_tracker: parking_lot::Mutex<ApiFailureTracker>,
    /// Context optimizer for token budget management
    context_optimizer: tokio::sync::Mutex<ContextOptimizer>,
    /// Tracks recent streaming failures to avoid repeated double-requests
    streaming_failures: parking_lot::Mutex<u8>,
    /// Records when streaming last failed for cooldown-based re-enablement
    streaming_last_failure: parking_lot::Mutex<Option<Instant>>,
    /// Tracks the latest reasoning stage name for the current turn
    last_reasoning_stage: parking_lot::Mutex<Option<String>>,
    /// Receiver for steering messages (e.g., stop, pause)
    steering_receiver:
        parking_lot::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>>,
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
        // Create client based on model
        let client: AnyClient = make_client(api_key.clone(), model)?;

        // Create unified provider client for tool calling
        let provider_client =
            create_provider_for_model(model.as_str(), api_key.clone(), None, None)
                .map_err(|e| anyhow!("Failed to create provider client: {}", e))?;

        // Load configuration once to seed system prompt and runtime policies
        let (config_value, system_prompt) = match ConfigManager::load_from_workspace(&workspace) {
            Ok(manager) => {
                let cfg = manager.config().clone();
                let prompt =
                    compose_system_instruction_text(workspace.as_path(), Some(&cfg), None).await;
                (cfg, prompt)
            }
            Err(err) => {
                warn!("Failed to load vtcode configuration for system prompt composition: {err:#}");
                let cfg = VTCodeConfig::default();
                let prompt = compose_system_instruction_text(workspace.as_path(), None, None).await;
                (cfg, prompt)
            }
        };

        let max_repeated_tool_calls = config_value.tools.max_repeated_tool_calls.max(1);
        let config = Arc::new(config_value);
        let tool_registry = ToolRegistry::new(workspace.clone()).await;
        tool_registry.set_harness_session(session_id.clone());
        tool_registry.set_agent_type(agent_type.to_string());
        tool_registry.apply_timeout_policy(&config.timeouts);
        let loop_detector = LoopDetector::with_max_repeated_calls(max_repeated_tool_calls);

        Ok(Self {
            agent_type,
            client,
            provider_client,
            tool_registry,
            system_prompt,
            session_id,
            _workspace: workspace,
            config,
            model: model.to_string(),
            _api_key: api_key,
            reasoning_effort: settings.reasoning_effort,
            verbosity: settings.verbosity,
            quiet: false,
            event_sink: None,
            max_turns: defaults::DEFAULT_FULL_AUTO_MAX_TURNS,
            loop_detector: parking_lot::Mutex::new(loop_detector),
            failure_tracker: parking_lot::Mutex::new(ApiFailureTracker::new()),
            context_optimizer: tokio::sync::Mutex::new(ContextOptimizer::new()),
            streaming_failures: parking_lot::Mutex::new(0),
            streaming_last_failure: parking_lot::Mutex::new(None),
            last_reasoning_stage: parking_lot::Mutex::new(None),
            steering_receiver: parking_lot::Mutex::new(steering_receiver),
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

    /// Build available tools for this agent type
    async fn build_agent_tools(&self) -> Result<Vec<Tool>> {
        use crate::llm::providers::gemini::sanitize_function_parameters;

        let declarations =
            build_function_declarations_cached(self.config().agent.tool_documentation_mode);

        // Filter tools based on agent type and permissions
        let mut allowed_tools = Vec::with_capacity(declarations.len());
        for decl in declarations.iter() {
            if !self.is_tool_allowed(&decl.name).await {
                continue;
            }

            allowed_tools.push(Tool {
                function_declarations: vec![crate::gemini::FunctionDeclaration {
                    name: decl.name.clone(),
                    description: decl.description.clone(),
                    parameters: sanitize_function_parameters(decl.parameters.clone()),
                }],
            });
        }

        Ok(allowed_tools)
    }
}
