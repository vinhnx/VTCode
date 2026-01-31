use agent_client_protocol as acp;

use super::constants::{MODE_ID_ARCHITECT, MODE_ID_ASK, MODE_ID_CODE};

pub(crate) fn acp_session_modes() -> Vec<acp::SessionMode> {
    vec![
        acp::SessionMode::new(MODE_ID_ASK, "Ask")
            .description("Request permission before making any changes"),
        acp::SessionMode::new(MODE_ID_ARCHITECT, "Architect")
            .description("Design and plan software systems without implementation"),
        acp::SessionMode::new(MODE_ID_CODE, "Code")
            .description("Write and modify code with full tool access"),
    ]
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
        acp::AvailableCommand::new("config", "View the effective vtcode.toml configuration"),
        acp::AvailableCommand::new("status", "Show model, provider, workspace, and tool status"),
        acp::AvailableCommand::new("doctor", "Run installation and configuration diagnostics"),
        acp::AvailableCommand::new(
            "plan",
            "Toggle Plan Mode: read-only exploration and planning",
        )
        .input(acp::AvailableCommandInput::Unstructured(
            acp::UnstructuredCommandInput::new("Optional: on | off"),
        )),
        acp::AvailableCommand::new("mode", "Cycle through Edit -> Plan -> Agent modes"),
        acp::AvailableCommand::new("help", "Show slash command help"),
        acp::AvailableCommand::new("reset", "Reset conversation context"),
        acp::AvailableCommand::new("tools", "List tools and their descriptions"),
        acp::AvailableCommand::new("exit", "Close the VT Code session"),
    ]
}
