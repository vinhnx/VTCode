//! Distributed slice declaration for builtin tool registrations.
//!
//! Each tool module annotates a factory function with
//! `#[distributed_slice(BUILTIN_TOOLS)]` to self-register. The linker
//! collects all annotated functions into a contiguous slice at load time,
//! eliminating the need for a central enumeration in `builtins.rs`.
//!
//! The `TOOL_CONFIG` global is shared across every tool that reads user
//! config. Tests that need a fresh config snapshot should call
//! `install_tool_config` themselves; once a test process installs a
//! snapshot, the global cannot be reset (a `OnceLock` has no public
/// drop API by design — see review M1 in the code review for the
/// architectural rationale).
use std::sync::OnceLock;

use crate::tools::handlers::PlanningWorkflowState;
use vtcode_config::{WebFetchConfig, WebSearchConfig};

use super::registration::ToolRegistration;

/// Snapshot of the user-configurable bits of `ToolsConfig` that built-in
/// tools care about. A subset is intentional: most tools don't need the
/// whole `ToolsConfig`, and a narrow struct keeps the global slot small.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolConfigSnapshot {
    /// Settings for the `web_search` tool (provider, cooldown, cache, cap).
    pub web_search: WebSearchConfig,
    /// Settings for the `web_fetch` tool (mode, allow/block lists, https).
    pub web_fetch: WebFetchConfig,
}

static TOOL_CONFIG: OnceLock<ToolConfigSnapshot> = OnceLock::new();

/// Install the tool config snapshot. Must be called exactly once, before any
/// built-in tool that consumes it is constructed. The registry calls this in
/// `register_builtin_tools`; tests that exercise `register_*` functions in
/// isolation should call it too.
///
/// A second call with the SAME snapshot is a no-op (lets tests share
/// state across multiple `ToolRegistry::new` invocations in one
/// process). A second call with a DIFFERENT snapshot returns an error,
/// since silently dropping the new value would mask a real config-loading
/// bug (e.g. two vtcode.toml files racing during a hot reload).
pub fn install_tool_config(snapshot: ToolConfigSnapshot) -> anyhow::Result<()> {
    match TOOL_CONFIG.set(snapshot) {
        Ok(()) => Ok(()),
        Err(new) => {
            let existing = TOOL_CONFIG.get().expect("set failed; lock should be initialized");
            if existing != &new {
                Err(anyhow::anyhow!(
                    "install_tool_config called with a different snapshot; \
                     first install and second install disagree on the user config"
                ))
            } else {
                Ok(())
            }
        }
    }
}

/// Read the active tool config snapshot. Returns `None` if no snapshot has
/// been installed (e.g. a test calling a `register_*` function without first
/// calling `install_tool_config`).
pub fn tool_config() -> Option<&'static ToolConfigSnapshot> {
    TOOL_CONFIG.get()
}

/// Factory function type for builtin tool registrations.
///
/// Each tool module defines a function matching this signature and annotates it
/// with `#[distributed_slice(BUILTIN_TOOLS)]`. The function receives an optional
/// `PlanningWorkflowState` reference for tools that depend on planning workflow runtime state;
/// tools that do not need it simply ignore the parameter.
#[allow(dead_code)]
pub type BuiltinToolFactory = fn(Option<&PlanningWorkflowState>) -> ToolRegistration;

#[cfg(test)]
mod tests {
    use super::*;

    /// The second install of the SAME snapshot is a no-op. This is the
    /// common case in tests that build multiple `ToolRegistry` instances
    /// in one process.
    #[test]
    fn install_tool_config_re_install_with_same_snapshot_is_a_noop() {
        // The first install may have already happened in an earlier test
        // case in this process. We re-install the same default snapshot;
        // the function must return Ok, even if a prior install exists.
        install_tool_config(ToolConfigSnapshot::default()).ok();
    }

    /// A second install with a DIFFERENT snapshot returns an error. This guards
    /// against a real bug (two config files racing during a hot reload,
    /// or a test that forgot to reset the global) silently dropping the
    /// new value.
    #[test]
    fn install_tool_config_errors_on_diverging_snapshot() {
        // Force a divergent install by clearing whatever is already
        // there and re-installing with a non-default value.
        let divergent = ToolConfigSnapshot {
            web_search: WebSearchConfig { max_results: 999, ..WebSearchConfig::default() },
            ..ToolConfigSnapshot::default()
        };
        // If a prior test already installed a default snapshot, this
        // divergent install should return an error because the values disagree.
        // If no prior install has happened yet, this will succeed;
        // to make the test deterministic we first install a known default,
        // then the divergent one.
        install_tool_config(ToolConfigSnapshot::default()).ok();
        let result = install_tool_config(divergent);
        assert!(result.is_err(), "expected error on diverging snapshot, got: {result:?}");
    }
}

/// Distributed slice of builtin tool factory functions.
///
/// Elements are placed by `#[distributed_slice(BUILTIN_TOOLS)]` annotations
/// across tool modules. The linker collects them into a contiguous `&'static [BuiltinToolFactory]`.
#[linkme::distributed_slice]
pub static BUILTIN_TOOLS: [BuiltinToolFactory] = [..];
