//! Skill execution as Tool trait implementation
//!
//! Bridges Agent Skills to VT Code's tool system by implementing the Tool trait
//! for skills, enabling them to execute with full access to VT Code's permissions,
//! caching, and audit systems.
//!
//! ## LLM Sub-Calls (Phase 5)
//!
//! Skills can now execute with full LLM support via `execute_skill_with_sub_llm()`:
//! 1. Skill instructions become the system prompt
//! 2. User input is the first message
//! 3. All available tools are passed to the LLM
//! 4. Tool calls are executed and results are fed back
//! 5. Final response is returned

use crate::llm::provider::{FinishReason, LLMProvider, LLMRequest, Message, ToolDefinition};
use crate::skills::types::Skill;
use crate::tool_policy::ToolPolicy;
use crate::tools::ToolRegistry;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde_json::Value;
use std::borrow::Cow;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Network-capable tool names that should be filtered based on skill network policy
const NETWORK_TOOLS: &[&str] = &[
    "http",
    "fetch",
    "browser",
    "web_search",
    "read_web_page",
    "curl",
];

/// Filter available tools based on skill's network policy
///
/// - If skill has no network policy: remove network-capable tools
/// - If skill has allowed_domains: keep network tools but log the constraint
/// - If skill has denied_domains: keep network tools but log the constraint
pub fn filter_tools_for_skill(skill: &Skill, tools: Vec<ToolDefinition>) -> Vec<ToolDefinition> {
    let network_policy = &skill.manifest.network_policy;

    match network_policy {
        None => tools
            .into_iter()
            .filter(|t| {
                let name_lower = t
                    .function
                    .as_ref()
                    .map(|f| f.name.to_lowercase())
                    .unwrap_or_default();
                let is_network = NETWORK_TOOLS.iter().any(|nt| name_lower.contains(nt));
                if is_network {
                    debug!(
                        "Filtered network tool '{}' for skill '{}' (no network policy)",
                        name_lower,
                        skill.name()
                    );
                }
                !is_network
            })
            .collect(),
        Some(policy) => {
            if !policy.allowed_domains.is_empty() {
                info!(
                    "Skill '{}' has network allowlist: {:?}",
                    skill.name(),
                    policy.allowed_domains
                );
            }
            if !policy.denied_domains.is_empty() {
                info!(
                    "Skill '{}' has network denylist: {:?}",
                    skill.name(),
                    policy.denied_domains
                );
            }
            tools
        }
    }
}

