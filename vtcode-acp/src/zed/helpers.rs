use agent_client_protocol as acp;
use vtcode_core::core::interfaces::SessionMode;

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

pub(crate) fn text_chunk(text: impl Into<String>) -> acp::ContentChunk {
    acp::ContentChunk::new(acp::ContentBlock::from(text.into()))
}

pub(crate) fn agent_implementation_info(title_override: Option<String>) -> acp::Implementation {
    acp::Implementation::new("vtcode", env!("CARGO_PKG_VERSION"))
        .title(title_override.or_else(|| Some("VT Code".to_string())))
}

pub(crate) fn build_available_commands() -> Vec<acp::AvailableCommand> {
    vec![
        acp::AvailableCommand::new("init", "Create vtcode.toml and index the workspace").input(
            acp::AvailableCommandInput::Unstructured(acp::UnstructuredCommandInput::new(
                "Optional: --force flag",
            )),
        ),
        acp::AvailableCommand::new("config", "Browse vtcode.toml settings sections"),
        acp::AvailableCommand::new("status", "Show model, provider, workspace, and tool status"),
        acp::AvailableCommand::new("doctor", "Run installation and configuration diagnostics"),
        acp::AvailableCommand::new(
            "plan",
            "Toggle between Code and Architect modes for read-only planning",
        )
        .input(acp::AvailableCommandInput::Unstructured(
            acp::UnstructuredCommandInput::new("Optional: on | off"),
        )),
        acp::AvailableCommand::new("mode", "Cycle through Ask -> Architect -> Code modes"),
        acp::AvailableCommand::new("help", "Show slash command help"),
        acp::AvailableCommand::new("reset", "Reset conversation context"),
        acp::AvailableCommand::new("tools", "List tools and their descriptions"),
        acp::AvailableCommand::new("exit", "Close the VT Code session"),
    ]
}
