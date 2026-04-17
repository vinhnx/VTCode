use agent_client_protocol as acp;
use std::collections::HashSet;
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::core::interfaces::SessionMode;
use vtcode_core::prompts::PromptTemplate;
use vtcode_core::skills::find_command_skill_by_slash_name;
use vtcode_core::ui::slash::SlashCommandInfo;

pub(crate) const SESSION_CONFIG_MODE_ID: &str = "mode";
pub(crate) const SESSION_CONFIG_THOUGHT_LEVEL_ID: &str = "thought_level";
pub(crate) const SESSION_CONFIG_PROVIDER_ID: &str = "provider";
pub(crate) const SESSION_CONFIG_MODEL_ID: &str = "model";

pub(crate) fn session_mode_description(mode: SessionMode) -> &'static str {
    match mode {
        SessionMode::Ask => "Answer questions with read-only workspace inspection",
        SessionMode::Architect => {
            "Design and plan software systems with read-only workspace inspection"
        }
        SessionMode::Code => "Write and modify code with full tool access",
    }
}

pub(crate) fn session_mode_prompt(mode: SessionMode) -> Option<&'static str> {
    match mode {
        SessionMode::Ask => Some(
            "You are in Ask mode. Answer questions directly and use only read-only workspace inspection tools when needed. Do not make code changes or run implementation tools. If the user wants implementation, tell them to switch to Code mode.",
        ),
        SessionMode::Architect => Some(
            "You are in Architect mode. Focus on design, planning, and read-only workspace inspection. Do not make code changes or run implementation tools. If implementation is requested, provide a plan or ask the user to switch to Code mode.",
        ),
        SessionMode::Code => None,
    }
}

pub(crate) fn session_mode_allows_local_tools(mode: SessionMode) -> bool {
    matches!(mode, SessionMode::Code)
}

pub(crate) fn acp_session_modes() -> Vec<acp::SessionMode> {
    vec![
        acp::SessionMode::new(SessionMode::Ask.as_str(), "Ask")
            .description(session_mode_description(SessionMode::Ask)),
        acp::SessionMode::new(SessionMode::Architect.as_str(), "Architect")
            .description(session_mode_description(SessionMode::Architect)),
        acp::SessionMode::new(SessionMode::Code.as_str(), "Code")
            .description(session_mode_description(SessionMode::Code)),
    ]
}

pub(crate) fn session_mode_id(mode: SessionMode) -> acp::SessionModeId {
    acp::SessionModeId::new(mode.as_str())
}

fn session_mode_name(mode: SessionMode) -> &'static str {
    match mode {
        SessionMode::Ask => "Ask",
        SessionMode::Architect => "Architect",
        SessionMode::Code => "Code",
    }
}

fn reasoning_effort_name(level: ReasoningEffortLevel) -> &'static str {
    match level {
        ReasoningEffortLevel::None => "None",
        ReasoningEffortLevel::Minimal => "Minimal",
        ReasoningEffortLevel::Low => "Low",
        ReasoningEffortLevel::Medium => "Medium",
        ReasoningEffortLevel::High => "High",
        ReasoningEffortLevel::XHigh => "Extra High",
    }
}

pub(crate) fn session_config_options(
    current_mode: SessionMode,
    reasoning_effort: ReasoningEffortLevel,
    include_thought_level: bool,
    current_provider: &str,
    provider_options: Vec<acp::SessionConfigSelectOption>,
    current_model: &str,
    model_options: Vec<acp::SessionConfigSelectOption>,
) -> Vec<acp::SessionConfigOption> {
    let mode_options = [SessionMode::Ask, SessionMode::Architect, SessionMode::Code]
        .into_iter()
        .map(|mode| acp::SessionConfigSelectOption::new(mode.as_str(), session_mode_name(mode)))
        .collect::<Vec<_>>();

    let thought_level_options = ReasoningEffortLevel::allowed_values()
        .iter()
        .filter_map(|value| {
            ReasoningEffortLevel::parse(value).map(|level| {
                acp::SessionConfigSelectOption::new(level.as_str(), reasoning_effort_name(level))
            })
        })
        .collect::<Vec<_>>();

    let mut config_options = Vec::with_capacity(4);
    config_options.push(
        acp::SessionConfigOption::select(
            SESSION_CONFIG_MODE_ID,
            "Mode",
            current_mode.as_str(),
            mode_options,
        )
        .description("Controls whether VT Code answers, plans, or edits.")
        .category(acp::SessionConfigOptionCategory::Mode),
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
                "Thought level",
                reasoning_effort.as_str(),
                thought_level_options,
            )
            .description("Controls how much reasoning effort VT Code requests from the model.")
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
