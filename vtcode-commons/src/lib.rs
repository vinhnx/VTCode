//! Shared traits and helper types reused across the component extraction
//! crates. The goal is to keep thin prototypes like `vtcode-llm` and
//! `vtcode-tools` decoupled from VT Code's internal configuration and
//! telemetry wiring while still sharing common contracts.
//!
//! See `docs/vtcode_commons_reference.md` for ready-to-use adapters that
//! demonstrate how downstream consumers can wire these traits into their own
//! applications or tests.

pub mod ansi;
pub mod async_utils;
pub mod at_pattern;
pub mod colors;
pub mod errors;
pub mod formatting;
pub mod fs;
pub mod http;
pub mod image;
pub mod paths;
pub mod reference;
pub mod serde_helpers;
pub mod slug;
pub mod telemetry;
pub mod utils;
pub mod validation;
pub mod vtcodegitignore;

pub use errors::{DisplayErrorFormatter, ErrorFormatter, ErrorReporter, NoopErrorReporter};
pub use paths::{PathResolver, PathScope, WorkspacePaths};
pub use reference::{MemoryErrorReporter, MemoryTelemetry, StaticWorkspacePaths};
pub use telemetry::{NoopTelemetry, TelemetrySink};
