use crate::constants::{defaults, tool_limits, tools};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Automation-specific configuration toggles.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct AutomationConfig {
    /// Full-auto execution safeguards.
    #[serde(default)]
    pub full_auto: FullAutoConfig,
    /// Session and durable scheduled task controls.
    #[serde(default)]
    pub scheduled_tasks: ScheduledTasksConfig,
    /// Loop engineering controls for external scheduler integration.
    #[serde(default)]
    pub loop_engine: LoopEngineConfig,
}

/// Controls for the built-in scheduled task subsystem.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScheduledTasksConfig {
    /// Enable scheduler tools and durable `vtcode schedule` jobs.
    #[serde(default = "default_scheduled_tasks_enabled")]
    pub enabled: bool,
}

impl Default for ScheduledTasksConfig {
    fn default() -> Self {
        Self { enabled: default_scheduled_tasks_enabled() }
    }
}

/// Controls for running the agent without interactive approvals.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FullAutoConfig {
    /// Enable the runtime flag once the workspace is configured for autonomous runs.
    #[serde(default = "default_full_auto_enabled")]
    pub enabled: bool,

    /// Maximum number of autonomous agent turns before the exec runner pauses.
    #[serde(default = "default_full_auto_max_turns")]
    pub max_turns: usize,

    /// Allow-list of tools that may execute automatically.
    #[serde(default = "default_full_auto_allowed_tools")]
    pub allowed_tools: Vec<String>,

    /// Require presence of a profile/acknowledgement file before activation.
    #[serde(default = "default_require_profile_ack")]
    pub require_profile_ack: bool,

    /// Optional path to a profile describing acceptable behaviors.
    #[serde(default)]
    pub profile_path: Option<PathBuf>,

    /// Run a read-only verifier sub-agent after each mutating tool call.
    /// When enabled, the harness spawns a verifier that re-reads the diff
    /// and either approves or rejects the change before it is committed.
    /// This doubles cost on mutating calls but catches propose-side errors.
    #[serde(default)]
    verify_mutations: bool,

    /// Automatically grant tool-loop limit increases while a full-auto run is
    /// active, instead of prompting. Grants reuse the same per-prompt
    /// increments and absolute hard caps as manual approvals. Set to `false`
    /// to restore the interactive prompts.
    #[serde(default = "default_auto_grant_tool_limits")]
    pub auto_grant_tool_limits: bool,
}

impl Default for FullAutoConfig {
    fn default() -> Self {
        Self {
            enabled: default_full_auto_enabled(),
            max_turns: default_full_auto_max_turns(),
            allowed_tools: default_full_auto_allowed_tools(),
            require_profile_ack: default_require_profile_ack(),
            profile_path: None,
            verify_mutations: false,
            auto_grant_tool_limits: default_auto_grant_tool_limits(),
        }
    }
}

fn default_full_auto_enabled() -> bool {
    false
}

fn default_scheduled_tasks_enabled() -> bool {
    false
}

fn default_full_auto_allowed_tools() -> Vec<String> {
    vec![
        tools::EXEC_COMMAND.to_string(),
        tools::WRITE_STDIN.to_string(),
        tools::APPLY_PATCH.to_string(),
        tools::CODE_SEARCH.to_string(),
        // Workflow-coordination tools stay available in every mode (see
        // `ALWAYS_AVAILABLE_WORKFLOW_TOOLS`); listing them here keeps the
        // documented default coherent with runtime behavior.
        tools::TASK_TRACKER.to_string(),
        tools::START_PLANNING.to_string(),
        tools::REQUEST_USER_INPUT.to_string(),
    ]
}

fn default_require_profile_ack() -> bool {
    true
}

fn default_auto_grant_tool_limits() -> bool {
    true
}

fn default_full_auto_max_turns() -> usize {
    tool_limits::DEFAULT_FULL_AUTO_MAX_TURNS
}

/// Controls for loop engineering — running vtcode from an external scheduler.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoopEngineConfig {
    /// Enable loop engineering mode. When false, loop-specific features
    /// (reconciler, per-iteration skills) are inactive.
    #[serde(default)]
    enabled: bool,

    /// Optional circuit breaker: maximum loop iterations before the harness
    /// refuses further runs. `None` means unlimited.
    #[serde(default)]
    max_iterations: Option<usize>,

    /// After a worktree-isolated subagent completes, run the reconciler
    /// (diff → verify → merge) automatically.
    #[serde(default = "default_reconcile_on_complete")]
    pub reconcile_on_complete: bool,

    /// Skill names to preload into the agent context at the start of each
    /// loop iteration. Empty means no per-iteration skill injection.
    #[serde(default)]
    preload_skills: Vec<String>,
}

impl Default for LoopEngineConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_iterations: None,
            reconcile_on_complete: default_reconcile_on_complete(),
            preload_skills: Vec::new(),
        }
    }
}

fn default_reconcile_on_complete() -> bool {
    true
}

/// Returns `true` when the loop engine is enabled, respecting the
/// `VTCODE_DISABLE_LOOP_ENGINE` env-var override.
pub fn loop_engine_enabled(config: &AutomationConfig) -> bool {
    if std::env::var("VTCODE_DISABLE_LOOP_ENGINE").is_ok() {
        return false;
    }
    config.loop_engine.enabled
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_auto_defaults_to_shared_turn_budget() {
        assert_eq!(FullAutoConfig::default().max_turns, tool_limits::DEFAULT_FULL_AUTO_MAX_TURNS);
    }

    #[test]
    fn full_auto_grants_loop_limits_without_prompting_by_default() {
        assert!(FullAutoConfig::default().auto_grant_tool_limits);
    }

    #[test]
    fn full_auto_loop_grants_survive_missing_field_for_backward_compatibility() {
        let without_field = toml::from_str::<FullAutoConfig>("enabled = true").expect("minimal full-auto parses");
        assert!(without_field.auto_grant_tool_limits);

        let opted_out = toml::from_str::<FullAutoConfig>("enabled = true\nauto_grant_tool_limits = false")
            .expect("explicit opt-out parses");
        assert!(!opted_out.auto_grant_tool_limits);
    }
}
