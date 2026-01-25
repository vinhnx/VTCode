use agent_client_protocol as acp;

use super::constants::{MODE_ID_ARCHITECT, MODE_ID_ASK, MODE_ID_CODE};

pub(crate) fn acp_session_modes() -> Vec<acp::SessionMode> {
    vec![
        acp::SessionMode {
            id: acp::SessionModeId(MODE_ID_ASK.into()),
            name: "Ask".to_string(),
            description: Some("Request permission before making any changes".to_string()),
            meta: None,
        },
        acp::SessionMode {
            id: acp::SessionModeId(MODE_ID_ARCHITECT.into()),
            name: "Architect".to_string(),
            description: Some(
                "Design and plan software systems without implementation".to_string(),
            ),
            meta: None,
        },
        acp::SessionMode {
            id: acp::SessionModeId(MODE_ID_CODE.into()),
            name: "Code".to_string(),
            description: Some("Write and modify code with full tool access".to_string()),
            meta: None,
        },
    ]
}

pub(crate) fn text_chunk(text: impl Into<String>) -> acp::ContentChunk {
    acp::ContentChunk {
        content: acp::ContentBlock::from(text.into()),
        meta: None,
    }
}

pub(crate) fn agent_implementation_info() -> acp::Implementation {
    acp::Implementation {
        name: "vtcode".to_string(),
        title: Some("VT Code".to_string()),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

pub(crate) fn build_available_commands() -> Vec<acp::AvailableCommand> {
    vec![
        acp::AvailableCommand {
            name: "init".to_string(),
            description: "Create vtcode.toml and index the workspace".to_string(),
            input: Some(acp::AvailableCommandInput::Unstructured {
                hint: "Optional: --force flag".to_string(),
            }),
            meta: None,
        },
        acp::AvailableCommand {
            name: "config".to_string(),
            description: "View the effective vtcode.toml configuration".to_string(),
            input: None,
            meta: None,
        },
        acp::AvailableCommand {
            name: "status".to_string(),
            description: "Show model, provider, workspace, and tool status".to_string(),
            input: None,
            meta: None,
        },
        acp::AvailableCommand {
            name: "doctor".to_string(),
            description: "Run installation and configuration diagnostics".to_string(),
            input: None,
            meta: None,
        },
        acp::AvailableCommand {
            name: "plan".to_string(),
            description: "Toggle Plan Mode: read-only exploration and planning".to_string(),
            input: Some(acp::AvailableCommandInput::Unstructured {
                hint: "Optional: on | off".to_string(),
            }),
            meta: None,
        },
        acp::AvailableCommand {
            name: "mode".to_string(),
            description: "Cycle through Edit -> Plan -> Agent modes".to_string(),
            input: None,
            meta: None,
        },
        acp::AvailableCommand {
            name: "help".to_string(),
            description: "Show slash command help".to_string(),
            input: None,
            meta: None,
        },
        acp::AvailableCommand {
            name: "reset".to_string(),
            description: "Reset conversation context".to_string(),
            input: None,
            meta: None,
        },
        acp::AvailableCommand {
            name: "tools".to_string(),
            description: "List tools and their descriptions".to_string(),
            input: None,
            meta: None,
        },
        acp::AvailableCommand {
            name: "exit".to_string(),
            description: "Close the VT Code session".to_string(),
            input: None,
            meta: None,
        },
    ]
}
