use std::error::Error;
use std::fmt;
use std::path::PathBuf;

use vtcode_config::{DiscoveredSubagents, PermissionMode, SubagentSource, SubagentSpec};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveSessionAgentSpecIdentity {
    pub name: String,
    pub source: SubagentSource,
    pub file_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveSessionAgent {
    pub identity: ActiveSessionAgentSpecIdentity,
    pub display_name: String,
    pub instructions: String,
    pub tools: Option<Vec<String>>,
    pub disallowed_tools: Vec<String>,
    pub permission_mode: Option<PermissionMode>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
}

impl ActiveSessionAgent {
    #[must_use]
    pub fn from_spec(spec: &SubagentSpec) -> Self {
        Self {
            identity: ActiveSessionAgentSpecIdentity {
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActiveSessionAgentState {
    active: Option<ActiveSessionAgent>,
}

impl ActiveSessionAgentState {
    #[must_use]
    pub const fn active(&self) -> Option<&ActiveSessionAgent> {
        self.active.as_ref()
    }

    pub fn clear(&mut self) {
        self.active = None;
    }

    pub fn select_from_discovery(
        &mut self,
        discovered: &DiscoveredSubagents,
        requested: &str,
    ) -> SessionAgentResolutionResult<&ActiveSessionAgent> {
        self.select_from_specs(&discovered.effective, requested)
    }

    pub fn select_from_specs(
        &mut self,
        specs: &[SubagentSpec],
        requested: &str,
    ) -> SessionAgentResolutionResult<&ActiveSessionAgent> {
        let active = resolve_session_agent(specs, requested)?;
        Ok(self.active.insert(active))
    }
}

pub type SessionAgentResolutionResult<T> = Result<T, SessionAgentResolutionError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionAgentResolutionError {
    UnknownAgent { requested: String },
}

impl fmt::Display for SessionAgentResolutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownAgent { requested } => write!(f, "Unknown session agent {requested}"),
        }
    }
}

impl Error for SessionAgentResolutionError {}

pub fn resolve_discovered_session_agent(
    discovered: &DiscoveredSubagents,
    requested: &str,
) -> SessionAgentResolutionResult<ActiveSessionAgent> {
    resolve_session_agent(&discovered.effective, requested)
}

pub fn resolve_session_agent(
    specs: &[SubagentSpec],
    requested: &str,
) -> SessionAgentResolutionResult<ActiveSessionAgent> {
    specs
        .iter()
        .find(|spec| spec.matches_name(requested))
        .map(ActiveSessionAgent::from_spec)
        .ok_or_else(|| SessionAgentResolutionError::UnknownAgent {
            requested: requested.to_string(),
        })
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
        let active = resolve_session_agent(&[spec], "planner").expect("resolved");

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
        let mut state = ActiveSessionAgentState::default();
        let original = state
            .select_from_specs(&specs, "current")
            .expect("initial selection")
            .clone();

        let error = state
            .select_from_specs(&specs, "missing")
            .expect_err("unknown agent");

        assert_eq!(
            error,
            SessionAgentResolutionError::UnknownAgent {
                requested: "missing".to_string()
            }
        );
        assert_eq!(state.active(), Some(&original));
    }

    #[test]
    fn alias_resolution_uses_existing_matches_name_semantics() {
        let mut spec = test_spec("reviewer");
        spec.aliases = vec!["critic".to_string()];

        let active = resolve_session_agent(&[spec], "CRITIC").expect("resolved by alias");

        assert_eq!(active.identity.name, "reviewer");
        assert_eq!(active.display_name, "reviewer");
    }

    #[test]
    fn ignored_subagent_fields_do_not_enter_runtime_overlay() {
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

        let active = ActiveSessionAgent::from_spec(&spec);

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
    fn default_state_has_no_overlay_and_clear_restores_no_overlay() {
        let mut state = ActiveSessionAgentState::default();

        assert!(state.active().is_none());

        state
            .select_from_specs(&[test_spec("worker")], "worker")
            .expect("selected");
        state.clear();

        assert!(state.active().is_none());
    }

    #[test]
    fn discovery_precedence_is_used_for_runtime_resolution() {
        let temp = TempDir::new().expect("tempdir");
        let discovered = discover_subagents(&SubagentDiscoveryInput {
            workspace_root: temp.path().to_path_buf(),
            cli_agents: Some(json!({
                "default": {
                    "description": "CLI default",
                    "prompt": "cli default instructions",
                    "model": "gpt-cli"
                }
            })),
            plugin_agent_files: Vec::new(),
        })
        .expect("discovered subagents");

        let active = resolve_discovered_session_agent(&discovered, "default").expect("resolved");

        assert_eq!(active.identity.name, "default");
        assert_eq!(active.identity.source, SubagentSource::Cli);
        assert_eq!(active.instructions, "cli default instructions");
        assert_eq!(active.model.as_deref(), Some("gpt-cli"));
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
