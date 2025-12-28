//! Subagent execution runner
//!
//! Executes subagents with isolated context, filtered tool access,
//! and separate transcript management.

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info};

use vtcode_config::subagent::{SubagentConfig, SubagentModel};

use crate::config::models::ModelId;
use crate::config::types::AgentConfig;
use crate::llm::{AnyClient, make_client};
use crate::tools::ToolRegistry;

use super::registry::SubagentRegistry;

/// Result from a subagent execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentResult {
    /// Unique agent ID for this execution
    pub agent_id: String,
    /// Subagent name that was executed
    pub subagent_name: String,
    /// Final output from the subagent
    pub output: String,
    /// Whether execution completed successfully
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
    /// Execution duration
    pub duration_ms: u64,
    /// Number of turns/exchanges
    pub turn_count: u32,
    /// Token usage (if available)
    pub tokens_used: Option<TokenUsage>,
}

/// Token usage statistics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

/// Thoroughness level for exploration subagents
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Thoroughness {
    /// Fast searches with minimal exploration
    Quick,
    /// Moderate exploration (default)
    #[default]
    Medium,
    /// Comprehensive analysis
    VeryThorough,
}

impl std::fmt::Display for Thoroughness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Quick => write!(f, "quick"),
            Self::Medium => write!(f, "medium"),
            Self::VeryThorough => write!(f, "very thorough"),
        }
    }
}

/// Parameters for spawning a subagent
#[derive(Debug, Clone)]
pub struct SpawnParams {
    /// Task description for the subagent
    pub prompt: String,
    /// Specific subagent to use (if None, auto-select)
    pub subagent_type: Option<String>,
    /// Agent ID to resume (for continuation)
    pub resume: Option<String>,
    /// Thoroughness level (for explore-type agents)
    pub thoroughness: Thoroughness,
    /// Timeout override
    pub timeout: Option<Duration>,
    /// Additional context from parent
    pub parent_context: Option<String>,
}

impl SpawnParams {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            subagent_type: None,
            resume: None,
            thoroughness: Thoroughness::default(),
            timeout: None,
            parent_context: None,
        }
    }

    pub fn with_subagent(mut self, subagent: impl Into<String>) -> Self {
        self.subagent_type = Some(subagent.into());
        self
    }

    pub fn with_resume(mut self, agent_id: impl Into<String>) -> Self {
        self.resume = Some(agent_id.into());
        self
    }

    pub fn with_thoroughness(mut self, thoroughness: Thoroughness) -> Self {
        self.thoroughness = thoroughness;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn with_parent_context(mut self, context: impl Into<String>) -> Self {
        self.parent_context = Some(context.into());
        self
    }
}

/// Subagent execution runner
pub struct SubagentRunner {
    /// Registry of available subagents
    registry: Arc<SubagentRegistry>,
    /// Parent agent configuration
    parent_config: AgentConfig,
    /// Parent tool registry (for filtering)
    parent_tools: Arc<ToolRegistry>,
    /// Workspace root
    workspace_root: PathBuf,
}

impl SubagentRunner {
    pub fn new(
        registry: Arc<SubagentRegistry>,
        parent_config: AgentConfig,
        parent_tools: Arc<ToolRegistry>,
        workspace_root: PathBuf,
    ) -> Self {
        Self {
            registry,
            parent_config,
            parent_tools,
            workspace_root,
        }
    }

