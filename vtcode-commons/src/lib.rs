//! Shared traits and helper types reused across the component extraction
//! crates. The goal is to keep thin prototypes like `vtcode-llm` and
//! `vtcode-tools` decoupled from VT Code's internal configuration and
//! telemetry wiring while still sharing common contracts.
//!
//! See `docs/modules/vtcode_commons_reference.md` for ready-to-use adapters that
//! demonstrate how downstream consumers can wire these traits into their own
//! applications or tests.

pub mod ansi;
pub mod ansi_capabilities;
pub mod ansi_codes;
pub mod anstyle_utils;
pub mod async_utils;
pub mod at_pattern;
pub mod colors;
pub mod diff;
pub mod diff_paths;
pub mod errors;
pub mod formatting;
pub mod fs;
pub mod http;
pub mod image;
pub mod llm;
pub mod paths;
pub mod project;
pub mod reference;
pub mod sanitizer;
pub mod serde_helpers;
pub mod slug;
pub mod styling;
pub mod telemetry;
pub mod tokens;
pub mod unicode;
pub mod utils;
pub mod validation;
pub mod vtcodegitignore;

pub use colors::{blend_colors, color_from_hex, contrasting_color, is_light_color, style};
pub use errors::{DisplayErrorFormatter, ErrorFormatter, ErrorReporter, NoopErrorReporter};
pub use paths::{
    PathResolver, PathScope, WorkspacePaths, file_name_from_path, is_safe_relative_path,
    normalize_ascii_identifier, resolve_workspace_path,
};
pub use project::{ProjectOverview, build_project_overview};
pub use reference::{MemoryErrorReporter, MemoryTelemetry, StaticWorkspacePaths};
pub use styling::{ColorPalette, DiffColorPalette, render_styled};
pub use telemetry::{NoopTelemetry, TelemetrySink};
pub use tokens::{estimate_tokens, truncate_to_tokens};
pub use unicode::{UNICODE_MONITOR, UnicodeMonitor, UnicodeValidationContext};