/// Execute a skill with LLM sub-call support (Phase 5)
///
/// Creates a sub-conversation where:
/// 1. Skill instructions become the system prompt
/// 2. User input becomes the first user message
/// 3. All available tools are passed to the LLM
/// 4. Tool calls are executed via the tool registry
/// 5. Tool results are fed back to continue the conversation
/// 6. Final response is returned
///
/// # Arguments
/// * `skill` - The skill to execute
/// * `user_input` - The user's input/request for the skill
/// * `provider` - The LLM provider for sub-calls
/// * `tool_registry` - The tool registry for executing nested tools
/// * `available_tools` - Tools available to the skill
/// * `model` - The model to use for skill execution
pub async fn execute_skill_with_sub_llm(
    skill: &Skill,
    user_input: String,
    provider: &dyn LLMProvider,
    tool_registry: &mut ToolRegistry,
    available_tools: Vec<ToolDefinition>,
    model: String,
) -> Result<String> {
    debug!("Executing skill '{}' with LLM sub-call", skill.name());

    // Apply network policy filtering
    let available_tools = filter_tools_for_skill(skill, available_tools);

    // Build conversation starting with user input
    let mut messages = vec![Message::user(user_input.clone())];

    // Create LLM request with skill instructions as system prompt
    let mut request = LLMRequest {
        messages: messages.clone(),
        system_prompt: Some(std::sync::Arc::new(skill.instructions.clone())),
        tools: if available_tools.is_empty() {
            None
        } else {
            Some(std::sync::Arc::new(available_tools.clone()))
        },
        model: model.clone(),
        max_tokens: Some(4096),
        ..Default::default()
    };

    // Loop: Make LLM request and handle tool calls
    const MAX_ITERATIONS: usize = 10;
    const BACKOFF_BASE_MS: u64 = 50; // initial back‑off delay
    const MAX_RATE_LIMIT_WAIT_CYCLES: usize = 20;
    const SKILL_RATE_LIMIT_KEY: &str = "skill_sub_llm";
    let mut iterations = 0;
    let mut backoff = BACKOFF_BASE_MS;
    let mut wait_cycles = 0usize;

    loop {
        // Rate‑limit tool execution before each iteration
        if let Err(wait_hint) =
            crate::tools::adaptive_rate_limiter::try_acquire_global(SKILL_RATE_LIMIT_KEY)
        {
            wait_cycles += 1;
            if wait_cycles > MAX_RATE_LIMIT_WAIT_CYCLES {
                return Err(anyhow!(
                    "Skill execution stayed rate-limited for too long ({} cycles)",
                    MAX_RATE_LIMIT_WAIT_CYCLES
                ));
            }

            let delay = wait_hint
                .max(Duration::from_millis(backoff))
                .min(Duration::from_secs(2));
            // If rate limited, wait a bit and retry without counting as an iteration
            warn!(
                "Rate limit hit for skill execution – backing off {}ms",
                delay.as_millis()
            );
            tokio::time::sleep(delay).await;
            backoff = (backoff * 2).min(2000); // cap back‑off at 2 s
            continue;
        }
        wait_cycles = 0;
        backoff = BACKOFF_BASE_MS;

        iterations += 1;
        if iterations > MAX_ITERATIONS {
            return Err(anyhow!(
                "Skill execution exceeded max iterations ({})",
                MAX_ITERATIONS
            ));
        }

        info!("Skill LLM iteration {} for '{}'", iterations, skill.name());

        // Make LLM request
        let response = provider.generate(request.clone()).await?;

        // Extract content - handle Option
        let content = response.content.unwrap_or_default();

        // Add assistant response to conversation
        if let Some(tool_calls) = &response.tool_calls {
            messages.push(Message::assistant_with_tools(
                content.clone(),
                tool_calls.clone(),
            ));
        } else {
            messages.push(Message::assistant(content.clone()));
        }

        // Check if there are tool calls to handle
        if let Some(tool_calls) = response.tool_calls {
            if !tool_calls.is_empty() {
                info!(
                    "Skill '{}' made {} tool calls",
                    skill.name(),
                    tool_calls.len()
                );

                // Execute each tool call
                for tool_call in tool_calls {
                    // Extract function name and arguments
                    if let Some(function) = &tool_call.function {
                        let tool_name = &function.name;
                        let tool_args_str = &function.arguments;

                        debug!(
                            "Executing tool '{}' for skill '{}'",
                            tool_name,
                            skill.name()
                        );

                        // Parse arguments as JSON
                        let tool_args = serde_json::from_str::<Value>(tool_args_str)
                            .unwrap_or_else(|_| serde_json::json!({}));

                        // Execute tool via registry
                        let tool_result =
                            match tool_registry.execute_tool_ref(tool_name, &tool_args).await {
                                Ok(result) => result.to_string(),
                                Err(e) => {
                                    warn!("Tool '{}' failed: {}", tool_name, e);
                                    format!("Error executing {}: {}", tool_name, e)
                                }
                            };

                        // Add tool result to conversation
                        messages.push(Message::tool_response(tool_call.id.clone(), tool_result));
                    } else {
                        warn!("Tool call has no function: {:?}", tool_call.call_type);
                    }
                }

                // Update request for next iteration
                request.messages = messages.clone();

                // Continue loop to process tool results
            } else {
                // No tool calls, return the text response
                return Ok(content);
            }
        } else {
            // No tool calls, return the final response
            return Ok(content);
        }

        // Check finish reason
        match response.finish_reason {
            FinishReason::Stop => {
                // Normal termination
                return Ok(content);
            }
            FinishReason::ToolCalls => {
                // Continue to handle tool calls (already handled above)
            }
            FinishReason::Length => {
                warn!("Skill '{}' hit token limit", skill.name());
                return Ok(content);
            }
            FinishReason::ContentFilter => {
                warn!(
                    "Skill '{}' response filtered by content policy",
                    skill.name()
                );
                return Ok(content);
            }
            FinishReason::Error(ref msg) => {
                return Err(anyhow!("LLM error during skill execution: {}", msg));
            }
            FinishReason::Pause => {
                // For skill execution, treatment is similar to ToolCalls: we continue the loop
                // to process whatever triggered the pause (usually server-side tool use).
            }
            FinishReason::Refusal => {
                return Err(anyhow!(
                    "LLM refused to continue generating response due to policy violations"
                ));
            }
        }
    }
}

