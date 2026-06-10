//! Unified UI crate for VT Code: design system, theme registry, and TUI framework.
//!
//! # Module layout
//!
//! - [`design`] — Color conversion, style bridging, layout, diff, panel primitives
//! - [`theme`] — Theme registry, runtime state, syntax theme resolution
//! - [`tui`]   — Full TUI framework (session, widgets, runner, markdown, etc.)
//!
//! Items from `design` and `theme` are also re-exported at the crate root for
//! backward-compatibility with callers that previously imported from the
//! standalone `vtcode-design` / `vtcode-theme` crates (now consolidated into `vtcode-ui`).

pub mod design;
pub mod theme;
pub mod tui;

// Backward-compat re-exports so `vtcode_ui::ThemeStyles`, `vtcode_ui::color::*`,
// etc. continue to work without path-qualified imports.
pub use design::*;
pub use theme::*;
