//! Spawn subagent tool for delegating tasks to specialized agents
//!
//! This tool allows the main agent to spawn specialized subagents for
//! specific tasks, providing context isolation and task-specific expertise.

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::config::types::AgentConfig;
use crate::subagents::{
    SpawnParams, SubagentRegistry, SubagentResult, SubagentRunner, Thoroughness,
};
use crate::tool_policy::ToolPolicy;
use crate::tools::ToolRegistry;
use crate::tools::result::ToolResult as SplitToolResult;
use crate::tools::traits::Tool;

/// Tool arguments for spawn_subagent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnSubagentArgs {
    /// Task description for the subagent
    pub prompt: String,

    /// Optional: specific subagent to use (e.g., "explore", "code-reviewer")
    /// If not specified, auto-selects based on prompt
    #[serde(default)]
    pub subagent_type: Option<String>,

    /// Optional: agent ID to resume a previous subagent conversation
    #[serde(default)]
    pub resume: Option<String>,

    /// Optional: thoroughness level for exploration tasks
    /// Values: "quick", "medium", "very_thorough"
    #[serde(default)]
    pub thoroughness: Option<String>,

    /// Optional: timeout in seconds (default: 300)
    #[serde(default)]
    pub timeout_seconds: Option<u64>,

    /// Optional: additional context from the parent agent
    #[serde(default)]
    pub parent_context: Option<String>,
}

/// Spawn subagent tool implementation
pub struct SpawnSubagentTool {
    /// Subagent registry
    registry: Arc<SubagentRegistry>,
    /// Parent agent configuration
    parent_config: AgentConfig,
    /// Parent tool registry
    parent_tools: Arc<ToolRegistry>,
    /// Workspace root
    workspace_root: PathBuf,
}

impl SpawnSubagentTool {
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

    fn parse_thoroughness(s: &str) -> Thoroughness {
        match s.to_lowercase().as_str() {
            "quick" | "fast" => Thoroughness::Quick,
            "very_thorough" | "thorough" | "comprehensive" => Thoroughness::VeryThorough,
            _ => Thoroughness::Medium,
        }
    }
}

