use anyhow::{Context, Result};
use regex::Regex;

use vtcode_core::config::{HookCommandConfig, HookGroupConfig, LifecycleHooksConfig};

#[derive(Default)]
pub(super) struct CompiledLifecycleHooks {
    pub(super) session_start: Vec<CompiledHookGroup>,
    pub(super) session_end: Vec<CompiledHookGroup>,
    pub(super) user_prompt_submit: Vec<CompiledHookGroup>,
    pub(super) pre_tool_use: Vec<CompiledHookGroup>,
    pub(super) post_tool_use: Vec<CompiledHookGroup>,
    pub(super) task_completion: Vec<CompiledHookGroup>,
}

impl CompiledLifecycleHooks {
    pub(super) fn from_config(config: &LifecycleHooksConfig) -> Result<Self> {
        Ok(Self {
            session_start: compile_groups(&config.session_start)?,
            session_end: compile_groups(&config.session_end)?,
            user_prompt_submit: compile_groups(&config.user_prompt_submit)?,
            pre_tool_use: compile_groups(&config.pre_tool_use)?,
            post_tool_use: compile_groups(&config.post_tool_use)?,
            task_completion: compile_groups(&config.task_completion)?,
        })
    }

    pub(super) fn is_empty(&self) -> bool {
        self.session_start.is_empty()
            && self.session_end.is_empty()
            && self.user_prompt_submit.is_empty()
            && self.pre_tool_use.is_empty()
            && self.post_tool_use.is_empty()
            && self.task_completion.is_empty()
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
