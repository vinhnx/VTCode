//! Re-exports of shared configuration types from their canonical definitions.
//!
//! Types that need to convert to/from `vtcode-commons::ui_protocol` types live
//! in `vtcode-config`, which owns them: a local type in the trait's generic
//! parameter slot is enough to satisfy the orphan rule, so no UI-side copy is
//! needed.

// Re-export from canonical sources.
pub use vtcode_commons::reasoning::ReasoningEffortLevel;
pub use vtcode_config::types::{SystemPromptMode, ToolDocumentationMode, UiSurfacePreference, VerbosityLevel};
