use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use vtcode_config::constants::defaults::DEFAULT_PRIMARY_AGENT_NAME;
use vtcode_config::{
    DiscoveredSubagents, PermissionMode, SubagentSource, SubagentSpec, builtin_primary_build_agent,
};

use crate::llm::provider::ToolDefinition;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivePrimaryAgentSpecIdentity {
    pub name: String,
    pub source: SubagentSource,
    pub file_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivePrimaryAgent {
    pub identity: ActivePrimaryAgentSpecIdentity,
    pub display_name: String,
    pub instructions: String,
    pub tools: Option<Vec<String>>,
    pub disallowed_tools: Vec<String>,
    pub permission_mode: Option<PermissionMode>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
}

impl ActivePrimaryAgent {
    #[must_use]
    pub fn from_spec(spec: &SubagentSpec) -> Self {
        Self {
            identity: ActivePrimaryAgentSpecIdentity {
                name: spec.name.clone(),
                source: spec.source.clone(),
                file_path: spec.file_path.clone(),
            },
            display_name: spec.name.clone(),
            instructions: spec.prompt.clone(),
            tools: spec.tools.clone(),
            disallowed_tools: spec.disallowed_tools.clone(),
            permission_mode: spec.permission_mode,
            model: spec.model.clone(),
            reasoning_effort: spec.reasoning_effort.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
        let active = resolve_primary_agent(specs, DEFAULT_PRIMARY_AGENT_NAME)
            .unwrap_or_else(|_| ActivePrimaryAgent::from_spec(&builtin_primary_build_agent()));
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

#[must_use]
pub fn clamp_primary_permission_mode(
    base: PermissionMode,
    requested: Option<PermissionMode>,
) -> PermissionMode {
    let Some(requested) = requested else {
        return base;
    };

    if permission_rank(requested) <= permission_rank(base) {
        requested
    } else {
        base
    }
}

#[must_use]
pub fn primary_agent_allows_tool(agent: &ActivePrimaryAgent, tool_name: &str) -> bool {
    let tool_name = normalise_tool_name(tool_name);
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

fn permission_rank(mode: PermissionMode) -> u8 {
    match mode {
        PermissionMode::DontAsk => 0,
        PermissionMode::Plan => 1,
        PermissionMode::Default => 2,
        PermissionMode::AcceptEdits => 3,
        PermissionMode::Auto => 4,
        PermissionMode::BypassPermissions => 5,
    }
}

fn normalise_tool_name(tool_name: &str) -> String {
    tool_name.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::TempDir;
    use vtcode_config::{
        SubagentDiscoveryInput, SubagentMcpServer, SubagentMemoryScope, SubagentSource,
        discover_subagents,
    };

    use super::*;

    #[test]
    fn resolves_existing_spec_by_name() {
        let spec = test_spec("planner");
        let active = resolve_primary_agent(&[spec], "planner").expect("resolved");

        assert_eq!(active.identity.name, "planner");
        assert_eq!(active.display_name, "planner");
        assert_eq!(active.instructions, "planner instructions");
        assert_eq!(active.tools, Some(vec!["unified_search".to_string()]));
        assert_eq!(active.disallowed_tools, vec!["unified_file".to_string()]);
        assert_eq!(active.permission_mode, Some(PermissionMode::Plan));
        assert_eq!(active.model.as_deref(), Some("gpt-5.1"));
        assert_eq!(active.reasoning_effort.as_deref(), Some("high"));
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
        assert_eq!(active.instructions, "worker instructions");
        assert_eq!(active.tools, Some(vec!["unified_search".to_string()]));
        assert_eq!(active.disallowed_tools, vec!["unified_file".to_string()]);
        assert_eq!(active.permission_mode, Some(PermissionMode::Plan));
        assert_eq!(active.model.as_deref(), Some("gpt-5.1"));
        assert_eq!(active.reasoning_effort.as_deref(), Some("high"));
    }

    #[test]
    fn default_state_uses_builtin_build_agent() {
        let mut state = ActivePrimaryAgentState::default();

        assert_eq!(state.active().identity.name, DEFAULT_PRIMARY_AGENT_NAME);
        assert_eq!(state.active().identity.source, SubagentSource::Builtin);

        state
            .select_from_specs(&[test_spec("worker")], "worker")
            .expect("selected");
        assert_eq!(state.active().identity.name, "worker");

        state.reset_to_default_from_specs(&[]);

        assert_eq!(state.active().identity.name, DEFAULT_PRIMARY_AGENT_NAME);
        assert_eq!(state.active().identity.source, SubagentSource::Builtin);
    }

    #[test]
    fn discovery_precedence_overrides_builtin_build_agent() {
        let temp = TempDir::new().expect("tempdir");
        let discovered = discover_subagents(&SubagentDiscoveryInput {
            workspace_root: temp.path().to_path_buf(),
            cli_agents: Some(json!({
                "build": {
                    "description": "CLI build",
                    "prompt": "cli build instructions",
                    "model": "gpt-cli",
                    "mode": "primary"
                }
            })),
            plugin_agent_files: Vec::new(),
        })
        .expect("discovered subagents");

        let active = ActivePrimaryAgentState::from_discovery(&discovered);

        assert_eq!(active.active().identity.name, "build");
        assert_eq!(active.active().identity.source, SubagentSource::Cli);
        assert_eq!(active.active().instructions, "cli build instructions");
        assert_eq!(active.active().model.as_deref(), Some("gpt-cli"));
    }

    #[test]
    fn default_build_agent_allows_baseline_tools() {
        let active = ActivePrimaryAgentState::default();

        assert!(primary_agent_allows_tool(active.active(), "unified_search"));
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
    fn permission_policy_clamps_without_broadening() {
        assert_eq!(
            clamp_primary_permission_mode(PermissionMode::Default, Some(PermissionMode::Plan)),
            PermissionMode::Plan
        );
        assert_eq!(
            clamp_primary_permission_mode(PermissionMode::Default, Some(PermissionMode::Auto)),
            PermissionMode::Default
        );
        assert_eq!(
            clamp_primary_permission_mode(PermissionMode::Auto, Some(PermissionMode::Plan)),
            PermissionMode::Plan
        );
        assert_eq!(
            clamp_primary_permission_mode(PermissionMode::Plan, None),
            PermissionMode::Plan
        );
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
            permission_mode: Some(PermissionMode::Plan),
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
}
