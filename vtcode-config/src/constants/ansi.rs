//! ANSI escape sequence parsing constants
//!
//! These constants prevent infinite loops when parsing malformed ANSI sequences.
//! See: https://github.com/anthropics/claude-code/issues/22094

/// Maximum length for OSC/DCS/PM/APC/SOS escape sequences before bailing out.
/// OSC sequences rarely exceed 4KB in practice (e.g., hyperlinks, window titles).
/// This guard prevents CPU spinning on malformed input.
pub const ANSI_MAX_ESCAPE_SEQ_LENGTH: usize = 4096;

/// Maximum parameter length for CSI sequences (e.g., ESC[...m).
/// CSI parameters are typically short numeric values separated by semicolons.
pub const ANSI_MAX_CSI_PARAM_LENGTH: usize = 20;
