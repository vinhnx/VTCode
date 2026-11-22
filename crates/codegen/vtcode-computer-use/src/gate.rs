//! Computer-use route admission and live-probe records.
//!
//! Admission is by declared native route (provider + protocol wire name): any
//! model and any endpoint on a route the provider actually implements is
//! admitted, so new models and custom proxies work without a gate change.
//! Unknown routes stay denied before any network I/O.
//!
//! [`VERIFIED_MATRIX`] records tuples proven by live acceptance runs. It is
//! advisory documentation of what was probed, not an admission list.

use vtcode_commons::provider::Provider;

/// Wire name of the OpenAI Responses `computer` tool. The provider codecs
/// declare this constant rather than restating the string, so a wire-version
/// bump cannot silently desynchronize declaration, gate, and parser.
pub const OPENAI_COMPUTER_PROTOCOL: &str = "computer";
/// Wire name of the Anthropic Messages computer toolset.
pub const ANTHROPIC_COMPUTER_PROTOCOL: &str = "computer_toolset_20260801";

/// Native computer-use routes implemented and contract-tested in `vtcode-llm`.
///
/// Both halves of a route are canonical here: provider identity comes from
/// [`Provider`] and protocol wire names from the constants above. Providers
/// re-export those constants as their `computer_use::PROTOCOL`, so this table
/// is the single source of truth for route admission.
pub const NATIVE_ROUTES: &[(Provider, &str)] = &[
    (Provider::OpenAI, OPENAI_COMPUTER_PROTOCOL),
    (Provider::Anthropic, ANTHROPIC_COMPUTER_PROTOCOL),
];

/// Whether a provider implements a native computer-use route. Model and
/// endpoint are deliberately absent: every model on a declared route can
/// drive a turn, including models released after this crate was built and
/// requests sent through a custom base URL.
#[must_use]
pub fn native_route_supported(provider: Provider, protocol: &str) -> bool {
    NATIVE_ROUTES.contains(&(provider, protocol))
}

/// One tuple proven by a live acceptance probe. Advisory only; see
/// [`native_route_supported`] for admission.
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

/// Whether [`VERIFIED_MATRIX`] recorded a probe for the target. Never used
/// for admission: see [`native_route_supported`].
#[must_use]
pub fn verified_target_default(provider: &str, model: &str, endpoint: &str, protocol: &str) -> bool {
    verified_target(VERIFIED_MATRIX, provider, model, endpoint, protocol)
}

#[cfg(test)]
mod tests;
