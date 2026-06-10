use crate::acp;
use std::collections::HashSet;
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::prompts::PromptTemplate;
use vtcode_core::skills::find_command_skill_by_slash_name;
use vtcode_core::ui::slash::SlashCommandInfo;

pub(crate) const SESSION_CONFIG_PRIMARY_AGENT_ID: &str = "primary_agent";
pub(crate) const SESSION_CONFIG_THOUGHT_LEVEL_ID: &str = "thought_level";
pub(crate) const SESSION_CONFIG_PROVIDER_ID: &str = "provider";
pub(crate) const SESSION_CONFIG_MODEL_ID: &str = "model";

const PRIMARY_AGENT_OPTIONS: [(&str, &str); 5] = [
    ("duck", "Duck"),
    ("plan", "Plan"),
    ("build", "Build"),
    ("auto", "Auto"),
    ("review", "Review"),
];

pub(crate) fn normalise_primary_agent_id(primary_agent: &str) -> Option<&'static str> {
    let primary_agent = primary_agent.trim();
    PRIMARY_AGENT_OPTIONS
        .iter()
        .find_map(|(id, _)| primary_agent.eq_ignore_ascii_case(id).then_some(*id))
}

pub(crate) fn normalise_primary_agent_id_or_default(primary_agent: &str) -> &'static str {
    normalise_primary_agent_id(primary_agent).unwrap_or("duck")
}

pub(crate) fn primary_agent_allows_local_tools(primary_agent: &str) -> bool {
    matches!(
        normalise_primary_agent_id(primary_agent),
        Some("build" | "auto")
    )
}

pub(crate) fn primary_agent_prompt(primary_agent: &str) -> Option<&'static str> {
    match normalise_primary_agent_id(primary_agent)? {
        "duck" => Some(
            "Use the duck primary agent behaviour: discuss first, clarify scope and trade-offs, and ask before implementation.",
        ),
        "plan" => Some(
            "Use the plan primary agent behaviour: focus on planning workflow work, repository discovery, and read-only analysis unless durable plan files are explicitly in scope.",
        ),
        "build" => Some(
            "Use the build primary agent behaviour: implement the requested change with focused edits and relevant validation.",
        ),
        "auto" => Some(
            "Use the auto primary agent behaviour: continue implementation autonomously within the configured tool and permission policy.",
        ),
        "review" => Some(
            "Use the review primary agent behaviour: inspect changes, prioritise findings, and avoid edits unless explicitly requested.",
        ),
        _ => None,
    }
}

fn primary_agent_options() -> Vec<acp::SessionConfigSelectOption> {
    PRIMARY_AGENT_OPTIONS
        .iter()
        .map(|(id, name)| acp::SessionConfigSelectOption::new(*id, *name))
        .collect::<Vec<_>>()
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
    let current_primary_agent = normalise_primary_agent_id(current_primary_agent).unwrap_or("duck");

    let mut config_options = Vec::with_capacity(4);
    config_options.push(
        acp::SessionConfigOption::select(
            SESSION_CONFIG_PRIMARY_AGENT_ID,
            "Primary agent",
            current_primary_agent.to_string(),
            primary_agent_options(),
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
