//! Re-exports of shared configuration types from their canonical definitions,
//! plus locally-defined types that need to satisfy Rust's orphan rule for
//! trait implementations.

use serde::{Deserialize, Serialize};
use std::fmt;

// Re-export from canonical sources.
pub use vtcode_commons::reasoning::ReasoningEffortLevel;
pub use vtcode_config::types::{SystemPromptMode, ToolDocumentationMode, VerbosityLevel};

/// UI surface preference for rendering.
///
/// Kept as a local definition (rather than re-exported from vtcode-config)
/// because `vtcode-ui` implements `From<SessionSurface>` and
/// `From<UiSurfacePreference> for SessionSurface`, which require a local type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum UiSurfacePreference {
    #[default]
    Auto,
    Alternate,
    Inline,
}

impl UiSurfacePreference {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Alternate => "alternate",
            Self::Inline => "inline",
        }
    }
}

impl fmt::Display for UiSurfacePreference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
