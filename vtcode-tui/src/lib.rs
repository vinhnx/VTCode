//! `vtcode-tui` exposes VT Code's inline terminal UI API as a reusable crate.
//!
//! The migrated implementation source lives in `src/core_tui/`.
//! The stable public API currently re-exports `vtcode_core::ui::tui`.

pub use vtcode_core::{auth, cache, config, llm, notifications, tools, utils};

pub mod core_tui;
pub mod host;
mod session_options;

pub use session_options::{
    KeyboardProtocolSettings, SessionOptions, SessionSurface, spawn_session_with_host,
    spawn_session_with_options,
};
pub use vtcode_core::ui::tui::*;

/// Compatibility namespace for migrated code that still references `crate::ui::*`.
mod ui {
    pub use vtcode_core::ui::FileColorizer;

    pub mod markdown {
        pub use vtcode_core::ui::markdown::*;
    }

    pub mod search {
        pub use vtcode_core::ui::search::*;
    }

    pub mod slash {
        pub use vtcode_core::ui::slash::*;
    }

    pub mod syntax_highlight {
        pub use vtcode_core::ui::syntax_highlight::*;
    }

    pub mod theme {
        pub use vtcode_core::ui::theme::*;
    }

    pub mod tui {
        pub use crate::core_tui::*;
    }
}

/// Commonly used TUI API items.
pub mod prelude {
    pub use crate::{
        EditingMode, InlineCommand, InlineEvent, InlineHandle, InlineMessageKind, InlineSegment,
        InlineSession, InlineTextStyle, InlineTheme, KeyboardProtocolSettings,
        PlanConfirmationResult, PlanContent, PlanPhase, PlanStep, SecurePromptConfig,
        SessionOptions, SessionSurface, TrustMode, WizardModalMode, WizardStep, convert_style,
        spawn_session, spawn_session_with_host, spawn_session_with_options,
        spawn_session_with_prompts, theme_from_styles,
    };
}
