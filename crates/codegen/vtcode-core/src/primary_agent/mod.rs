use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use vtcode_config::constants::{defaults::DEFAULT_PRIMARY_AGENT_NAME, tools};
use vtcode_config::core::permissions::AgentPermissionsConfig;
use vtcode_config::core::tools::ToolPolicy;
use vtcode_config::{
    DiscoveredSubagents, HookGroupConfig, HooksConfig, SubagentMcpServer, SubagentMemoryScope, SubagentSource,
    SubagentSpec, builtin_primary_build_agent,
};

use crate::config::{ReasoningEffortLevel, VTCodeConfig};
use crate::llm::provider::ToolDefinition;
use crate::permissions::{PermissionRequest, ResolvedPermissionDecision, evaluate_effective_permissions};
use crate::prompts::PromptContext;
use crate::subagents::{ResolvedAgentRuntimeView, inline_mcp_provider};
use tracing::warn;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivePrimaryAgentSpecIdentity {
    pub name: String,
    pub source: SubagentSource,
    pub file_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActivePrimaryAgent {
    pub identity: ActivePrimaryAgentSpecIdentity,
    pub display_name: String,
    pub description: String,
    pub color: Option<String>,
    pub aliases: Vec<String>,
    pub instructions: String,
    pub tools: Option<Vec<String>>,
    pub disallowed_tools: Vec<String>,
    pub permissions: AgentPermissionsConfig,
    pub model: Option<String>,
    pub reasoning_effort: Option<ReasoningEffortLevel>,
    pub hooks: Option<HooksConfig>,
    pub skills: Vec<String>,
    pub mcp_servers: Vec<SubagentMcpServer>,
    pub memory: Option<SubagentMemoryScope>,
    pub tool_policy_overrides: BTreeMap<String, ToolPolicy>,
}

impl ActivePrimaryAgent {
    #[must_use]
    pub fn from_spec(spec: &SubagentSpec) -> Self {
        Self::from_runtime_view(&ResolvedAgentRuntimeView::from_spec(spec))
    }

    #[must_use]
    pub fn from_runtime_view(runtime: &ResolvedAgentRuntimeView) -> Self {
        Self {
            identity: ActivePrimaryAgentSpecIdentity {
                name: runtime.canonical_name.clone(),
                source: runtime.source.clone(),
                file_path: runtime.file_path.clone(),
            },
            display_name: runtime.display_name.clone(),
            description: runtime.description.clone(),
            color: runtime.color.clone(),
            aliases: runtime.aliases.clone(),
            instructions: runtime.instructions.clone(),
            tools: runtime.tools.clone(),
            disallowed_tools: runtime.disallowed_tools.clone(),
            permissions: runtime.permissions.clone(),
            model: runtime.model.clone(),
            reasoning_effort: runtime.reasoning_effort,
            hooks: runtime.hooks.clone(),
            skills: runtime.skills.clone(),
            mcp_servers: runtime.mcp_servers.clone(),
            memory: runtime.memory,
            tool_policy_overrides: runtime.tool_policy_overrides.clone(),
        }
    }

    /// Canonical primary agent name (the active session "mode").
    #[must_use]
    pub fn name(&self) -> &str {
        &self.identity.name
    }

    /// Whether this agent contributes lifecycle hooks that originate from
    /// workspace-controlled content (project agent-spec files inside the
    /// repository). Such hooks must be gated behind the workspace lifecycle
    /// hook approval just like hooks declared in a workspace `vtcode.toml`.
    #[must_use]
    pub fn contributes_workspace_controlled_hooks(&self) -> bool {
        self.hooks.is_some()
            && matches!(
                self.identity.source,
                SubagentSource::ProjectVtcode | SubagentSource::ProjectClaude | SubagentSource::ProjectCodex
            )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActivePrimaryAgentState {
    active: ActivePrimaryAgent,
}

impl Default for ActivePrimaryAgentState {
    fn default() -> Self {
        Self {
            active: ActivePrimaryAgent::from_spec(&builtin_primary_build_agent()),
        }
    }
}

impl ActivePrimaryAgentState {
    #[must_use]
    pub const fn active(&self) -> &ActivePrimaryAgent {
        &self.active
    }

    #[must_use]
    pub fn from_discovery(discovered: &DiscoveredSubagents) -> Self {
        Self::from_specs(&discovered.effective)
    }

    #[must_use]
    pub fn from_specs(specs: &[SubagentSpec]) -> Self {
        Self::from_specs_with_default(specs, DEFAULT_PRIMARY_AGENT_NAME)
    }

    #[must_use]
    pub fn from_specs_with_default(specs: &[SubagentSpec], requested_default: &str) -> Self {
        let requested = if requested_default.trim().is_empty() {
            DEFAULT_PRIMARY_AGENT_NAME
        } else {
            requested_default.trim()
        };
        let active = match resolve_primary_agent(specs, requested) {
            Ok(active) => active,
            Err(_) => {
                if let Some(requested_name) = fallback_notice(requested) {
                    warn!(
                        requested = %requested_name,
                        fallback = %DEFAULT_PRIMARY_AGENT_NAME,
                        "Primary agent '{}' was not discovered; falling back to the built-in '{}' agent. \
                         Set default_primary_agent to a discovered agent or add a matching primary agent.",
                        requested_name,
                        DEFAULT_PRIMARY_AGENT_NAME,
                    );
                }
                ActivePrimaryAgent::from_spec(&builtin_primary_build_agent())
            }
        };
        Self { active }
    }

    pub fn reset_to_default_from_specs(&mut self, specs: &[SubagentSpec]) -> &ActivePrimaryAgent {
        self.active = Self::from_specs(specs).active;
        &self.active
    }

    /// Restore a previously captured active-agent snapshot. Runloop prompt
    /// boundaries use this to keep a write-capable Build handoff intact when
    /// unrelated queued UI input attempts to switch modes mid-turn.
    pub fn restore_snapshot(&mut self, snapshot: ActivePrimaryAgent) -> &ActivePrimaryAgent {
        self.active = snapshot;
        &self.active
    }

    pub fn select_from_discovery(
        &mut self,
        discovered: &DiscoveredSubagents,
        requested: &str,
    ) -> PrimaryAgentResolutionResult<&ActivePrimaryAgent> {
        self.select_from_specs(&discovered.effective, requested)
    }

    pub fn select_from_specs(
        &mut self,
        specs: &[SubagentSpec],
        requested: &str,
    ) -> PrimaryAgentResolutionResult<&ActivePrimaryAgent> {
        let active = resolve_primary_agent(specs, requested)?;
        self.active = active;
        Ok(&self.active)
    }
}

pub type PrimaryAgentResolutionResult<T> = Result<T, PrimaryAgentResolutionError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrimaryAgentResolutionError {
    UnknownAgent { requested: String },
}

impl fmt::Display for PrimaryAgentResolutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownAgent { requested } => write!(f, "Unknown primary agent {requested}"),
        }
    }
}

