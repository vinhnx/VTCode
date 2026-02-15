use anyhow::{Context, Result, ensure};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Top-level configuration for automation hooks and lifecycle events
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct HooksConfig {
    /// Configuration for lifecycle-based shell command execution
    #[serde(default)]
    pub lifecycle: LifecycleHooksConfig,
}

/// Configuration for hooks triggered during distinct agent lifecycle events.
/// Each event supports a list of groups with optional matchers.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct LifecycleHooksConfig {
    /// Commands to run immediately when an agent session begins
    #[serde(default)]
    pub session_start: Vec<HookGroupConfig>,

    /// Commands to run when an agent session ends
    #[serde(default)]
    pub session_end: Vec<HookGroupConfig>,

    /// Commands to run when the user submits a prompt (pre-processing)
    #[serde(default)]
    pub user_prompt_submit: Vec<HookGroupConfig>,

    /// Commands to run immediately before a tool is executed
    #[serde(default)]
    pub pre_tool_use: Vec<HookGroupConfig>,

    /// Commands to run immediately after a tool returns its output
    #[serde(default)]
    pub post_tool_use: Vec<HookGroupConfig>,

    /// Commands to run when the agent indicates task completion (pre-exit)
    #[serde(default)]
    pub task_completion: Vec<HookGroupConfig>,

    /// Commands to run after a task is finalized and session is closed
    #[serde(default)]
    pub task_completed: Vec<HookGroupConfig>,

    /// Commands to run when a teammate agent remains idle
    #[serde(default)]
    pub teammate_idle: Vec<HookGroupConfig>,
}

impl LifecycleHooksConfig {
    pub fn is_empty(&self) -> bool {
        self.session_start.is_empty()
            && self.session_end.is_empty()
            && self.user_prompt_submit.is_empty()
            && self.pre_tool_use.is_empty()
            && self.post_tool_use.is_empty()
            && self.task_completion.is_empty()
            && self.task_completed.is_empty()
            && self.teammate_idle.is_empty()
    }
}

/// A group of hooks sharing a common execution matcher
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct HookGroupConfig {
    /// Optional regex matcher to filter when this group runs.
    /// Matched against context strings (e.g. tool name, project path).
    #[serde(default)]
    pub matcher: Option<String>,

    /// List of hook commands to execute sequentially in this group
    #[serde(default)]
    pub hooks: Vec<HookCommandConfig>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum HookCommandKind {
    #[default]
    Command,
}

/// Configuration for a single shell command hook
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct HookCommandConfig {
    /// Type of hook command (currently only 'command' is supported)
    #[serde(default)]
    #[serde(rename = "type")]
    pub kind: HookCommandKind,

    /// The shell command string to execute
    #[serde(default)]
    pub command: String,

    /// Optional execution timeout in seconds
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
}

impl HooksConfig {
    pub fn validate(&self) -> Result<()> {
        self.lifecycle
            .validate()
            .context("Invalid lifecycle hooks configuration")
    }
}

impl LifecycleHooksConfig {
    pub fn validate(&self) -> Result<()> {
        validate_groups(&self.session_start, "session_start")?;
        validate_groups(&self.session_end, "session_end")?;
        validate_groups(&self.user_prompt_submit, "user_prompt_submit")?;
        validate_groups(&self.pre_tool_use, "pre_tool_use")?;
        validate_groups(&self.post_tool_use, "post_tool_use")?;
        validate_groups(&self.task_completion, "task_completion")?;
        validate_groups(&self.task_completed, "task_completed")?;
        validate_groups(&self.teammate_idle, "teammate_idle")?;
        Ok(())
    }
}

fn validate_groups(groups: &[HookGroupConfig], context_name: &str) -> Result<()> {
    for (index, group) in groups.iter().enumerate() {
        if let Some(pattern) = group.matcher.as_ref() {
            validate_matcher(pattern).with_context(|| {
                format!("Invalid matcher in hooks.{context_name}[{index}] -> matcher")
            })?;
        }

        ensure!(
            !group.hooks.is_empty(),
            "hooks.{context_name}[{index}] must define at least one hook command"
        );

        for (hook_index, hook) in group.hooks.iter().enumerate() {
            ensure!(
                matches!(hook.kind, HookCommandKind::Command),
                "hooks.{context_name}[{index}].hooks[{hook_index}] has unsupported type"
            );

            ensure!(
                !hook.command.trim().is_empty(),
                "hooks.{context_name}[{index}].hooks[{hook_index}] must specify a command"
            );

            if let Some(timeout) = hook.timeout_seconds {
                ensure!(
                    timeout > 0,
                    "hooks.{context_name}[{index}].hooks[{hook_index}].timeout_seconds must be positive"
                );
            }
        }
    }

    Ok(())
}

fn validate_matcher(pattern: &str) -> Result<()> {
    let trimmed = pattern.trim();
    if trimmed.is_empty() || trimmed == "*" {
        return Ok(());
    }

    let regex_pattern = format!("^(?:{})$", trimmed);
    Regex::new(&regex_pattern)
        .with_context(|| format!("failed to compile lifecycle hook matcher: {pattern}"))?;
    Ok(())
}
