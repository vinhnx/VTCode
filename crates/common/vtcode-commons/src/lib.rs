#![allow(missing_docs, clippy::expect_used)]
//! Shared primitives and helper types reused across VT Code crates.
//!
//! This crate provides the foundational building blocks that both the core
//! agent library (`vtcode-core`) and the terminal UI (`vtcode-ui`) depend on.
//! Modules include ANSI processing, diff rendering, file traversal, color
//! policy, error classification, and shared protocol types.
//!
//! Items live here (rather than `vtcode-ui`) when they are consumed by
//! `vtcode-core` or the main binary -- keeping the dependency direction clean.
//!
//! See `docs/modules/vtcode_commons_reference.md` for ready-to-use adapters.

pub mod ansi;
pub mod ansi_capabilities;
pub mod ansi_codes;
pub mod async_utils;
pub mod at_pattern;
pub mod cgp;
pub mod color256_theme;
pub mod color_policy;
pub mod colors;
pub mod diff;
pub mod diff_paths;
pub mod diff_preview;
pub mod diff_theme;
pub mod editor;
pub mod env_lock;
pub mod error_category;
pub mod errors;
pub mod exclusions;
pub mod file_input;
pub mod formatting;
pub mod fs;
pub mod http;
pub mod image;
pub mod interjection;
pub mod interner;
pub mod llm;
pub mod lr_map;
pub mod memory;
pub mod message_metadata;
pub mod model_family;
pub mod paths;
pub mod preview;
pub mod project;
pub mod provider;
pub mod reasoning;
pub mod reference;
pub mod retry;
pub mod sanitizer;
pub mod serde_helpers;
pub mod slug;
pub mod stop_hints;
pub mod styling;
pub mod telemetry;
pub mod terminal_detection;
pub mod thread_safety;
pub mod tokens;
pub mod tool_types;
pub mod trace_flush;
pub mod ui_protocol;
pub mod unicode;
pub mod utils;
pub mod validation;
pub mod vtcodegitignore;
pub mod walk;
pub mod workspace_snapshot;
pub use colors::{blend_colors, color_from_hex, contrasting_color, is_light_color, style};
pub use editor::{
    EditorPoint, EditorTarget, normalize_editor_hash_fragment, parse_editor_target, resolve_editor_path,
    resolve_editor_target,
};
pub use error_category::{
    BackoffStrategy, ErrorCategory, Retryability, classify_anyhow_error, classify_error_message,
    is_retryable_llm_error_message,
};
pub use errors::{DisplayErrorFormatter, ErrorFormatter, ErrorReporter, MultiErrors, NoopErrorReporter};
pub(crate) use interjection::{
    EventQueue, FormattedInterjection, InterjectionBuffer, LARGE_PROMPT_THRESHOLD, PendingInterjection,
    drain_formatted, format_interjection, user_query,
};
pub use interner::{StringId, StringInterner};
pub use paths::{
    PathExt, PathResolver, PathScope, StrPathExt, WorkspacePaths, canonicalize, canonicalize_async,
    file_name_from_path, is_safe_relative_path, normalize_ascii_identifier, resolve_workspace_path,
};
pub use project::{ProjectOverview, build_project_overview};
pub use reference::{MemoryErrorReporter, MemoryTelemetry, StaticWorkspacePaths};
pub use stop_hints::{STOP_HINT_COMPACT, STOP_HINT_INLINE, with_stop_hint};
pub use styling::{ColorPalette, DiffColorPalette, render_styled};
pub use telemetry::{NoopTelemetry, TelemetrySink};
pub use tokens::{estimate_tokens, truncate_to_tokens};
pub use unicode::{UNICODE_MONITOR, UnicodeMonitor, UnicodeValidationContext};

// Re-export key thread safety primitives.
pub(crate) use thread_safety::RelaxedAtomic;