    /// Spawn and execute a subagent
    pub async fn spawn(&self, params: SpawnParams) -> Result<SubagentResult> {
        let start = Instant::now();

        // Check if we can spawn
        if !self.registry.can_spawn().await {
            return Err(anyhow!(
                "Maximum concurrent subagents reached. Wait for running agents to complete."
            ));
        }

        // Resolve which subagent to use
        let subagent_config = self.resolve_subagent(&params)?;
        let subagent_name = subagent_config.name.clone();

        // Generate or reuse agent ID
        let agent_id = params
            .resume
            .clone()
            .unwrap_or_else(|| self.registry.generate_agent_id());

        info!(
            agent_id = %agent_id,
            subagent = %subagent_name,
            "Spawning subagent"
        );

        // Register as running
        self.registry
            .register_running(agent_id.clone(), subagent_config.clone())
            .await;

        // Execute the subagent
        let result = self
            .execute_subagent(&agent_id, &subagent_config, &params)
            .await;

        // Unregister
        self.registry.unregister_running(&agent_id).await;

        let duration_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok((output, turn_count, tokens)) => Ok(SubagentResult {
                agent_id,
                subagent_name,
                output,
                success: true,
                error: None,
                duration_ms,
                turn_count,
                tokens_used: Some(tokens),
            }),
            Err(e) => Ok(SubagentResult {
                agent_id,
                subagent_name,
                output: String::new(),
                success: false,
                error: Some(e.to_string()),
                duration_ms,
                turn_count: 0,
                tokens_used: None,
            }),
        }
    }

    /// Resolve which subagent configuration to use
    fn resolve_subagent(&self, params: &SpawnParams) -> Result<SubagentConfig> {
        // If specific subagent requested
        if let Some(name) = &params.subagent_type {
            return self
                .registry
                .get(name)
                .cloned()
                .ok_or_else(|| anyhow!("Subagent not found: {}", name));
        }

        // Auto-select based on prompt
        if let Some(config) = self.registry.find_best_match(&params.prompt) {
            debug!("Auto-selected subagent: {}", config.name);
            return Ok(config.clone());
        }

        // Default to general-purpose agent
        self.registry
            .get("general")
            .cloned()
            .ok_or_else(|| anyhow!("No suitable subagent found and 'general' not available"))
    }

    /// Create LLM client for the subagent
    fn create_client(&self, config: &SubagentConfig) -> Result<AnyClient> {
        let model_id = self.resolve_model(config)?;
        let api_key = self.parent_config.api_key.clone();
        make_client(api_key, model_id).context("Failed to create LLM client for subagent")
    }

    /// Resolve the model to use for this subagent
    fn resolve_model(&self, config: &SubagentConfig) -> Result<ModelId> {
        match &config.model {
            SubagentModel::Inherit => {
                // Use parent's model
                self.parent_config
                    .model
                    .parse::<ModelId>()
                    .context("Failed to parse parent model")
            }
            SubagentModel::Alias(alias) => {
                // Map alias to model ID
                match alias.to_lowercase().as_str() {
                    "sonnet" => Ok(ModelId::default_subagent()),
                    "opus" => Ok(ModelId::default_orchestrator()),
                    "haiku" => {
                        // Use a fast model for explore-type agents
                        // Try to get provider-specific fast model
                        Ok(ModelId::default_subagent())
                    }
                    _ => alias
                        .parse::<ModelId>()
                        .context("Failed to parse model alias"),
                }
            }
            SubagentModel::ModelId(id) => {
                id.parse::<ModelId>().context("Failed to parse model ID")
            }
        }
    }

    /// Build system prompt for the subagent
    fn build_system_prompt(&self, config: &SubagentConfig, params: &SpawnParams) -> String {
        let mut prompt = config.system_prompt.clone();

        // Add thoroughness instruction for explore-type agents
        if config.name == "explore" {
            prompt.push_str(&format!(
                "\n\n**Thoroughness Level: {}**\n",
                params.thoroughness
            ));
            match params.thoroughness {
                Thoroughness::Quick => {
                    prompt.push_str("Be fast. Minimal exploration. Target specific files.");
                }
                Thoroughness::Medium => {
                    prompt.push_str("Balance speed and coverage. Follow promising leads.");
                }
                Thoroughness::VeryThorough => {
                    prompt.push_str("Be comprehensive. Check multiple locations. Try different naming conventions.");
                }
            }
        }

        // Add parent context if provided
        if let Some(context) = &params.parent_context {
            prompt.push_str(&format!(
                "\n\n**Context from parent agent:**\n{}\n",
                context
            ));
        }

        prompt
    }

    /// Execute the subagent and return results
    async fn execute_subagent(
        &self,
        agent_id: &str,
        config: &SubagentConfig,
        params: &SpawnParams,
    ) -> Result<(String, u32, TokenUsage)> {
        let client = self.create_client(config)?;
        let system_prompt = self.build_system_prompt(config, params);
        let timeout = params.timeout.unwrap_or_else(|| self.registry.default_timeout());

        // Create filtered tool registry if tools are restricted
        let tools = self.create_filtered_tools(config).await?;

        // Build initial message
        let user_message = params.prompt.clone();

        // Get available tools count
        let available_tools = tools.available_tools().await;

        debug!(
            agent_id = %agent_id,
            model = %config.model,
            tools_count = available_tools.len(),
            "Starting subagent execution"
        );

        // Execute the agent loop
        let mut turn_count = 0u32;
        let mut tokens = TokenUsage::default();
        let mut final_output = String::new();

        // For now, we do a simplified single-turn execution
        // A full implementation would use the agent runloop
        let response = tokio::time::timeout(timeout, async {
            // This is a simplified version - full implementation would
            // integrate with the agent runloop for multi-turn execution
            self.single_turn_execution(&client, &system_prompt, &user_message, &tools)
                .await
        })
        .await
        .context("Subagent execution timed out")??;

        final_output = response.0;
        turn_count = response.1;
        tokens = response.2;

        Ok((final_output, turn_count, tokens))
    }

    /// Create a filtered tool registry for the subagent
    async fn create_filtered_tools(&self, _config: &SubagentConfig) -> Result<Arc<ToolRegistry>> {
        // For now, return the parent tools
        // A full implementation would filter based on config.tools
        // and config.permission_mode
        Ok(self.parent_tools.clone())
    }

    /// Simplified single-turn execution
    async fn single_turn_execution(
        &self,
        _client: &AnyClient,
        system_prompt: &str,
        user_message: &str,
        _tools: &Arc<ToolRegistry>,
    ) -> Result<(String, u32, TokenUsage)> {
        // This is a placeholder for the full agent loop integration
        // In a complete implementation, this would:
        // 1. Build messages with system prompt
        // 2. Call LLM with tool declarations
        // 3. Handle tool calls
        // 4. Continue until task complete

        // For now, return a placeholder
        Ok((
            format!(
                "[Subagent execution placeholder]\nPrompt: {}\nSystem: {}",
                user_message,
                &system_prompt[..100.min(system_prompt.len())]
            ),
            1,
            TokenUsage::default(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_params_builder() {
        let params = SpawnParams::new("Find all test files")
            .with_subagent("explore")
            .with_thoroughness(Thoroughness::VeryThorough)
            .with_timeout(Duration::from_secs(60));

        assert_eq!(params.prompt, "Find all test files");
        assert_eq!(params.subagent_type, Some("explore".to_string()));
        assert_eq!(params.thoroughness, Thoroughness::VeryThorough);
        assert_eq!(params.timeout, Some(Duration::from_secs(60)));
    }

    #[test]
    fn test_thoroughness_display() {
        assert_eq!(format!("{}", Thoroughness::Quick), "quick");
        assert_eq!(format!("{}", Thoroughness::Medium), "medium");
        assert_eq!(format!("{}", Thoroughness::VeryThorough), "very thorough");
    }
}
