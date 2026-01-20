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
