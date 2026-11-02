pub use crate::tui::core::SessionAppearanceConfig;
pub use crate::tui::core_tui::app::types::*;
pub use crate::tui::options::{FullscreenInteractionSettings, KeyboardProtocolSettings, SessionSurface};
pub use crate::tui::session_options::{
    HostCommandReceiver, HostInteraction, HostInteractionChoice, HostInteractionKind, HostInteractionStep, HostSession,
    SessionOptions, host_interaction_from_command, host_interaction_response, new_host_session,
    spawn_session_with_host, spawn_session_with_options,
};

/// Commonly used VT Code TUI API items.
pub mod prelude {
    pub use super::{
        FullscreenInteractionSettings, HostCommandReceiver, HostSession, InlineCommand, InlineEvent, InlineHandle,
        InlineMessageKind, InlineSegment, InlineSession, InlineTextStyle, InlineTheme, KeyboardProtocolSettings,
        PlanContent, PlanPhase, PlanStep, SessionAppearanceConfig, SessionOptions, SessionSurface, SlashCommandItem,
        TrustMode, WizardModalMode, WizardStep, new_host_session, spawn_session_with_host, spawn_session_with_options,
    };
}
