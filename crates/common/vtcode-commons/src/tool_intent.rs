//! Pure tool-intent probes shared with surfaces that cannot use the full classifier.
//!
//! `vtcode-core::tools::tool_intent` owns intent classification for dispatch,
//! security, and execution. The probes here are the pure JSON subset a
//! transcript needs to route display — session-action routing for the
//! exec-session labels ([`crate::tool_headlines`]) — so the desktop app,
//! which must not depend on `vtcode-core`, shares them instead of mirroring
//! them. Core re-exports both probes, so every existing import path keeps
//! working and dispatch call sites are untouched.

use serde_json::Value;

/// Whether the payload carries indexed command parts (`command.0`/`command.1`).
///
/// Only the `0`/`1` seeds are probed: their presence proves the indexed form,
/// and deeper indices never appear without them.
#[must_use]
pub fn has_indexed_command_parts(args: &Value) -> bool {
    let Some(payload) = args.as_object() else {
        return false;
    };

    payload.contains_key("command.0") || payload.contains_key("command.1")
}

/// Determine the action for command-session calls based on args.
///
/// Returns the action string or `None` when no inference is possible. An
/// explicit `action` wins; otherwise standard command, code, input, inspect,
/// and session-id fields route in that order.
#[must_use]
pub fn command_session_action(args: &Value) -> Option<&str> {
    args.get("action").and_then(|v| v.as_str()).or_else(|| {
        // Check for standard command fields
        if args.get("command").is_some()
            || args.get("cmd").is_some()
            || args.get("raw_command").is_some()
            || has_indexed_command_parts(args)
        {
            Some("run")
        } else if args.get("code").is_some() {
            Some("code")
        } else if args.get("input").is_some() || args.get("chars").is_some() || args.get("text").is_some() {
            Some("write")
        } else if args.get("spool_path").is_some()
            || args.get("query").is_some()
            || args.get("head_lines").is_some()
            || args.get("tail_lines").is_some()
            || args.get("max_matches").is_some()
            || args.get("literal").is_some()
        {
            Some("inspect")
        } else if args.get("session_id").is_some() || args.get("s").is_some() {
            Some("poll")
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests;
