#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
pub(super) use super::super::*;
pub(super) use super::helpers::*;

fn load_primary_agent_palette(session: &mut AppSession) {
    session.handle_command(app_types::InlineCommand::ShowTransient {
        request: Box::new(app_types::TransientRequest::AgentPalette(app_types::AgentPaletteTransientRequest {
            agents: vec![
                app_types::AgentPaletteItem { name: "beta".to_string(), description: None },
                app_types::AgentPaletteItem { name: "alpha".to_string(), description: None },
            ],
            visible: None,
        })),
    });
    session.close_transient();
}

mod activity;
mod drawer;
mod hints;
mod panel;
mod tab_cycle;
