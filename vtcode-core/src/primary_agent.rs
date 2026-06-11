use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use vtcode_config::constants::defaults::DEFAULT_PRIMARY_AGENT_NAME;
use vtcode_config::core::permissions::AgentPermissionsConfig;
use vtcode_config::{
    DiscoveredSubagents, HookGroupConfig, HooksConfig, McpProviderConfig, SubagentMcpServer,
    SubagentMemoryScope, SubagentSource, SubagentSpec, builtin_primary_duck_agent,
};

use crate::config::{ReasoningEffortLevel, VTCodeConfig};
use crate::llm::provider::ToolDefinition;
use crate::permissions::{
    PermissionRequest, ResolvedPermissionDecision, evaluate_effective_permissions,
};
use crate::prompts::PromptContext;
use crate::subagents::ResolvedAgentRuntimeView;

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
    pub reasoning_effort: Option<String>,
    pub hooks: Option<HooksConfig>,
    pub skills: Vec<String>,
    pub mcp_servers: Vec<SubagentMcpServer>,
    pub memory: Option<SubagentMemoryScope>,
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
            reasoning_effort: runtime.reasoning_effort.clone(),
            hooks: runtime.hooks.clone(),
            skills: runtime.skills.clone(),
            mcp_servers: runtime.mcp_servers.clone(),
            memory: runtime.memory,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActivePrimaryAgentState {
    active: ActivePrimaryAgent,
}

impl Default for ActivePrimaryAgentState {
    fn default() -> Self {
        Self {
            active: ActivePrimaryAgent::from_spec(&builtin_primary_duck_agent()),
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
        let active = resolve_primary_agent(specs, requested)
            .unwrap_or_else(|_| ActivePrimaryAgent::from_spec(&builtin_primary_duck_agent()));
        Self { active }
    }

    pub fn reset_to_default_from_specs(&mut self, specs: &[SubagentSpec]) -> &ActivePrimaryAgent {
        self.active = Self::from_specs(specs).active;
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
        .or_else(|| {
            specs
                .iter()
                .find(|spec| spec.is_primary() && spec.matches_name(requested))
        })
        .map(ActivePrimaryAgent::from_spec)
        .ok_or_else(|| PrimaryAgentResolutionError::UnknownAgent {
            requested: requested.to_string(),
        })
}

/// Returns `true` when `tool_name` is a subagent lifecycle tool that must
/// remain available regardless of the active primary agent's tool policy.
/// These tools manage running subagents (spawn, wait, close, send input,
/// resume, background subprocess). Blocking them would orphan active
/// subagents when a restricted primary agent (e.g. `plan`) is selected.
fn is_subagent_lifecycle_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "spawn_agent"
            | "spawn_background_subprocess"
            | "send_input"
            | "wait_agent"
            | "resume_agent"
            | "close_agent"
    )
}

#[must_use]
pub fn primary_agent_allows_tool(agent: &ActivePrimaryAgent, tool_name: &str) -> bool {
    let tool_name = normalise_tool_name(tool_name);

    // Subagent lifecycle tools are always allowed -- they manage running
    // subagents regardless of which primary agent is active.
    if is_subagent_lifecycle_tool(&tool_name) {
        return true;
    }

    let allow_list_allows = agent.tools.as_ref().is_none_or(|tools| {
        tools
            .iter()
            .any(|allowed| normalise_tool_name(allowed) == tool_name)
    });
    if !allow_list_allows {
        return false;
    }

    !agent
        .disallowed_tools
        .iter()
        .any(|denied| normalise_tool_name(denied) == tool_name)
}

#[must_use]
pub fn apply_primary_agent_tool_policy(
    tools: Option<Arc<Vec<ToolDefinition>>>,
    agent: &ActivePrimaryAgent,
) -> Option<Arc<Vec<ToolDefinition>>> {
    let tools = tools?;
    let filtered = tools
        .iter()
        .filter(|tool| primary_agent_allows_tool(agent, tool.function_name()))
        .cloned()
        .collect::<Vec<_>>();

    (!filtered.is_empty()).then(|| Arc::new(filtered))
}