#[async_trait]
impl Tool for SpawnSubagentTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        let args: SpawnSubagentArgs =
            serde_json::from_value(args).context("Failed to parse spawn_subagent arguments")?;

        // Build spawn parameters
        let mut params = SpawnParams::new(&args.prompt);

        if let Some(subagent_type) = args.subagent_type {
            params = params.with_subagent(subagent_type);
        }

        if let Some(resume_id) = args.resume {
            params = params.with_resume(resume_id);
        }

        if let Some(thoroughness) = args.thoroughness {
            params = params.with_thoroughness(Self::parse_thoroughness(&thoroughness));
        }

        if let Some(timeout_secs) = args.timeout_seconds {
            params = params.with_timeout(Duration::from_secs(timeout_secs));
        }

        if let Some(context) = args.parent_context {
            params = params.with_parent_context(context);
        }

        // Create runner and spawn subagent
        let runner = SubagentRunner::new(
            self.registry.clone(),
            self.parent_config.clone(),
            self.parent_tools.clone(),
            self.workspace_root.clone(),
        );

        // Surface any spawn errors as a structured error payload so the tool
        // pipeline can render the actual message instead of a generic failure.
        let result = match runner.spawn(params).await {
            Ok(result) => result,
            Err(err) => {
                return Ok(json!({ "error": err.to_string() }));
            }
        };

        // Convert result to JSON
        Ok(serde_json::to_value(&result)?)
    }

    async fn execute_dual(&self, args: Value) -> Result<SplitToolResult> {
        let result = self.execute(args).await?;

        // Parse the result for formatted output
        let subagent_result: SubagentResult = serde_json::from_value(result.clone())?;

        // Create concise LLM summary
        let llm_summary = if subagent_result.success {
            format!(
                "Subagent '{}' completed (id: {}, {}ms, {} turns). Output:\n{}",
                subagent_result.subagent_name,
                subagent_result.agent_id,
                subagent_result.duration_ms,
                subagent_result.turn_count,
                truncate_for_llm(&subagent_result.output, 2000)
            )
        } else {
            format!(
                "Subagent '{}' failed (id: {}): {}",
                subagent_result.subagent_name,
                subagent_result.agent_id,
                subagent_result.error.unwrap_or_default()
            )
        };

        // Full output for UI
        let ui_content = format!(
            "## Subagent Execution: {}\n\n\
            **Agent ID:** `{}`\n\
            **Status:** {}\n\
            **Duration:** {}ms\n\
            **Turns:** {}\n\n\
            ### Output\n\n{}",
            subagent_result.subagent_name,
            subagent_result.agent_id,
            if subagent_result.success {
                "✓ Success"
            } else {
                "✗ Failed"
            },
            subagent_result.duration_ms,
            subagent_result.turn_count,
            subagent_result.output
        );

        Ok(SplitToolResult::new(self.name(), llm_summary, ui_content))
    }

    fn name(&self) -> &'static str {
        "spawn_subagent"
    }

    fn description(&self) -> &'static str {
        "Spawn a specialized subagent to handle a specific task. Subagents operate with isolated \
        context and can be specialized for different purposes (explore, plan, code-review, debug, \
        or custom). Use this tool when a task benefits from focused expertise or when you want to \
        preserve the main conversation's context."
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(json!({
            "type": "object",
            "required": ["prompt"],
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Task description for the subagent. Be specific about what you want the subagent to accomplish."
                },
                "subagent_type": {
                    "type": "string",
                    "description": "Optional: specific subagent to use. Built-in options: 'explore' (fast read-only search), 'plan' (research for planning), 'general' (full capabilities), 'code-reviewer', 'debugger'. If not specified, auto-selects based on prompt."
                },
                "resume": {
                    "type": "string",
                    "description": "Optional: agent ID from a previous execution to resume the conversation."
                },
                "thoroughness": {
                    "type": "string",
                    "description": "Optional: thoroughness level for exploration tasks. Options: 'quick', 'medium', 'very_thorough'. Default: 'medium'. Any unrecognized value defaults to 'medium'."
                },
                "timeout_seconds": {
                    "type": "integer",
                    "description": "Optional: execution timeout in seconds. Default: 300 (5 minutes)."
                },
                "parent_context": {
                    "type": "string",
                    "description": "Optional: additional context to pass from the parent agent."
                }
            }
        }))
    }

    fn default_permission(&self) -> ToolPolicy {
        ToolPolicy::Prompt
    }
}

/// Truncate text for LLM context efficiency
fn truncate_for_llm(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        text.to_string()
    } else {
        let truncated = &text[..max_chars];
        format!(
            "{}...\n[truncated, {} more chars]",
            truncated,
            text.len() - max_chars
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_thoroughness() {
        assert_eq!(
            SpawnSubagentTool::parse_thoroughness("quick"),
            Thoroughness::Quick
        );
        assert_eq!(
            SpawnSubagentTool::parse_thoroughness("fast"),
            Thoroughness::Quick
        );
        assert_eq!(
            SpawnSubagentTool::parse_thoroughness("medium"),
            Thoroughness::Medium
        );
        assert_eq!(
            SpawnSubagentTool::parse_thoroughness("very_thorough"),
            Thoroughness::VeryThorough
        );
        assert_eq!(
            SpawnSubagentTool::parse_thoroughness("comprehensive"),
            Thoroughness::VeryThorough
        );
        assert_eq!(
            SpawnSubagentTool::parse_thoroughness("unknown"),
            Thoroughness::Medium
        );
    }

    #[test]
    fn test_truncate_for_llm() {
        let short = "short text";
        assert_eq!(truncate_for_llm(short, 100), short);

        let long = "a".repeat(1000);
        let truncated = truncate_for_llm(&long, 100);
        assert!(truncated.contains("..."));
        assert!(truncated.contains("[truncated"));
    }
}
