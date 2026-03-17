pub use crate::core::SessionAppearanceConfig;
pub use crate::core_tui::app::types::*;
pub use crate::options::{KeyboardProtocolSettings, SessionSurface};
pub use crate::session_options::{
    SessionOptions, spawn_session_with_host, spawn_session_with_options,
};

/// Commonly used VT Code TUI API items.
pub mod prelude {
    pub use super::{
        InlineCommand, InlineEvent, InlineHandle, InlineMessageKind, InlineSegment, InlineSession,
        InlineTextStyle, InlineTheme, KeyboardProtocolSettings, PlanContent, PlanPhase, PlanStep,
        SessionAppearanceConfig, SessionOptions, SessionSurface, SlashCommandItem, TrustMode,
        WizardModalMode, WizardStep, spawn_session_with_host, spawn_session_with_options,
    };
}