impl Error for PrimaryAgentResolutionError {}

pub fn resolve_discovered_primary_agent(
    discovered: &DiscoveredSubagents,
    requested: &str,
) -> PrimaryAgentResolutionResult<ActivePrimaryAgent> {
    resolve_primary_agent(&discovered.effective, requested)
}

pub fn resolve_primary_agent(
    specs: &[SubagentSpec],
    requested: &str,
) -> PrimaryAgentResolutionResult<ActivePrimaryAgent> {
    specs
        .iter()
        .find(|spec| spec.is_primary() && spec.name.eq_ignore_ascii_case(requested))
        .or_else(|| specs.iter().find(|spec| spec.is_primary() && spec.matches_name(requested)))
        .map(ActivePrimaryAgent::from_spec)
        .ok_or_else(|| PrimaryAgentResolutionError::UnknownAgent { requested: requested.to_string() })
}

/// Returns `true` for cleanup-only child-agent tools that must remain
/// available regardless of the active primary agent's tool policy.
///
/// NOTE: after `wait_agent`/`close_agent` were folded into the unified
/// `agent` tool (action='wait'/'close'), `LIFECYCLE_CLEANUP_TOOLS` no longer
/// matches any name that is actually *emitted* to the model — the emitted
/// name is always `tools::AGENT` ("agent"). This carve-out therefore only
/// takes effect if a caller passes one of the legacy granular names
/// (`wait_agent`/`close_agent`) directly rather than the folded name.
/// Expressing true per-action granularity at runtime (allow cleanup actions
/// on the folded `agent` tool while denying `spawn`) requires
/// execution-time action gating, the same mechanism used for
/// `mcp:connect`/`mcp:disconnect` (see `action_qualified_policy_name` in
/// `tool_intent.rs`), and is deferred — see
/// `apply_primary_agent_tool_policy`'s doc comment for the specific gap.
fn is_subagent_cleanup_tool(tool_name: &str) -> bool {
    tools::LIFECYCLE_CLEANUP_TOOLS
        .iter()
        .any(|cleanup| cleanup.eq_ignore_ascii_case(tool_name))
}