/// Adapter implementing Tool trait for a Skill
#[derive(Clone)]
pub struct SkillToolAdapter {
    skill: Skill,
}

impl SkillToolAdapter {
    /// Create a new skill tool adapter
    pub fn new(skill: Skill) -> Self {
        SkillToolAdapter { skill }
    }

    /// Get reference to underlying skill
    pub fn skill(&self) -> &Skill {
        &self.skill
    }

    /// Get mutable reference to underlying skill
    pub fn skill_mut(&mut self) -> &mut Skill {
        &mut self.skill
    }

    /// Execute skill by invoking LLM with skill instructions as system prompt
    async fn execute_skill_with_lm(&self, user_input: Value) -> Result<Value> {
        debug!("Executing skill: {}", self.skill.name());

        // Return structured result with skill instructions and context
        // The agent harness will use this to invoke an LLM sub-call with:
        // 1. Skill instructions as system prompt
        // 2. User input in the message
        // 3. Available tools for the skill to use
        Ok(serde_json::json!({
            "skill_name": self.skill.name(),
            "status": "executing",
            "description": self.skill.description(),
            "instructions": self.skill.instructions,
            "resources_available": self.skill.list_resources(),
            "user_input": user_input,
            "version": self.skill.manifest.version.clone(),
            "author": self.skill.manifest.author.clone(),
        }))
    }
}

#[async_trait]
impl crate::tools::traits::Tool for SkillToolAdapter {
    async fn execute(&self, args: Value) -> Result<Value> {
        info!("Skill tool executing: {}", self.skill.name());

        // Execute skill with LLM
        let result = self.execute_skill_with_lm(args).await?;

        Ok(result)
    }

    fn name(&self) -> &'static str {
        // SAFETY: Skill names are validated and stored in heap; this is unsafe
        // but necessary for Tool trait's &'static str requirement.
        // In practice, we should refactor Tool trait to use &str
        let s: Box<str> = self.skill.name().into();
        Box::leak(s)
    }

    fn description(&self) -> &'static str {
        let s: Box<str> = self.skill.description().into();
        Box::leak(s)
    }

    fn validate_args(&self, args: &Value) -> Result<()> {
        // Skills are flexible; accept any args
        // The skill instructions will guide the LLM on what to do with them
        if args.is_null() {
            return Ok(());
        }
        Ok(())
    }

    fn parameter_schema(&self) -> Option<Value> {
        // Skills are flexible, accept any input
        Some(serde_json::json!({
            "type": "object",
            "description": "Flexible input for skill execution",
            "additionalProperties": true,
        }))
    }

    fn default_permission(&self) -> ToolPolicy {
        // Skills require explicit permission due to potential resource usage
        ToolPolicy::Prompt
    }

    fn allow_patterns(&self) -> Option<&'static [&'static str]> {
        // Skills can define their own patterns, but by default none
        None
    }

    fn deny_patterns(&self) -> Option<&'static [&'static str]> {
        None
    }

    fn prompt_path(&self) -> Option<Cow<'static, str>> {
        // Skills can bundle companion prompts
        Some(Cow::Borrowed("skills/skill_instructions.md"))
    }
}

