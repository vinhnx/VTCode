use anyhow::{Context, Result};
use regex::Regex;

use crate::config::{HookCommandConfig, HookGroupConfig, LifecycleHooksConfig};

#[derive(Default)]
pub(super) struct CompiledLifecycleHooks {
    pub(super) quiet_success_output: bool,
    pub(super) session_start: Vec<CompiledHookGroup>,
    pub(super) session_end: Vec<CompiledHookGroup>,
    pub(super) subagent_start: Vec<CompiledHookGroup>,
    pub(super) subagent_stop: Vec<CompiledHookGroup>,
    pub(super) user_prompt_submit: Vec<CompiledHookGroup>,
    pub(super) pre_tool_use: Vec<CompiledHookGroup>,
    pub(super) post_tool_use: Vec<CompiledHookGroup>,
    pub(super) permission_request: Vec<CompiledHookGroup>,
    pub(super) pre_compact: Vec<CompiledHookGroup>,
    pub(super) stop: Vec<CompiledHookGroup>,
    pub(super) notification: Vec<CompiledHookGroup>,
}

impl CompiledLifecycleHooks {
    pub(super) fn from_config(config: &LifecycleHooksConfig) -> Result<Self> {
        let normalized = config.normalized();
        Ok(Self {
            quiet_success_output: normalized.quiet_success_output,
            session_start: compile_groups(&normalized.session_start)?,
            session_end: compile_groups(&normalized.session_end)?,
            subagent_start: compile_groups(&normalized.subagent_start)?,
            subagent_stop: compile_groups(&normalized.subagent_stop)?,
            user_prompt_submit: compile_groups(&normalized.user_prompt_submit)?,
            pre_tool_use: compile_groups(&normalized.pre_tool_use)?,
            post_tool_use: compile_groups(&normalized.post_tool_use)?,
            permission_request: compile_groups(&normalized.permission_request)?,
            pre_compact: compile_groups(&normalized.pre_compact)?,
            stop: compile_groups(&normalized.stop)?,
            notification: compile_groups(&normalized.notification)?,
        })
    }

    pub(super) fn is_empty(&self) -> bool {
        self.session_start.is_empty()
            && self.session_end.is_empty()
            && self.subagent_start.is_empty()
            && self.subagent_stop.is_empty()
            && self.user_prompt_submit.is_empty()
            && self.pre_tool_use.is_empty()
            && self.post_tool_use.is_empty()
            && self.permission_request.is_empty()
            && self.pre_compact.is_empty()
            && self.stop.is_empty()
            && self.notification.is_empty()
    }
}

pub(super) struct CompiledHookGroup {
    pub(super) matcher: HookMatcher,
    pub(super) commands: Vec<HookCommandConfig>,
}

#[derive(Clone)]
pub(super) enum HookMatcher {
    Any,
    Pattern(Regex),
}

impl HookMatcher {
    pub(super) fn matches(&self, value: &str) -> bool {
        match self {
            Self::Any => true,
            Self::Pattern(regex) => regex.is_match(value),
        }
    }
}

fn compile_groups(groups: &[HookGroupConfig]) -> Result<Vec<CompiledHookGroup>> {
    let mut compiled = Vec::new();
    for group in groups {
        let matcher = if let Some(pattern) = group.matcher.as_ref() {
            let trimmed = pattern.trim();
            if trimmed.is_empty() || trimmed == "*" {
                HookMatcher::Any
            } else {
                let regex = Regex::new(&format!("^(?:{})$", trimmed))
                    .with_context(|| format!("invalid lifecycle hook matcher: {pattern}"))?;
                HookMatcher::Pattern(regex)
            }
        } else {
            HookMatcher::Any
        };

        compiled.push(CompiledHookGroup {
            matcher,
            commands: group.hooks.clone(),
        });
    }

    Ok(compiled)
}
