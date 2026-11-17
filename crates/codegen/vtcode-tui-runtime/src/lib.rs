#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "Extracted rendering code preserves vtcode-core's existing lint posture."
)]
#![allow(
    unused_results,
    clippy::let_underscore_must_use,
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_lossless,
    reason = "Extracted rendering code keeps validated text offsets and protocol-sized counters, matching core."
)]
//! Terminal rendering and inline-UI runtime for VT Code.
//!
//! This crate owns the TUI-flavoured code that used to live behind
//! `vtcode-core`'s `tui` feature: ANSI rendering, transcript presentation,
//! theming, the inline-session protocol, and the terminal setup wizard.
//! `vtcode-core` has no dependency on `vtcode-ui` or a terminal stack, so
//! headless consumers such as `vtcode-acp` compile a genuinely UI-free graph.

pub mod interfaces;
pub mod terminal_setup;
pub mod ui;
pub mod utils;
