use anyhow::{Context, Result, ensure};
use regex::Regex;
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct HooksConfig {
    #[serde(default)]
    pub lifecycle: LifecycleHooksConfig,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct LifecycleHooksConfig {
    #[serde(default)]
    pub session_start: Vec<HookGroupConfig>,
    #[serde(default)]
    pub session_end: Vec<HookGroupConfig>,
    #[serde(default)]
    pub user_prompt_submit: Vec<HookGroupConfig>,
    #[serde(default)]
    pub pre_tool_use: Vec<HookGroupConfig>,
    #[serde(default)]
    pub post_tool_use: Vec<HookGroupConfig>,
}

impl LifecycleHooksConfig {
    pub fn is_empty(&self) -> bool {
        self.session_start.is_empty()
            && self.session_end.is_empty()
            && self.user_prompt_submit.is_empty()
            && self.pre_tool_use.is_empty()
            && self.post_tool_use.is_empty()
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct HookGroupConfig {
    #[serde(default)]
    pub matcher: Option<String>,
    #[serde(default)]
    pub hooks: Vec<HookCommandConfig>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HookCommandKind {
    Command,
}

impl Default for HookCommandKind {
    fn default() -> Self {
        Self::Command
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct HookCommandConfig {
    #[serde(default)]
    #[serde(rename = "type")]
    pub kind: HookCommandKind,
    #[serde(default)]
    pub command: String,
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
