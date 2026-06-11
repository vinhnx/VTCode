use crate::acp;
use std::collections::HashSet;
use vtcode_config::{SubagentSource, SubagentSpec, builtin_primary_duck_agent};
use vtcode_core::ActivePrimaryAgentState;
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::prompts::PromptTemplate;
use vtcode_core::skills::find_command_skill_by_slash_name;
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
    pub allows_local_tools: bool,
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
    pub(crate) fn allows_local_tools(&self, primary_agent: &str) -> bool {
        let Some(primary_agent) = self.resolve_id(primary_agent) else {
            return false;
        };
        self.options
            .iter()
            .any(|option| option.id == primary_agent && option.allows_local_tools)
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
    PrimaryAgentSessionOption {
        id: spec.name.clone(),
        label: primary_agent_label(spec),
        prompt: spec.prompt.clone(),
        aliases: spec.aliases.clone(),
        allows_local_tools: spec.permissions_allows_mutation(),
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
        ReasoningEffortLevel::None => "None",
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