#[must_use]
pub fn build_primary_agent_runtime_config(
    parent: &VTCodeConfig,
    agent: &ActivePrimaryAgent,
) -> VTCodeConfig {
    let mut config = parent.clone();
    if let Some(model) = agent.model.as_ref() {
        config.agent.default_model = model.clone();
    }
    if let Some(reasoning_effort) = agent
        .reasoning_effort
        .as_deref()
        .and_then(ReasoningEffortLevel::parse)
    {
        config.agent.reasoning_effort = reasoning_effort;
    }
    merge_primary_mcp_servers(&mut config, agent.mcp_servers.as_slice());
    config
}

#[must_use]
pub fn build_primary_agent_hook_config(
    global: &HooksConfig,
    agent: &ActivePrimaryAgent,
) -> HooksConfig {
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

#[must_use]
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

fn normalise_tool_name(tool_name: &str) -> String {
    tool_name.trim().to_ascii_lowercase()
}

fn merge_primary_mcp_servers(config: &mut VTCodeConfig, servers: &[SubagentMcpServer]) {
    for server in servers {
        match server {
            SubagentMcpServer::Named(_) => {}
            SubagentMcpServer::Inline(definition) => {
                for (name, value) in definition {
                    if config
                        .mcp
                        .providers
                        .iter()
                        .any(|provider| provider.name == *name)
                    {
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
    append_hook_groups(
        &mut config.lifecycle.user_prompt_submit,
        &hooks.lifecycle.user_prompt_submit,
    );
    append_hook_groups(
        &mut config.lifecycle.pre_tool_use,
        &hooks.lifecycle.pre_tool_use,
    );
    append_hook_groups(
        &mut config.lifecycle.post_tool_use,
        &hooks.lifecycle.post_tool_use,
    );
    append_hook_groups(
        &mut config.lifecycle.permission_request,
        &hooks.lifecycle.permission_request,
    );
    append_hook_groups(
        &mut config.lifecycle.pre_compact,
        &hooks.lifecycle.pre_compact,
    );
    append_hook_groups(&mut config.lifecycle.stop, &hooks.lifecycle.stop);
    append_hook_groups(
        &mut config.lifecycle.notification,
        &hooks.lifecycle.notification,
    );
}

fn append_hook_groups(target: &mut Vec<HookGroupConfig>, source: &[HookGroupConfig]) {
    target.extend(source.iter().cloned());
}

fn inline_mcp_provider(name: &str, value: &serde_json::Value) -> Option<McpProviderConfig> {
    let object = value.as_object()?;
    let mut payload = serde_json::Map::with_capacity(object.len().saturating_add(1));
    payload.insert(
        "name".to_string(),
        serde_json::Value::String(name.to_string()),
    );
    for (key, value) in object {
        if key == "type" {
            continue;
        }
        payload.insert(key.clone(), value.clone());
    }
    if payload.contains_key("command") && !payload.contains_key("args") {
        payload.insert("args".to_string(), serde_json::Value::Array(Vec::new()));
    }
    serde_json::from_value(serde_json::Value::Object(payload)).ok()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;
    use tempfile::TempDir;
    use vtcode_config::core::permissions::{AgentPermissionsConfig, PermissionDefault};
    use vtcode_config::{
        HookCommandConfig, HooksConfig, SubagentDiscoveryInput, SubagentMcpServer,
        SubagentMemoryScope, SubagentSource, builtin_subagents, discover_subagents,
    };

    use crate::config::constants::tools;
    use crate::permissions::{ResolvedPermissionDecision, build_permission_request};

    use super::*;

    #[test]
    fn resolves_existing_spec_by_name() {
        let spec = test_spec("planner");
        let active = resolve_primary_agent(&[spec], "planner").expect("resolved");

        assert_eq!(active.identity.name, "planner");
        assert_eq!(active.display_name, "planner");
        assert_eq!(active.description, "planner description");
        assert_eq!(active.color.as_deref(), Some("blue"));
        assert_eq!(active.instructions, "planner instructions");
        assert_eq!(active.tools, Some(vec!["unified_search".to_string()]));
        assert_eq!(active.disallowed_tools, vec!["unified_file".to_string()]);
        assert_eq!(active.permissions.default, PermissionDefault::Deny);
        assert_eq!(active.model.as_deref(), Some("gpt-5.1"));
        assert_eq!(active.reasoning_effort.as_deref(), Some("high"));
        assert!(active.hooks.is_none());
    }

    #[test]
    fn unknown_agent_error_preserves_current_active_agent() {
        let current = test_spec("current");
        let specs = vec![current.clone()];
        let mut state = ActivePrimaryAgentState::default();
        let original = state
            .select_from_specs(&specs, "current")
            .expect("initial selection")
            .clone();

        let error = state
            .select_from_specs(&specs, "missing")
            .expect_err("unknown agent");

        assert_eq!(
            error,
            PrimaryAgentResolutionError::UnknownAgent {
                requested: "missing".to_string()
            }
        );
        assert_eq!(state.active(), &original);
    }

    #[test]
    fn alias_resolution_uses_existing_matches_name_semantics() {
        let mut spec = test_spec("reviewer");
        spec.aliases = vec!["critic".to_string()];

        let active = resolve_primary_agent(&[spec], "CRITIC").expect("resolved by alias");

        assert_eq!(active.identity.name, "reviewer");
        assert_eq!(active.display_name, "reviewer");
    }

    #[test]
    fn exact_name_resolution_wins_over_alias() {
        let mut build = test_spec("build");
        build.aliases = vec!["builder".to_string()];
        let builder = test_spec("builder");

        let active = resolve_primary_agent(&[build, builder], "builder").expect("resolved");

        assert_eq!(active.identity.name, "builder");
    }

    #[test]
    fn ignored_subagent_fields_do_not_enter_primary_agent_runtime() {
        let mut spec = test_spec("worker");
        spec.aliases = vec!["builder".to_string()];
        spec.skills = vec!["rust".to_string()];
        spec.mcp_servers = vec![SubagentMcpServer::Named("filesystem".to_string())];
        spec.background = true;
        spec.max_turns = Some(12);
        spec.nickname_candidates = vec!["w".to_string()];
        spec.initial_prompt = Some("start here".to_string());
        spec.memory = Some(SubagentMemoryScope::Project);
        spec.isolation = Some("full".to_string());

        let active = ActivePrimaryAgent::from_spec(&spec);

        assert_eq!(active.identity.name, "worker");
        assert_eq!(active.display_name, "worker");
        assert_eq!(active.description, "worker description");
        assert_eq!(active.color.as_deref(), Some("blue"));
        assert_eq!(active.aliases, vec!["builder".to_string()]);
        assert_eq!(active.instructions, "worker instructions");
        assert_eq!(active.tools, Some(vec!["unified_search".to_string()]));
        assert_eq!(active.disallowed_tools, vec!["unified_file".to_string()]);
        assert_eq!(active.permissions.default, PermissionDefault::Deny);
        assert_eq!(active.model.as_deref(), Some("gpt-5.1"));
        assert_eq!(active.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(active.skills, vec!["rust".to_string()]);
        assert_eq!(
            active.mcp_servers,
            vec![SubagentMcpServer::Named("filesystem".to_string())]
        );
        assert_eq!(active.memory, Some(SubagentMemoryScope::Project));
    }

    #[test]
    fn primary_runtime_adapter_uses_shared_resolved_view_for_overlapping_fields() {
        let mut spec = test_spec("worker");
        spec.description = "Worker display metadata".to_string();
        spec.color = Some("green".to_string());
        spec.aliases = vec!["builder".to_string()];
        spec.skills = vec!["rust".to_string(), "repo".to_string()];
        spec.mcp_servers = vec![SubagentMcpServer::Named("filesystem".to_string())];
        spec.hooks = Some(HooksConfig::default());
        spec.memory = Some(SubagentMemoryScope::Project);

        let runtime = ResolvedAgentRuntimeView::from_spec(&spec);
        let active = ActivePrimaryAgent::from_runtime_view(&runtime);

        assert_eq!(runtime.canonical_name, "worker");
        assert_eq!(runtime.display_name, "worker");
        assert_eq!(runtime.description, "Worker display metadata");
        assert_eq!(runtime.color.as_deref(), Some("green"));
        assert_eq!(runtime.aliases, vec!["builder".to_string()]);
        assert_eq!(runtime.skills, vec!["rust".to_string(), "repo".to_string()]);
        assert_eq!(runtime.mcp_servers.len(), 1);
        assert!(runtime.hooks.is_some());
        assert_eq!(runtime.memory, Some(SubagentMemoryScope::Project));
        assert!(runtime.read_only);
        assert_eq!(active.identity.name, runtime.canonical_name);
        assert_eq!(active.display_name, runtime.display_name);
        assert_eq!(active.description, runtime.description);
        assert_eq!(active.color, runtime.color);
        assert_eq!(active.aliases, runtime.aliases);
        assert_eq!(active.instructions, runtime.instructions);
        assert_eq!(active.tools, runtime.tools);
        assert_eq!(active.disallowed_tools, runtime.disallowed_tools);
        assert_eq!(active.permissions, runtime.permissions);
        assert_eq!(active.model, runtime.model);
        assert_eq!(active.reasoning_effort, runtime.reasoning_effort);
        assert_eq!(active.hooks, runtime.hooks);
        assert_eq!(active.skills, runtime.skills);
        assert_eq!(active.mcp_servers, runtime.mcp_servers);
        assert_eq!(active.memory, runtime.memory);
    }

    #[test]
    fn default_state_uses_builtin_duck_agent() {
        let mut state = ActivePrimaryAgentState::default();

        assert_eq!(state.active().identity.name, DEFAULT_PRIMARY_AGENT_NAME);
        assert_eq!(state.active().identity.name, "duck");
        assert_eq!(state.active().identity.source, SubagentSource::Builtin);

        state
            .select_from_specs(&[test_spec("worker")], "worker")
            .expect("selected");
        assert_eq!(state.active().identity.name, "worker");

        state.reset_to_default_from_specs(&[]);

        assert_eq!(state.active().identity.name, DEFAULT_PRIMARY_AGENT_NAME);
        assert_eq!(state.active().identity.name, "duck");
        assert_eq!(state.active().identity.source, SubagentSource::Builtin);
    }

    #[test]
    fn from_specs_falls_back_to_builtin_duck_agent() {
        let active = ActivePrimaryAgentState::from_specs(&[]);

        assert_eq!(active.active().identity.name, "duck");
        assert_eq!(active.active().identity.source, SubagentSource::Builtin);
    }

    #[test]
    fn from_specs_with_default_selects_configured_primary_agent() {
        let active =
            ActivePrimaryAgentState::from_specs_with_default(&[test_spec("builder")], "builder");

        assert_eq!(active.active().identity.name, "builder");
    }

    #[test]
    fn from_specs_with_default_falls_back_to_duck_for_missing_configured_agent() {
        let active =
            ActivePrimaryAgentState::from_specs_with_default(&[test_spec("builder")], "missing");

        assert_eq!(active.active().identity.name, "duck");
        assert_eq!(active.active().identity.source, SubagentSource::Builtin);
    }

    #[test]
    fn discovery_precedence_overrides_builtin_duck_agent() {
        let temp = TempDir::new().expect("tempdir");
        let discovered = discover_subagents(&SubagentDiscoveryInput {
            workspace_root: temp.path().to_path_buf(),
            cli_agents: Some(json!({
                "duck": {
                    "description": "CLI duck",
                    "prompt": "cli duck instructions",
                    "model": "gpt-cli",
                    "mode": "primary",
                    "permissions": { "default": "deny" }
                }
            })),
            plugin_agent_files: Vec::new(),
            include_user_agents: false,
        })
        .expect("discovered subagents");

        let active = ActivePrimaryAgentState::from_discovery(&discovered);

        assert_eq!(active.active().identity.name, "duck");
        assert_eq!(active.active().identity.source, SubagentSource::Cli);
        assert_eq!(active.active().instructions, "cli duck instructions");
        assert_eq!(active.active().model.as_deref(), Some("gpt-cli"));
    }

    #[test]
    fn default_duck_agent_allows_baseline_read_tools() {
        let active = ActivePrimaryAgentState::default();

        assert!(primary_agent_allows_tool(active.active(), "unified_search"));
        assert!(!primary_agent_allows_tool(active.active(), "unified_file"));
    }

    #[test]
    fn tool_policy_intersects_allow_list_then_applies_deny_list() {
        let mut spec = test_spec("worker");
        spec.tools = Some(vec![
            "unified_search".to_string(),
            "unified_file".to_string(),
        ]);
        spec.disallowed_tools = vec!["UNIFIED_SEARCH".to_string()];
        let active = ActivePrimaryAgent::from_spec(&spec);

        assert!(!primary_agent_allows_tool(&active, "unified_exec"));
        assert!(!primary_agent_allows_tool(&active, "unified_search"));
        assert!(primary_agent_allows_tool(&active, "unified_file"));
    }

    #[test]
    fn empty_present_tool_allow_list_exposes_no_tools() {
        let mut spec = test_spec("worker");
        spec.tools = Some(Vec::new());
        spec.disallowed_tools = Vec::new();
        let active = ActivePrimaryAgent::from_spec(&spec);

        assert!(!primary_agent_allows_tool(&active, "unified_search"));
    }

    #[test]
    fn subagent_lifecycle_tools_bypass_tool_policy() {
        let mut spec = test_spec("restricted");
        spec.tools = Some(vec!["unified_search".to_string()]);
        spec.disallowed_tools = vec![
            "spawn_agent".to_string(),
            "wait_agent".to_string(),
            "close_agent".to_string(),
            "send_input".to_string(),
            "resume_agent".to_string(),
            "spawn_background_subprocess".to_string(),
        ];
        let active = ActivePrimaryAgent::from_spec(&spec);

        // Non-lifecycle tools respect the policy.
        assert!(!primary_agent_allows_tool(&active, "unified_exec"));
        assert!(!primary_agent_allows_tool(&active, "unified_file"));

        // Subagent lifecycle tools are always allowed even when listed in
        // disallowed_tools and absent from the allow list.
        assert!(primary_agent_allows_tool(&active, "spawn_agent"));
        assert!(primary_agent_allows_tool(&active, "wait_agent"));
        assert!(primary_agent_allows_tool(&active, "close_agent"));
        assert!(primary_agent_allows_tool(&active, "send_input"));
        assert!(primary_agent_allows_tool(&active, "resume_agent"));
        assert!(primary_agent_allows_tool(
            &active,
            "spawn_background_subprocess"
        ));
    }

    #[test]
    fn build_primary_agent_runtime_config_preserves_baseline_fields_and_merges_mcp() {
        let mut parent = VTCodeConfig::default();
        parent.agent.default_model = "parent-model".to_string();
        parent.mcp.providers.push(
            serde_json::from_value(json!({
                "name": "global",
                "command": "global-mcp",
                "args": []
            }))
            .expect("global provider"),
        );

        let mut spec = test_spec("worker");
        spec.permissions = AgentPermissionsConfig::new(PermissionDefault::Auto);
        spec.model = Some("agent-model".to_string());
        spec.reasoning_effort = Some("low".to_string());
        spec.mcp_servers = vec![SubagentMcpServer::Inline(BTreeMap::from([
            (
                "global".to_string(),
                json!({
                    "type": "stdio",
                    "command": "duplicate-mcp"
                }),
            ),
            (
                "local".to_string(),
                json!({
                    "type": "stdio",
                    "command": "local-mcp"
                }),
            ),
        ]))];
        let active = ActivePrimaryAgent::from_spec(&spec);

        let runtime = build_primary_agent_runtime_config(&parent, &active);

        assert_eq!(runtime.agent.default_model, "agent-model");
        assert_eq!(runtime.agent.reasoning_effort, ReasoningEffortLevel::Low);
        assert_eq!(runtime.mcp.providers.len(), 2);
        assert_eq!(runtime.mcp.providers[0].name, "global");
        assert_eq!(runtime.mcp.providers[1].name, "local");
    }

    #[test]
    fn built_in_primary_agents_resolve_required_permission_policy() {
        let builtins = builtin_subagents();

        for name in ["duck", "plan", "build", "auto"] {
            let active = resolve_primary_agent(&builtins, name)
                .unwrap_or_else(|_| panic!("missing built-in primary agent {name}"));
            assert_eq!(active.identity.name, name);
            let expected_default = match name {
                "build" => PermissionDefault::Ask,
                "auto" => PermissionDefault::Auto,
                "plan" | "duck" => PermissionDefault::Deny,
                _ => unreachable!("unexpected built-in primary agent"),
            };
            assert_eq!(active.permissions.default, expected_default);
        }
    }

    #[test]
    fn active_primary_permissions_overlay_runtime_decisions() {
        let builtins = builtin_subagents();
        let mut state = ActivePrimaryAgentState::from_specs(&builtins);
        let config = VTCodeConfig::default();
        let workspace = TempDir::new().expect("workspace");
        let current_dir = workspace.path();

        state
            .select_from_specs(&builtins, "auto")
            .expect("auto primary");
        let exec = build_permission_request(
            workspace.path(),
            current_dir,
            tools::UNIFIED_EXEC,
            Some(&json!({"command": "cargo test"})),
        );
        assert_eq!(
            evaluate_active_primary_agent_permissions(
                &config,
                state.active(),
                workspace.path(),
                current_dir,
                &exec,
            ),
            ResolvedPermissionDecision::Auto
        );

        state
            .select_from_specs(&builtins, "plan")
            .expect("plan primary");
        let edit = build_permission_request(
            workspace.path(),
            current_dir,
            tools::UNIFIED_FILE,
            Some(&json!({"action": "edit", "path": "src/lib.rs"})),
        );
        assert_eq!(
            evaluate_active_primary_agent_permissions(
                &config,
                state.active(),
                workspace.path(),
                current_dir,
                &edit,
            ),
            ResolvedPermissionDecision::Deny
        );
    }

    #[test]
    fn primary_agent_switching_changes_permission_policy_without_mutating_parent_config() {
        let builtins = builtin_subagents();
        let mut state = ActivePrimaryAgentState::from_specs(&builtins);
        let initial_model = state.active().model.clone();
        let initial_tools = state.active().tools.clone();

        state
            .select_from_specs(&builtins, "auto")
            .expect("auto primary");
        assert_eq!(state.active().identity.name, "auto");
        assert_eq!(state.active().permissions.default, PermissionDefault::Auto);
        assert_eq!(state.active().model, initial_model);
        assert_ne!(state.active().tools, initial_tools);

        state
            .select_from_specs(&builtins, "duck")
            .expect("duck primary");
        assert_eq!(state.active().identity.name, "duck");
        assert_eq!(state.active().permissions.default, PermissionDefault::Deny);
        assert_eq!(state.active().model, initial_model);
        assert_eq!(state.active().tools, initial_tools);
    }

    #[test]
    fn primary_hook_config_merges_supported_main_session_events_after_global_hooks() {
        let mut global = HooksConfig::default();
        global.lifecycle.user_prompt_submit = vec![hook_group("global-user")];
        global.lifecycle.pre_tool_use = vec![hook_group("global-pre")];
        global.lifecycle.post_tool_use = vec![hook_group("global-post")];
        global.lifecycle.permission_request = vec![hook_group("global-permission")];
        global.lifecycle.pre_compact = vec![hook_group("global-compact")];
        global.lifecycle.stop = vec![hook_group("global-stop")];
        global.lifecycle.notification = vec![hook_group("global-notification")];

        let mut primary_hooks = HooksConfig::default();
        primary_hooks.lifecycle.user_prompt_submit = vec![hook_group("primary-user")];
        primary_hooks.lifecycle.pre_tool_use = vec![hook_group("primary-pre")];
        primary_hooks.lifecycle.post_tool_use = vec![hook_group("primary-post")];
        primary_hooks.lifecycle.permission_request = vec![hook_group("primary-permission")];
        primary_hooks.lifecycle.pre_compact = vec![hook_group("primary-compact")];
        primary_hooks.lifecycle.stop = vec![hook_group("primary-stop")];
        primary_hooks.lifecycle.notification = vec![hook_group("primary-notification")];

        let mut spec = test_spec("worker");
        spec.hooks = Some(primary_hooks);
        let active = ActivePrimaryAgent::from_spec(&spec);

        let merged = build_primary_agent_hook_config(&global, &active);

        assert_hook_commands(
            &merged.lifecycle.user_prompt_submit,
            &["global-user", "primary-user"],
        );
        assert_hook_commands(
            &merged.lifecycle.pre_tool_use,
            &["global-pre", "primary-pre"],
        );
        assert_hook_commands(
            &merged.lifecycle.post_tool_use,
            &["global-post", "primary-post"],
        );
        assert_hook_commands(
            &merged.lifecycle.permission_request,
            &["global-permission", "primary-permission"],
        );
        assert_hook_commands(
            &merged.lifecycle.pre_compact,
            &["global-compact", "primary-compact"],
        );
        assert_hook_commands(&merged.lifecycle.stop, &["global-stop", "primary-stop"]);
        assert_hook_commands(
            &merged.lifecycle.notification,
            &["global-notification", "primary-notification"],
        );
    }

    #[test]
    fn primary_hook_config_excludes_global_and_subagent_lifecycle_events() {
        let global = HooksConfig::default();
        let mut primary_hooks = HooksConfig::default();
        primary_hooks.lifecycle.session_start = vec![hook_group("primary-session-start")];
        primary_hooks.lifecycle.session_end = vec![hook_group("primary-session-end")];
        primary_hooks.lifecycle.subagent_start = vec![hook_group("primary-subagent-start")];
        primary_hooks.lifecycle.subagent_stop = vec![hook_group("primary-subagent-stop")];
        primary_hooks.lifecycle.task_completion = vec![hook_group("primary-task-completion")];
        primary_hooks.lifecycle.task_completed = vec![hook_group("primary-task-completed")];

        let mut spec = test_spec("worker");
        spec.hooks = Some(primary_hooks);
        let active = ActivePrimaryAgent::from_spec(&spec);

        let merged = build_primary_agent_hook_config(&global, &active);

        assert!(merged.lifecycle.session_start.is_empty());
        assert!(merged.lifecycle.session_end.is_empty());
        assert!(merged.lifecycle.subagent_start.is_empty());
        assert!(merged.lifecycle.subagent_stop.is_empty());
        assert!(merged.lifecycle.task_completion.is_empty());
        assert!(merged.lifecycle.task_completed.is_empty());
        assert!(merged.lifecycle.stop.is_empty());
    }

    #[test]
    fn primary_hook_config_recomputes_without_previous_primary_leakage() {
        let global = HooksConfig::default();
        let mut first_hooks = HooksConfig::default();
        first_hooks.lifecycle.pre_tool_use = vec![hook_group("first-pre")];
        let mut second_hooks = HooksConfig::default();
        second_hooks.lifecycle.pre_tool_use = vec![hook_group("second-pre")];

        let mut first = test_spec("first");
        first.hooks = Some(first_hooks);
        let mut second = test_spec("second");
        second.hooks = Some(second_hooks);
        let specs = vec![first, second];
        let mut state = ActivePrimaryAgentState::default();

        state
            .select_from_specs(&specs, "first")
            .expect("selected first");
        let first_config = build_primary_agent_hook_config(&global, state.active());
        assert_hook_commands(&first_config.lifecycle.pre_tool_use, &["first-pre"]);

        state
            .select_from_specs(&specs, "second")
            .expect("selected second");
        let second_config = build_primary_agent_hook_config(&global, state.active());
        assert_hook_commands(&second_config.lifecycle.pre_tool_use, &["second-pre"]);
    }

    #[test]
    fn active_primary_state_recomputes_skills_mcp_and_metadata_on_switch() {
        let mut first = test_spec("first");
        first.description = "First metadata".to_string();
        first.color = Some("red".to_string());
        first.aliases = vec!["one".to_string()];
        first.skills = vec!["rust".to_string()];
        first.mcp_servers = vec![SubagentMcpServer::Inline(BTreeMap::from([(
            "first-mcp".to_string(),
            json!({
                "type": "stdio",
                "command": "first-mcp"
            }),
        )]))];
        let second = test_spec("second");
        let specs = vec![first, second];
        let mut state = ActivePrimaryAgentState::default();

        state
            .select_from_specs(&specs, "one")
            .expect("selected first by alias");
        assert_eq!(state.active().identity.name, "first");
        assert_eq!(state.active().description, "First metadata");
        assert_eq!(state.active().color.as_deref(), Some("red"));
        assert_eq!(state.active().aliases, vec!["one".to_string()]);
        assert_eq!(state.active().skills, vec!["rust".to_string()]);
        assert_eq!(state.active().mcp_servers.len(), 1);

        state
            .select_from_specs(&specs, "second")
            .expect("selected second");
        assert_eq!(state.active().identity.name, "second");
        assert_eq!(state.active().description, "second description");
        assert_eq!(state.active().color.as_deref(), Some("blue"));
        assert!(state.active().aliases.is_empty());
        assert!(state.active().skills.is_empty());
        assert!(state.active().mcp_servers.is_empty());
    }

    fn test_spec(name: &str) -> SubagentSpec {
        SubagentSpec {
            name: name.to_string(),
            description: format!("{name} description"),
            prompt: format!("{name} instructions"),
            tools: Some(vec!["unified_search".to_string()]),
            disallowed_tools: vec!["unified_file".to_string()],
            model: Some("gpt-5.1".to_string()),
            color: Some("blue".to_string()),
            reasoning_effort: Some("high".to_string()),
            permissions: AgentPermissionsConfig::new(PermissionDefault::Deny),
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
            mode: vtcode_config::AgentMode::Primary,
            max_turns: None,
            nickname_candidates: Vec::new(),
            initial_prompt: None,
            memory: None,
            isolation: None,
            aliases: Vec::new(),
            source: SubagentSource::ProjectVtcode,
            file_path: None,
            warnings: Vec::new(),
        }
    }

    fn hook_group(command: &str) -> HookGroupConfig {
        HookGroupConfig {
            matcher: None,
            hooks: vec![HookCommandConfig {
                command: command.to_string(),
                ..HookCommandConfig::default()
            }],
        }
    }

    fn assert_hook_commands(groups: &[HookGroupConfig], expected: &[&str]) {
        let commands = groups
            .iter()
            .flat_map(|group| group.hooks.iter())
            .map(|hook| hook.command.as_str())
            .collect::<Vec<_>>();
        assert_eq!(commands, expected);
    }
}
