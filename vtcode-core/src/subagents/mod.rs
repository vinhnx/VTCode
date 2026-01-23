//! Subagent system for VT Code
//!
//! Provides specialized AI subagents that can be delegated tasks with:
//! - Isolated context (separate conversation)
//! - Filtered tool access
//! - Custom system prompts
//! - Model selection (inherit, alias, or specific)
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     Main Agent                               │
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │                 SubagentRegistry                        ││
//! │  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐   ││
//! │  │  │ explore  │ │  plan    │ │ general  │ │ custom   │   ││
//! │  │  │ (haiku)  │ │ (sonnet) │ │ (sonnet) │ │ (config) │   ││
//! │  │  └──────────┘ └──────────┘ └──────────┘ └──────────┘   ││
//! │  └─────────────────────────────────────────────────────────┘│
//! │                           │                                  │
//! │                           ▼                                  │
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │                 SubagentRunner                          ││
//! │  │  • Spawns subagent with filtered tools                  ││
//! │  │  • Manages isolated context                             ││
//! │  │  • Tracks execution in transcript                       ││
//! │  └─────────────────────────────────────────────────────────┘│
//! │                           │                                  │
//! │                           ▼                                  │
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │                 spawn_subagent Tool                     ││
//! │  │  Parameters:                                            ││
//! │  │  • prompt: Task description                             ││
//! │  │  • subagent_type: Optional specific agent               ││
//! │  │  • resume: Optional agent_id for continuation           ││
//! │  │  Returns: SubagentResult with output + agent_id         ││
//! │  └─────────────────────────────────────────────────────────┘│
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Built-in Subagents
//!
//! - **explore**: Fast read-only codebase search (haiku-equivalent)
//! - **plan**: Research specialist for planning mode (sonnet)
//! - **general**: Multi-step tasks with full capabilities (sonnet)
//! - **code-reviewer**: Code quality and security review
//! - **debugger**: Error investigation and fixes
//!
//! # Custom Subagents
//!
//! Create `.vtcode/agents/my-agent.md` with YAML frontmatter:
//!
//! ```markdown
//! ---
//! name: my-agent
//! description: Custom agent for specific tasks
//! tools: read_file, grep_file
//! model: inherit
//! ---
//!
//! Your system prompt here...
//! ```
//!
//! # Skill Library Evolution Pattern
//!
//! The skill library implements the Skill Library Evolution pattern from
//! The Agentic AI Handbook. This enables agents to persist and reuse
//! working solutions across sessions:
//!
//! 1. **Discovery**: Agent writes code to solve an immediate problem
//! 2. **Persistence**: If solution works, save to `.vtcode/skills/`
//! 3. **Generalization**: Refactor for reuse (parameterize hard-coded values)
//! 4. **Documentation**: Add purpose, parameters, returns, examples
//! 5. **Reuse**: Future agents discover via `list_skills`/`load_skill`
//!
//! **Progressive disclosure** achieves 91% token reduction:
//! - Discovery profile: Names and descriptions via `list_skills`
//! - Active instructions: Full `SKILL.md` via `load_skill`
//! - Deep resources: Scripts/docs via `load_skill_resource`
//!
//! See: `.vtcode/skills/INDEX.md` for available skills.
//!
//! # Usage
//!
//! ```rust,ignore
//! use vtcode_core::subagents::{SubagentRegistry, SubagentRunner, SpawnParams};
//!
//! // Load registry
//! let registry = SubagentRegistry::new(workspace, config).await?;
//!
//! // Create runner
//! let runner = SubagentRunner::new(
//!     Arc::new(registry),
//!     agent_config,
//!     tool_registry,
//!     workspace,
//! );
//!
//! // Spawn subagent
//! let result = runner.spawn(
//!     SpawnParams::new("Find all authentication code")
//!         .with_subagent("explore")
//!         .with_thoroughness(Thoroughness::VeryThorough)
//! ).await?;
//!
//! println!("Agent {} completed: {}", result.agent_id, result.output);
//! ```

