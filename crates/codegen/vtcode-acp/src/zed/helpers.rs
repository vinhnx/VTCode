use crate::acp;
use std::collections::HashSet;
use std::path::Path;
use vtcode_config::core::permissions::AgentPermissionsConfig;
use vtcode_config::{SubagentSource, SubagentSpec, builtin_primary_duck_agent};
use vtcode_core::ActivePrimaryAgentState;
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::permissions::{
    ResolvedPermissionDecision, build_advertised_permission_requests, evaluate_agent_permissions,
};
use vtcode_core::prompts::PromptTemplate;
use vtcode_core::skills::find_command_skill_by_slash_name;
use vtcode_core::tools::names::canonical_tool_name;
use vtcode_core::ui::slash::SlashCommandInfo;

pub(crate) const SESSION_CONFIG_PRIMARY_AGENT_ID: &str = "primary_agent";
pub(crate) const SESSION_CONFIG_THOUGHT_LEVEL_ID: &str = "thought_level";
pub(crate) const SESSION_CONFIG_PROVIDER_ID: &str = "provider";
pub(crate) const SESSION_CONFIG_MODEL_ID: &str = "model";

const BUILTIN_PRIMARY_AGENT_ORDER: [&str; 4] = ["duck", "plan", "build", "auto"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrimaryAgentSessionOption {
    pub id: String,
    pub label: String,
    pub prompt: String,
    pub aliases: Vec<String>,
    pub allowed_local_tools: Option<HashSet<String>>,
    pub denied_local_tools: HashSet<String>,
    pub permissions: AgentPermissionsConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrimaryAgentCatalog {
    options: Vec<PrimaryAgentSessionOption>,
    default_id: String,
}

impl PrimaryAgentCatalog {
    #[must_use]
    pub(crate) fn from_specs_with_default(
        specs: &[SubagentSpec],
        default_primary_agent: &str,
    ) -> Self {
        let mut options = specs
            .iter()
            .filter(|spec| spec.is_primary())
            .map(primary_agent_option_from_spec)
            .collect::<Vec<_>>();
        options.sort_by(|left, right| {
            primary_agent_order_key(&left.id, &left.label)
                .cmp(&primary_agent_order_key(&right.id, &right.label))
        });

        if options.is_empty() {
            options.push(primary_agent_option_from_spec(&builtin_primary_duck_agent()));
        }

        let active = ActivePrimaryAgentState::from_specs_with_default(specs, default_primary_agent);
        let default_id = if options.iter().any(|option| {
            option
                .id
                .eq_ignore_ascii_case(&active.active().identity.name)
        }) {
            active.active().identity.name.clone()
        } else {
            options
                .first()
                .map(|option| option.id.clone())
                .unwrap_or_else(|| "duck".to_string())
        };

        Self {
            options,
            default_id,
        }
    }

    #[must_use]
    pub(crate) fn default_id(&self) -> &str {
        &self.default_id
    }

    #[must_use]
    pub(crate) fn resolve_id(&self, primary_agent: &str) -> Option<&str> {
        let primary_agent = primary_agent.trim();
        self.options
            .iter()
            .find_map(|option| {
                option
                    .id
                    .eq_ignore_ascii_case(primary_agent)
                    .then_some(option.id.as_str())
            })
            .or_else(|| {
                self.options.iter().find_map(|option| {
                    option
                        .aliases
                        .iter()
                        .any(|alias| alias.eq_ignore_ascii_case(primary_agent))
                        .then_some(option.id.as_str())
                })
            })
    }

    #[must_use]
    pub(crate) fn prompt(&self, primary_agent: &str) -> Option<&str> {
        let primary_agent = self.resolve_id(primary_agent)?;
        self.options
            .iter()
            .find_map(|option| (option.id == primary_agent).then_some(option.prompt.as_str()))
    }

    #[must_use]
    pub(crate) fn allows_local_tool(&self, primary_agent: &str, tool_name: &str) -> bool {
        let Some(primary_agent) = self.resolve_id(primary_agent) else {
            return false;
        };
        self.options
            .iter()
            .find(|option| option.id == primary_agent)
            .is_some_and(|option| {
                let tool_name = tool_name.trim().to_ascii_lowercase();
                let semantic_name = local_tool_semantic_name(&tool_name);
                if option.denied_local_tools.contains(&tool_name)
                    || semantic_name
                        .is_some_and(|semantic| option.denied_local_tools.contains(semantic))
                {
                    return false;
                }
                option.allowed_local_tools.as_ref().is_none_or(|allowed| {
                    allowed.contains(&tool_name)
                        || semantic_name.is_some_and(|semantic| allowed.contains(semantic))
                })
            })
    }

    #[must_use]
    fn option_for(&self, primary_agent: &str) -> Option<&PrimaryAgentSessionOption> {
        let resolved = self.resolve_id(primary_agent)?;
        self.options.iter().find(|option| option.id == resolved)
    }

    /// Returns `true` if the resolved primary agent's permission policy permits
    /// (allow/ask/auto) the given local tool. Unknown agents deny by default.
    ///
    /// This gates individual local tools by the agent's declared permissions so
    /// a `default: deny` agent that only allows `exec_command` does not silently
    /// expose every other local tool (e.g. `apply_patch`).
    ///
    /// This is an **advertising** gate only: it controls which local tool
    /// definitions are offered to the model, not whether a call is allowed to
    /// execute. The authoritative enforcement boundary remains the runtime
    /// permission/safety check (`evaluate_effective_permissions` +
    /// `check_safety_for_request`) evaluated against the concrete call arguments
    /// at dispatch time. A tool that slips past this gate is still subject to
    /// that runtime check.
    #[must_use]
    pub(crate) fn allows_tool(
        &self,
        primary_agent: &str,
        tool_name: &str,
        workspace_root: &Path,
    ) -> bool {
        let Some(option) = self.option_for(primary_agent) else {
            return false;
        };
        // Normalize to the canonical tool name so the lookup in
        // `advertised_permission_args` matches a known `tools::*` constant.
        // Otherwise an unrecognized name falls through to a `PermissionRequestKind::Other`
        // representative request, which would over-permit for any non-deny default.
        let canonical = canonical_tool_name(tool_name).to_ascii_lowercase();
        // Advertising cannot know future call arguments, so evaluate each
        // representative request the tool can produce and permit the tool if any
        // categorized operation is not denied (mirrors provider-native
        // availability gating in the main runtime). `workspace_root` is passed as
        // both the workspace root and the current dir because advertising has no
        // per-call working directory.
        build_advertised_permission_requests(workspace_root, workspace_root, &canonical)
            .iter()
            .any(|request| {
                !matches!(
                    evaluate_agent_permissions(
                        &option.permissions,
                        workspace_root,
                        workspace_root,
                        request,
                    ),
                    ResolvedPermissionDecision::Deny
                )
            })
    }

    #[must_use]
    fn select_options(&self) -> Vec<acp::SessionConfigSelectOption> {
        self.options
            .iter()
            .map(|option| {
                acp::SessionConfigSelectOption::new(option.id.clone(), option.label.clone())
            })
            .collect::<Vec<_>>()
    }
}

fn primary_agent_option_from_spec(spec: &SubagentSpec) -> PrimaryAgentSessionOption {
    let (allowed_local_tools, denied_local_tools) = local_tool_rules(spec);
    PrimaryAgentSessionOption {
        id: spec.name.clone(),
        label: primary_agent_label(spec),
        prompt: spec.prompt.clone(),
        aliases: spec.aliases.clone(),
        allowed_local_tools,
        denied_local_tools,
        permissions: spec.permissions.clone(),
    }
}

fn local_tool_rules(spec: &SubagentSpec) -> (Option<HashSet<String>>, HashSet<String>) {
    use vtcode_config::core::permissions::PermissionDefault;

    let mut allowed = if spec.permissions.default == PermissionDefault::Deny {
        Some(
            spec.permissions
                .allow
                .iter()
                .chain(&spec.permissions.ask)
                .chain(&spec.permissions.auto)
                .map(|rule| local_tool_rule_name(rule))
                .collect::<HashSet<_>>(),
        )
    } else {
        None
    };
    if let Some(tools) = &spec.tools {
        let tools = tools
            .iter()
            .map(|tool| tool.trim().to_ascii_lowercase())
            .collect::<HashSet<_>>();
        allowed = Some(match allowed {
            Some(current) => tools
                .into_iter()
                .filter(|tool| local_tool_rules_match(&current, tool))
                .collect(),
            None => tools,
        });
    }
    let denied = spec
        .permissions
        .deny
        .iter()
        .chain(&spec.disallowed_tools)
        .map(|rule| local_tool_rule_name(rule))
        .collect();
    (allowed, denied)
}

fn local_tool_rule_name(rule: &str) -> String {
    let normalized = vtcode_config::core::permissions::normalize_permission_rule(rule);
    let normalized = normalized.trim().to_ascii_lowercase();
    let tool_name = normalized
        .strip_suffix(')')
        .and_then(|rule| rule.split_once('(').map(|(tool_name, _)| tool_name))
        .filter(|tool_name| matches!(*tool_name, "bash" | "read" | "edit" | "write" | "webfetch"))
        .unwrap_or(&normalized);
    local_tool_semantic_name(tool_name)
        .unwrap_or(tool_name)
        .to_string()
}

fn local_tool_rules_match(rules: &HashSet<String>, tool_name: &str) -> bool {
    rules.contains(tool_name)
        || local_tool_semantic_name(tool_name).is_some_and(|semantic| rules.contains(semantic))
        || rules.contains(&local_tool_rule_name(tool_name))
}

fn local_tool_semantic_name(tool_name: &str) -> Option<&'static str> {
    match tool_name {
        "exec_command" | "write_stdin" => Some("bash"),
        "apply_patch" => Some("edit"),
        "code_search" => Some("read"),
        _ => None,
    }
}

fn primary_agent_label(spec: &SubagentSpec) -> String {
    if spec.source == SubagentSource::Builtin || spec.description.trim().is_empty() {
        title_case_identifier(&spec.name)
    } else {
        spec.description.clone()
    }
}

fn primary_agent_order_key(id: &str, label: &str) -> (usize, String) {
    let built_in_position = BUILTIN_PRIMARY_AGENT_ORDER
        .iter()
        .position(|candidate| candidate.eq_ignore_ascii_case(id))
        .unwrap_or(BUILTIN_PRIMARY_AGENT_ORDER.len());
    (built_in_position, label.to_ascii_lowercase())
}

fn title_case_identifier(id: &str) -> String {
    id.split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn reasoning_effort_name(level: ReasoningEffortLevel) -> &'static str {
    match level {
        ReasoningEffortLevel::None | ReasoningEffortLevel::Unknown => "None",
        ReasoningEffortLevel::Minimal => "Minimal",
        ReasoningEffortLevel::Low => "Low",
        ReasoningEffortLevel::Medium => "Medium",
        ReasoningEffortLevel::High => "High",
        ReasoningEffortLevel::XHigh => "Extra High",
        ReasoningEffortLevel::Max => "Max",
    }
}

pub(crate) fn session_config_options(
    current_primary_agent: &str,
    primary_agents: &PrimaryAgentCatalog,
    reasoning_effort: ReasoningEffortLevel,
    include_thought_level: bool,
    current_provider: &str,
    provider_options: Vec<acp::SessionConfigSelectOption>,
    current_model: &str,
    model_options: Vec<acp::SessionConfigSelectOption>,
) -> Vec<acp::SessionConfigOption> {
    let thought_level_options = ReasoningEffortLevel::allowed_values()
        .iter()
        .filter_map(|value| {
            ReasoningEffortLevel::parse(value).map(|level| {
                acp::SessionConfigSelectOption::new(level.as_str(), reasoning_effort_name(level))
            })
        })
        .collect::<Vec<_>>();
    let current_primary_agent = primary_agents
        .resolve_id(current_primary_agent)
        .unwrap_or_else(|| primary_agents.default_id());

    let mut config_options = Vec::with_capacity(4);
    config_options.push(
        acp::SessionConfigOption::select(
            SESSION_CONFIG_PRIMARY_AGENT_ID,
            "Primary agent",
            current_primary_agent.to_string(),
            primary_agents.select_options(),
        )
        .description("Controls which VT Code primary agent handles this ACP session."),
    );
    config_options.push(
        acp::SessionConfigOption::select(
            SESSION_CONFIG_PROVIDER_ID,
            "Provider",
            current_provider.to_string(),
            provider_options,
        )
        .description("Controls which LLM provider VT Code uses for this ACP session."),
    );
    config_options.push(
        acp::SessionConfigOption::select(
            SESSION_CONFIG_MODEL_ID,
            "Model",
            current_model.to_string(),
            model_options,
        )
        .description("Controls which model VT Code uses for this ACP session.")
        .category(acp::SessionConfigOptionCategory::Model),
    );
    if include_thought_level {
        config_options.push(
            acp::SessionConfigOption::select(
                SESSION_CONFIG_THOUGHT_LEVEL_ID,
                "Effort level",
                reasoning_effort.as_str(),
                thought_level_options,
            )
            .description("Controls how much effort VT Code requests from the model.")
            .category(acp::SessionConfigOptionCategory::ThoughtLevel),
        );
    }

    config_options
}

pub(crate) fn text_chunk(text: impl Into<String>) -> acp::ContentChunk {
    acp::ContentChunk::new(acp::ContentBlock::from(text.into()))
}

pub(crate) fn agent_implementation_info(title_override: Option<String>) -> acp::Implementation {
    acp::Implementation::new("vtcode", env!("CARGO_PKG_VERSION"))
        .title(title_override.or_else(|| Some("VT Code".to_string())))
}

fn command_input_hint(name: &str) -> Option<String> {
    let usage = find_command_skill_by_slash_name(name)?.usage.trim();
    let bare_usage = format!("/{name}");
    if usage == bare_usage {
        None
    } else {
        Some(format!("Usage: {usage}"))
    }
}

fn build_available_command(name: &str, description: &str) -> acp::AvailableCommand {
    let mut command = acp::AvailableCommand::new(name.to_string(), description.to_string());
    if let Some(hint) = command_input_hint(name) {
        command = command.input(acp::AvailableCommandInput::Unstructured(
            acp::UnstructuredCommandInput::new(hint),
        ));
    }
    command
}

pub(crate) fn build_available_commands(
    slash_commands: &[&SlashCommandInfo],
    prompt_templates: &[PromptTemplate],
) -> Vec<acp::AvailableCommand> {
    let mut available_commands = slash_commands
        .iter()
        .map(|command| build_available_command(command.name, command.description))
        .collect::<Vec<_>>();

    let mut seen_names = slash_commands
        .iter()
        .map(|command| command.name.to_string())
        .collect::<HashSet<_>>();
    for template in prompt_templates {
        if !seen_names.insert(template.name.clone()) {
            continue;
        }
        available_commands.push(
            acp::AvailableCommand::new(template.name.clone(), template.description.clone()).input(
                acp::AvailableCommandInput::Unstructured(acp::UnstructuredCommandInput::new(
                    "Optional template arguments",
                )),
            ),
        );
    }

    available_commands
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use vtcode_config::core::permissions::PermissionDefault;
    use vtcode_config::{SubagentSource, builtin_primary_build_agent};

    #[test]
    fn primary_agent_resolution_prefers_exact_ids_before_aliases() {
        let mut custom_builder = builtin_primary_build_agent();
        custom_builder.name = "builder".to_string();
        custom_builder.description = "Custom builder".to_string();
        custom_builder.prompt = "Custom builder prompt.".to_string();
        custom_builder.aliases = vec!["project-builder".to_string()];
        custom_builder.source = SubagentSource::ProjectVtcode;

        let specs = [builtin_primary_build_agent(), custom_builder];
        let catalog = PrimaryAgentCatalog::from_specs_with_default(&specs, "duck");

        assert_eq!(catalog.resolve_id("build"), Some("build"));
        assert_eq!(catalog.resolve_id("builder"), Some("builder"));
        assert_eq!(catalog.resolve_id("project-builder"), Some("builder"));
    }

    #[test]
    fn local_tool_rules_intersect_semantic_and_path_qualified_permissions() {
        for permission_rule in ["read", "code_search(/src/**)"] {
            let mut spec = builtin_primary_build_agent();
            spec.permissions.default = PermissionDefault::Deny;
            spec.permissions.allow = vec![permission_rule.to_string()];
            spec.tools = Some(vec!["code_search".to_string()]);

            let catalog = PrimaryAgentCatalog::from_specs_with_default(&[spec], "build");

            assert!(
                catalog.allows_local_tool("build", "code_search"),
                "permission rule {permission_rule} should advertise code_search"
            );
            assert!(!catalog.allows_local_tool("build", "exec_command"));
        }
    }

    #[test]
    fn build_available_commands_includes_templates_and_deduplicates_names() {
        let slash_command = SlashCommandInfo {
            name: "status",
            description: "Show status",
        };
        let templates = vec![
            PromptTemplate {
                name: "custom-plan".to_string(),
                description: "Generate a custom plan".to_string(),
                body: "Plan $@".to_string(),
                path: PathBuf::from("/tmp/custom-plan.md"),
            },
            PromptTemplate {
                name: "status".to_string(),
                description: "Duplicate built-in name".to_string(),
                body: "ignored".to_string(),
                path: PathBuf::from("/tmp/status.md"),
            },
        ];

        let commands = build_available_commands(&[&slash_command], &templates);
        let names = commands
            .iter()
            .map(|command| command.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["status", "custom-plan"]);
    }
}
