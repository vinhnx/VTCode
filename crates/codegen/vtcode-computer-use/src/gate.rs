//! Verified computer-use target matrix.
//!
//! A (provider, model, endpoint, protocol) tuple becomes usable only after
//! the acceptance runs in the approved plan prove it live. The matrix grows
//! one entry per accepted probe: unknown tuples stay denied, and every
//! denial happens before any network I/O.

/// One live-verified computer-use target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerifiedTarget {
    /// Provider key, e.g. `"anthropic"`.
    pub provider: &'static str,
    /// Exact model identifier.
    pub model: &'static str,
    /// Exact API endpoint the probe ran against.
    pub endpoint: &'static str,
    /// Native protocol version, e.g. `"computer_toolset_20260801"`.
    pub protocol: &'static str,
}

/// Targets proven by live acceptance runs. Each entry records a probe that
/// returned `computer_call` for the exact tuple; unknown tuples stay denied.
pub const VERIFIED_MATRIX: &[VerifiedTarget] = &[
    // Accepted 2026-11-19 by scripts/probe_openai_computer_use.py: P1
    // declared tools=[{type:computer}] and received computer_call with a
    // screenshot action; P2 replayed a real container screenshot
    // statelessly (store=false, no previous_response_id) and the route
    // continued with reasoning + computer_call. ~3k tokens on gpt-6-luna.
    VerifiedTarget {
        provider: "openai",
        model: "gpt-6-luna",
        endpoint: "https://api.openai.com/v1",
        protocol: "computer",
    },
];

/// Whether `matrix` admits an exact target match.
#[must_use]
pub fn verified_target(matrix: &[VerifiedTarget], provider: &str, model: &str, endpoint: &str, protocol: &str) -> bool {
    matrix.iter().any(|target| {
        target.provider == provider
            && target.model == model
            && target.endpoint == endpoint
            && target.protocol == protocol
    })
}

/// Whether [`VERIFIED_MATRIX`] admits the target.
#[must_use]
pub fn verified_target_default(provider: &str, model: &str, endpoint: &str, protocol: &str) -> bool {
    verified_target(VERIFIED_MATRIX, provider, model, endpoint, protocol)
}

#[cfg(test)]
mod tests;