/// Skill execution context passed to sub-LLM calls
pub struct SkillExecutionContext {
    pub skill_name: String,
    pub instructions: String,
    pub available_tools: Vec<String>,
    pub user_input: Value,
}

impl SkillExecutionContext {
    pub fn new(skill: &Skill, user_input: Value, available_tools: Vec<String>) -> Self {
        SkillExecutionContext {
            skill_name: skill.name().to_string(),
            instructions: skill.instructions.clone(),
            available_tools,
            user_input,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::types::SkillManifest;
    use crate::tools::traits::Tool;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_skill_tool_adapter_name() {
        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test skill".to_string(),
            vtcode_native: Some(true),
            ..Default::default()
        };

        let skill = Skill::new(
            manifest,
            PathBuf::from("/tmp"),
            "# Instructions".to_string(),
        )
        .expect("failed to create skill");

        let adapter = SkillToolAdapter::new(skill);
        assert_eq!(adapter.name(), "test-skill");
    }

    #[tokio::test]
    async fn test_skill_tool_adapter_execute() {
        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test skill".to_string(),
            vtcode_native: Some(true),
            ..Default::default()
        };

        let skill = Skill::new(
            manifest,
            PathBuf::from("/tmp"),
            "# Test Instructions".to_string(),
        )
        .expect("failed to create skill");

        let adapter = SkillToolAdapter::new(skill);
        let args = serde_json::json!({"test": "value"});
        let result = adapter.execute(args).await;

        assert!(result.is_ok());
        let res = result.unwrap();
        assert_eq!(res["skill_name"], "test-skill");
        assert_eq!(res["status"], "executing");
    }

    #[test]
    fn test_filter_tools_no_network_policy() {
        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test".to_string(),
            network_policy: None,
            vtcode_native: Some(true),
            ..Default::default()
        };
        let skill = Skill::new(manifest, PathBuf::from("/tmp"), "instructions".to_string())
            .expect("failed to create skill");

        let tools = vec![
            ToolDefinition::function(
                "read_file".to_string(),
                "Read".to_string(),
                serde_json::json!({}),
            ),
            ToolDefinition::function(
                "web_search".to_string(),
                "Search".to_string(),
                serde_json::json!({}),
            ),
        ];
        let filtered = filter_tools_for_skill(&skill, tools);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].function.as_ref().unwrap().name, "read_file");
    }

    #[test]
    fn test_filter_tools_with_network_policy() {
        use crate::skills::types::SkillNetworkPolicy;
        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test".to_string(),
            network_policy: Some(SkillNetworkPolicy {
                allowed_domains: vec!["api.example.com".to_string()],
                denied_domains: vec![],
            }),
            vtcode_native: Some(true),
            ..Default::default()
        };
        let skill = Skill::new(manifest, PathBuf::from("/tmp"), "instructions".to_string())
            .expect("failed to create skill");

        let tools = vec![
            ToolDefinition::function(
                "read_file".to_string(),
                "Read".to_string(),
                serde_json::json!({}),
            ),
            ToolDefinition::function(
                "web_search".to_string(),
                "Search".to_string(),
                serde_json::json!({}),
            ),
        ];
        let filtered = filter_tools_for_skill(&skill, tools);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_skill_execution_context() {
        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test skill".to_string(),
            vtcode_native: Some(true),
            ..Default::default()
        };

        let skill = Skill::new(manifest, PathBuf::from("/tmp"), "Instructions".to_string())
            .expect("failed to create skill");

        let tools = vec!["file_ops".to_string(), "shell".to_string()];
        let input = serde_json::json!({"test": "input"});

        let ctx = SkillExecutionContext::new(&skill, input, tools);
        assert_eq!(ctx.skill_name, "test-skill");
        assert_eq!(ctx.available_tools.len(), 2);
    }
}
