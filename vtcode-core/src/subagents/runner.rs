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
use crate::llm::AnyClient;
use crate::tools::ToolRegistry;

use super::registry::SubagentRegistry;

/// Check if a model string indicates a local provider (Ollama, LMStudio, etc.)
///
/// This uses heuristics from the LLM factory to detect local models.
fn is_local_model(model: &str) -> bool {
    let m = model.to_lowercase();

    // Ollama format: "model:tag" without slashes
    if m.contains(':') && !m.contains('/') && !m.contains('@') {
        return true;
    }

    // LMStudio format
    if m.starts_with("lmstudio-community/") {
        return true;
    }

    // Explicit local provider prefixes
    if m.starts_with("ollama/") || m.starts_with("local/") {
        return true;
    }

    false
}

/// Cleanup guard to ensure subagent is unregistered even on panic/cancellation
struct CleanupGuard {
    registry: Arc<SubagentRegistry>,
    agent_id: String,
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        let registry = self.registry.clone();
        let agent_id = std::mem::take(&mut self.agent_id);

        // Only spawn cleanup task if tokio runtime is available
        // This prevents panics when Drop is called during shutdown
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                debug!(agent_id = %agent_id, "CleanupGuard: unregistering subagent");
                registry.unregister_running(&agent_id).await;
            });
        } else {
            // Runtime not available - log warning but don't panic
            // The stale entry cleanup in registry.can_spawn() will handle this
            tracing::warn!(
                agent_id = %agent_id,
                "CleanupGuard: tokio runtime unavailable, relying on stale entry cleanup"
            );
        }
    }
}

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
    #[allow(dead_code)]
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

        // Create cleanup guard to ensure unregister happens even on panic/cancellation
        let _cleanup_guard = CleanupGuard {
            registry: self.registry.clone(),
            agent_id: agent_id.clone(),
        };

        // Execute the subagent
        let result = self
            .execute_subagent(&agent_id, &subagent_config, &params)
            .await;

        // Cleanup guard will automatically unregister when dropped

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
    ///
    /// Uses raw model string instead of ModelId enum to support Ollama
    /// and other providers with custom model names.
    fn create_client(&self, config: &SubagentConfig) -> Result<AnyClient> {
        let model_string = self.resolve_model_string(config)?;
        let api_key = self.parent_config.api_key.clone();

        // Use factory directly with raw model string (bypasses ModelId parsing)
        let provider = crate::llm::factory::create_provider_for_model(&model_string, api_key, None)
            .context("Failed to create LLM provider for subagent")?;

        // Wrap provider in client adapter
        Ok(Box::new(crate::llm::ProviderClientAdapter::new(
            provider,
            model_string,
        )))
    }

    /// Resolve the model string to use for this subagent
    ///
    /// Returns raw model string (not ModelId) to support custom model names
    /// from providers like Ollama, LMStudio, etc.
    fn resolve_model_string(&self, config: &SubagentConfig) -> Result<String> {
        let parent_model_str = &self.parent_config.model;

        match &config.model {
            SubagentModel::Inherit => {
                // Use parent's model string directly (works for Ollama custom names)
                Ok(parent_model_str.clone())
            }
            SubagentModel::Alias(alias) => {
                // For aliases, check if parent is using local provider
                // If so, inherit parent model instead of mapping to cloud models
                let is_local = is_local_model(parent_model_str);

                if is_local {
                    debug!(
                        "Subagent alias '{}' overridden to inherit parent local model '{}'",
                        alias, parent_model_str
                    );
                    return Ok(parent_model_str.clone());
                }

                // Map alias to default model string
                match alias.to_lowercase().as_str() {
                    "sonnet" => Ok(ModelId::default_subagent().to_string()),
                    "opus" => Ok(ModelId::default_orchestrator().to_string()),
                    "haiku" => Ok(ModelId::default_subagent().to_string()),
                    "inherit" => Ok(parent_model_str.clone()),
                    _ => {
                        // Try to parse as ModelId, otherwise use as-is
                        if let Ok(model_id) = alias.parse::<ModelId>() {
                            Ok(model_id.to_string())
                        } else {
                            // Use the alias as raw model string (supports custom models)
                            Ok(alias.clone())
                        }
                    }
                }
            }
            SubagentModel::ModelId(id) => {
                // Try to parse as ModelId, otherwise use as-is
                if let Ok(model_id) = id.parse::<ModelId>() {
                    Ok(model_id.to_string())
                } else {
                    // Use as raw model string
                    Ok(id.clone())
                }
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
        let mut client = self.create_client(config)?;
        let system_prompt = self.build_system_prompt(config, params);
        let timeout = params
            .timeout
            .unwrap_or_else(|| self.registry.default_timeout());

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
        // For now, we do a simplified single-turn execution
        // A full implementation would use the agent runloop
        let response = tokio::time::timeout(timeout, async {
            // This is a simplified version - full implementation would
            // integrate with the agent runloop for multi-turn execution
            self.single_turn_execution(&mut client, &system_prompt, &user_message, &tools)
                .await
        })
        .await
        .context("Subagent execution timed out")??;

        let final_output = response.0;
        let turn_count = response.1;
        let tokens = response.2;

        Ok((final_output, turn_count, tokens))
    }

    /// Create a filtered tool registry for the subagent
    ///
    /// For subagents with restricted tool access (e.g., explore with read-only tools),
    /// this returns the parent registry but logs the restrictions. The actual filtering
    /// happens at execution time via `SubagentConfig::has_tool_access()`.
    ///
    /// Note: Full tool registry cloning with filtering is expensive. Instead, we:
    /// 1. Return the shared parent registry (zero-cost Arc clone)
    /// 2. Enforce restrictions at tool call time using config.has_tool_access()
    /// 3. For read-only subagents, set plan_read_only_mode on the registry
    async fn create_filtered_tools(&self, config: &SubagentConfig) -> Result<Arc<ToolRegistry>> {
        // Log tool restrictions for debugging
        if let Some(allowed) = config.allowed_tools() {
            debug!(
                subagent = %config.name,
                allowed_tools = ?allowed,
                "Subagent has restricted tool access"
            );
        }

        // For read-only subagents, the execution should respect is_read_only()
        // The parent registry is shared; plan_read_only_mode is set per-execution context
        if config.is_read_only() {
            debug!(
                subagent = %config.name,
                "Subagent is read-only (permission_mode: plan)"
            );
        }

        // Return shared parent registry - filtering happens at execution time
        Ok(self.parent_tools.clone())
    }

    /// Simplified single-turn execution
    ///
    /// This performs a basic LLM call with the system prompt and user message.
    /// For complex multi-turn tasks with tool usage, the full agent runloop
    /// should be used instead.
    async fn single_turn_execution(
        &self,
        client: &mut AnyClient,
        system_prompt: &str,
        user_message: &str,
        _tools: &Arc<ToolRegistry>,
    ) -> Result<(String, u32, TokenUsage)> {
        // Build a combined prompt with system context and user request
        let full_prompt = format!("{}\n\n---\n\n**Task:**\n{}", system_prompt, user_message);

        debug!(
            "Subagent executing with prompt length: {} chars",
            full_prompt.len()
        );

        // Call the LLM
        let response = client
            .generate(&full_prompt)
            .await
            .context("Failed to generate subagent response")?;

        // Extract token usage if available
        let tokens = response
            .usage
            .as_ref()
            .map(|u| TokenUsage {
                input_tokens: u.prompt_tokens as u64,
                output_tokens: u.completion_tokens as u64,
                total_tokens: u.total_tokens as u64,
            })
            .unwrap_or_default();

        debug!(
            "Subagent completed: {} output tokens, {} chars response",
            tokens.output_tokens,
            response.content_text().len()
        );

        Ok((response.content_string(), 1, tokens))
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