/// Returns the configured agent name that should be surfaced to the operator
/// when the resolver cannot find it and falls back to the built-in `build`
/// agent, or `None` when the fallback is expected and should stay silent.
///
/// A fallback is expected (and therefore silent) when the operator asked for
/// the default `build` agent — whether explicitly or because no
/// `default_primary_agent` was configured — since the built-in `build` agent
/// *is* the canonical build. Any other requested name that misses discovery is
/// a configuration mismatch the operator should hear about.
///
/// Extracted as a pure predicate so the decision is unit-testable without a
/// tracing subscriber.
fn fallback_notice(requested_default: &str) -> Option<&str> {
    let requested = requested_default.trim();
    if requested.is_empty() || requested.eq_ignore_ascii_case(DEFAULT_PRIMARY_AGENT_NAME) {
        None
    } else {
        Some(requested)
    }
}

#[must_use]
pub fn primary_agent_allows_tool(agent: &ActivePrimaryAgent, tool_name: &str) -> bool {
    let tool_name = tool_name.trim();

    // Cleanup tools stay available so restricted agents can still join or
    // close already-running child work.
    if is_subagent_cleanup_tool(tool_name) {
        return true;
    }

    // The task tracker stays available so restricted primaries (including
    // custom `auto` agents with an explicit `tools:` list) can still report
    // progress. Planning/interview tools stay author-curated: unlike the
    // tracker they change the session mode, so an explicit omission must
    // keep working.
    if tool_name.eq_ignore_ascii_case(tools::TASK_TRACKER) {
        return true;
    }

    let allow_list_allows = agent
        .tools
        .as_ref()
        .is_none_or(|tools| tools.iter().any(|allowed| allowed.trim().eq_ignore_ascii_case(tool_name)));
    if !allow_list_allows {
        return false;
    }

    !agent
        .disallowed_tools
        .iter()
        .any(|denied| denied.trim().eq_ignore_ascii_case(tool_name))
}

/// Filters the emitted tool list down to those `primary_agent_allows_tool`
/// permits for `agent`.
///
/// KNOWN LIMITATION: filtering is by each tool's canonical
/// `function_name()`, and matching only trims and ignores ASCII case — it
/// does not alias-resolve. For multi-action tools that fold several legacy
/// actions into one emitted name (e.g. `agent` folding
/// spawn/wait/close/send_input/resume, or `mcp` folding connect/disconnect),
/// a granular allow/deny entry such as `"spawn_agent"` in
/// `disallowed_tools`/`tools` has no effect on the folded `agent` tool: the
/// whole tool is either present or absent based on whether `"agent"` itself
/// is allowed. Expressing "allow cleanup (wait/close) but deny spawn"
/// therefore cannot be done here; it requires execution-time action gating —
/// the same action-qualified mechanism already used to keep `mcp:connect`/
/// `mcp:disconnect` HITL-gated separately from bare `mcp` (see
/// `action_qualified_policy_name` in `tool_intent.rs`) — applied per-action
/// at the point the subagent tool call is dispatched. That is deferred.
#[must_use]
pub fn apply_primary_agent_tool_policy(
    tools: Option<Arc<Vec<ToolDefinition>>>,
    agent: &ActivePrimaryAgent,
) -> Option<Arc<Vec<ToolDefinition>>> {
    let tools = tools?;
    let Some(first_rejected) = tools
        .iter()
        .position(|tool| !primary_agent_allows_tool(agent, tool.function_name()))
    else {
        return (!tools.is_empty()).then_some(tools);
    };
    let filtered = tools
        .iter()
        .take(first_rejected)
        .chain(
            tools
                .iter()
                .skip(first_rejected + 1)
                .filter(|tool| primary_agent_allows_tool(agent, tool.function_name())),
        )
        .cloned()
        .collect::<Vec<_>>();

    (!filtered.is_empty()).then(|| Arc::new(filtered))
}