pub mod registry;
pub mod runner;

#[cfg(test)]
mod test_cleanup;

pub use registry::{RunningSubagent, SubagentRegistry};
pub use runner::{SpawnParams, SubagentResult, SubagentRunner, Thoroughness, TokenUsage};

// Re-export config types
pub use vtcode_config::subagent::{
    SubagentConfig, SubagentModel, SubagentParseError, SubagentPermissionMode, SubagentSource,
    SubagentsConfig,
};

/// Get the system prompt for a built-in agent profile by name.
/// This is a convenience function for the planner/coder subagent architecture.
/// Returns None if the agent is not found or is not a built-in.
///
/// Note: For full registry access, use SubagentRegistry directly.
pub fn get_builtin_agent_prompt(name: &str) -> Option<&'static str> {
    match name {
        "planner" => Some(registry::builtins::PLANNER_AGENT),
        "coder" => Some(registry::builtins::CODER_AGENT),
        "explore" => Some(registry::builtins::EXPLORE_AGENT),
        "plan" => Some(registry::builtins::PLAN_AGENT),
        "general" => Some(registry::builtins::GENERAL_AGENT),
        "code-reviewer" => Some(registry::builtins::CODE_REVIEWER_AGENT),
        "debugger" => Some(registry::builtins::DEBUGGER_AGENT),
        _ => None,
    }
}

/// Extract just the system prompt body from a built-in agent definition.
/// This strips the YAML frontmatter and returns only the markdown body.
pub fn extract_agent_system_prompt(agent_definition: &str) -> Option<String> {
    let content = agent_definition.trim();
    if !content.starts_with("---") {
        return None;
    }

    let after_start = &content[3..];
    let end_pos = after_start.find("\n---")?;
    let body_start = 3 + end_pos + 4; // Skip "---" + yaml + "\n---"

    content
        .get(body_start..)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Get the extracted system prompt for a built-in agent profile.
/// Returns the prompt body without YAML frontmatter.
pub fn get_agent_prompt_body(name: &str) -> Option<String> {
    get_builtin_agent_prompt(name).and_then(extract_agent_system_prompt)
}

#[cfg(test)]
mod active_agent_tests {
    use super::*;

    #[test]
    fn test_get_builtin_agent_prompt_planner() {
        let prompt = get_builtin_agent_prompt("planner");
        assert!(prompt.is_some());
        assert!(prompt.unwrap().contains("name: planner"));
    }

    #[test]
    fn test_get_builtin_agent_prompt_coder() {
        let prompt = get_builtin_agent_prompt("coder");
        assert!(prompt.is_some());
        assert!(prompt.unwrap().contains("name: coder"));
    }

    #[test]
    fn test_get_builtin_agent_prompt_unknown() {
        assert!(get_builtin_agent_prompt("unknown-agent").is_none());
    }

    #[test]
    fn test_extract_agent_system_prompt_valid() {
        let definition = r#"---
name: test
description: Test agent
---

This is the system prompt body.
It has multiple lines."#;

        let body = extract_agent_system_prompt(definition);
        assert!(body.is_some());
        let body = body.unwrap();
        assert!(body.contains("This is the system prompt body"));
        assert!(body.contains("multiple lines"));
        assert!(!body.contains("name: test"));
    }

    #[test]
    fn test_extract_agent_system_prompt_no_frontmatter() {
        let definition = "Just a plain prompt without frontmatter";
        assert!(extract_agent_system_prompt(definition).is_none());
    }

    #[test]
    fn test_get_agent_prompt_body_planner() {
        let body = get_agent_prompt_body("planner");
        assert!(body.is_some());
        let body = body.unwrap();
        assert!(body.contains("PLAN MODE"));
        assert!(!body.contains("name: planner"));
    }

    #[test]
    fn test_get_agent_prompt_body_coder() {
        let body = get_agent_prompt_body("coder");
        assert!(body.is_some());
        let body = body.unwrap();
        assert!(body.contains("CODE MODE"));
        assert!(!body.contains("name: coder"));
    }
}