#[must_use]
pub fn build_primary_agent_runtime_config(parent: &VTCodeConfig, agent: &ActivePrimaryAgent) -> VTCodeConfig {
    let mut config = parent.clone();
    // "inherit" (and empty) are sentinels meaning "use the parent's model" — mirrors
    // resolve_subagent_model's handling in subagents/model.rs. Built-in primary agents
    // (e.g. "auto", "build") default their `model` field to "inherit", so without this
    // check every plain `exec` run would clobber a real, explicitly configured model
    // (via --model or [agent].default_model) with the literal string "inherit", which
    // then fails ModelId parsing entirely.
    if let Some(model) = agent.model.as_ref() {
        let trimmed = model.trim();
        if !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("inherit") {
            config.agent.default_model = model.clone();
        }
    }
    if let Some(reasoning_effort) = agent.reasoning_effort {
        config.agent.reasoning_effort = reasoning_effort;
    }
    merge_primary_mcp_servers(&mut config, agent.mcp_servers.as_slice());
    config
}

#[must_use]
pub fn build_primary_agent_hook_config(global: &HooksConfig, agent: &ActivePrimaryAgent) -> HooksConfig {
    let mut config = global.clone();
    merge_active_primary_hooks(&mut config, agent.hooks.as_ref());
    config
}

pub fn apply_primary_agent_prompt_context(context: &mut PromptContext, agent: &ActivePrimaryAgent) {
    context.replace_available_skills_with_named(agent.skills.as_slice());
}

#[must_use]
pub fn active_primary_agent_permissions(agent: &ActivePrimaryAgent) -> &AgentPermissionsConfig {
    &agent.permissions
}

pub fn evaluate_active_primary_agent_permissions(
    config: &VTCodeConfig,
    agent: &ActivePrimaryAgent,
    workspace_root: &std::path::Path,
    current_dir: &std::path::Path,
    request: &PermissionRequest,
) -> ResolvedPermissionDecision {
    evaluate_effective_permissions(
        &config.permissions,
        active_primary_agent_permissions(agent),
        workspace_root,
        current_dir,
        request,
    )
}

fn merge_primary_mcp_servers(config: &mut VTCodeConfig, servers: &[SubagentMcpServer]) {
    for server in servers {
        match server {
            SubagentMcpServer::Named(_) => {}
            SubagentMcpServer::Inline(definition) => {
                for (name, value) in definition {
                    if config.mcp.providers.iter().any(|provider| provider.name == *name) {
                        continue;
                    }
                    if let Some(provider) = inline_mcp_provider(name, value) {
                        config.mcp.providers.push(provider);
                    }
                }
            }
        }
    }
}

fn merge_active_primary_hooks(config: &mut HooksConfig, hooks: Option<&HooksConfig>) {
    let Some(hooks) = hooks else {
        return;
    };

    config.lifecycle.quiet_success_output |= hooks.lifecycle.quiet_success_output;
    append_hook_groups(&mut config.lifecycle.session_start, &hooks.lifecycle.session_start);
    append_hook_groups(&mut config.lifecycle.session_end, &hooks.lifecycle.session_end);
    append_hook_groups(&mut config.lifecycle.subagent_start, &hooks.lifecycle.subagent_start);
    append_hook_groups(&mut config.lifecycle.subagent_stop, &hooks.lifecycle.subagent_stop);
    append_hook_groups(&mut config.lifecycle.user_prompt_submit, &hooks.lifecycle.user_prompt_submit);
    append_hook_groups(&mut config.lifecycle.pre_tool_use, &hooks.lifecycle.pre_tool_use);
    append_hook_groups(&mut config.lifecycle.post_tool_use, &hooks.lifecycle.post_tool_use);
    append_hook_groups(&mut config.lifecycle.permission_request, &hooks.lifecycle.permission_request);
    append_hook_groups(&mut config.lifecycle.pre_compact, &hooks.lifecycle.pre_compact);
    append_hook_groups(&mut config.lifecycle.stop, &hooks.lifecycle.stop);
    append_hook_groups(&mut config.lifecycle.notification, &hooks.lifecycle.notification);
}

fn append_hook_groups(target: &mut Vec<HookGroupConfig>, source: &[HookGroupConfig]) {
    target.extend(source.iter().cloned());
}

#[cfg(test)]
mod tests;
