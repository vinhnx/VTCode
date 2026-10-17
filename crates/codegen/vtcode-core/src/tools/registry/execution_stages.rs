//! Extracted pipeline stages from `execute_tool_ref_internal`.
//!
//! Each stage is a focused, testable function that handles one step of the
//! tool execution pipeline. The main function in `execution_facade.rs`
//! orchestrates these stages.
//!
//! # Pipeline Stages
//!
//! 1. **resolve_tool_name** — alias resolution and canonical name lookup
//! 2. **prepare_execution_args** — normalization and handler preview metadata
//! 3. **resolve_tool_route** — registered and canonical MCP route metadata
//! 4. **resolve_execution_route** — awaited MCP discovery and lookup errors
//! 5. **check_circuit_breaker** — reject calls when breaker is open
//!
//! Planning-workflow enforcement lives in `execution_facade.rs` /
//! `execution_kernel.rs` on the already-classified intent, not here.

use anyhow::Result;
use serde_json::Value;
use std::borrow::Cow;
use tracing::{trace, warn};

use crate::mcp::McpToolExecutor;
use crate::tools::mcp::legacy_mcp_tool_name;

use crate::tools::{output_limits, tool_intent};

use super::{ToolRegistry, execution_kernel};

/// Normalized handler arguments and independently resolved preview metadata.
pub(super) struct ExecutionArgs<'a> {
    pub(super) handler_args: Cow<'a, Value>,
    pub(super) max_output_tokens: usize,
    pub(super) is_verification_command: bool,
}

/// Resolved tool name information.
pub struct ResolvedToolName {
    /// Canonical tool name (after alias resolution).
    pub canonical: String,
    /// Display name for error messages (includes alias info).
    pub display: String,
    /// Whether the name was resolved from an alias.
    pub is_alias: bool,
}

impl ToolRegistry {
    /// Resolve a requested tool name to its canonical form.
    ///
    /// Handles alias resolution through the inventory's registration lookup.
    /// If the name is not found, it's used as-is (for MCP tools or error handling).
    pub fn resolve_tool_name_with_display(&self, name: &str) -> ResolvedToolName {
        if let Some(registration) = self.inventory.registration_for(name) {
            let canonical = registration.name().to_string();
            let display = if canonical == name {
                canonical.clone()
            } else {
                format!("{name} (alias for {canonical})")
            };
            ResolvedToolName {
                canonical,
                display,
                is_alias: name != registration.name(),
            }
        } else {
            ResolvedToolName {
                canonical: name.to_string(),
                display: name.to_string(),
                is_alias: false,
            }
        }
    }

    /// Prepare arguments without granting preflight, policy, or safety admission.
    pub(super) fn prepare_execution_args<'a>(&self, tool_name: &str, args: &'a Value) -> Result<ExecutionArgs<'a>> {
        let parameter_schema = self
            .inventory
            .registration_for(tool_name)
            .and_then(|registration| registration.parameter_schema().cloned());
        let normalized_args = execution_kernel::normalize_tool_args(tool_name, args, parameter_schema.as_ref())?;
        // Classify before stripping output metadata: verification calls retain
        // full preview budgets and must remain exempt from result reuse.
        let is_verification_command = self.is_completion_verifier_call(tool_name, normalized_args.as_ref())
            || matches!(
                tool_intent::classify_shell_activity(tool_name, normalized_args.as_ref()),
                tool_intent::ShellActivity::Verification
            );
        let max_output_tokens = output_limits::resolve_max_output_tokens(
            normalized_args.as_ref(),
            self.is_planning_active(),
            is_verification_command,
        )?;
        let handler_args = if output_limits::handler_accepts_output_metadata(parameter_schema.as_ref()) {
            normalized_args
        } else {
            Cow::Owned(output_limits::args_without_output_metadata(normalized_args.as_ref()))
        };
        Ok(ExecutionArgs {
            handler_args,
            max_output_tokens,
            is_verification_command,
        })
    }

    /// Check if a tool call should be rejected by the circuit breaker.
    ///
    /// Returns `None` if the call is allowed, or `Some(error_message)` if rejected.
    pub fn check_circuit_breaker_for(&self, tool_name: &str, display_name: &str) -> Option<String> {
        let shared_breaker = self.shared_circuit_breaker();
        if let Some(breaker) = shared_breaker.as_ref()
            && !breaker.allow_request_for_tool(tool_name)
        {
            let diagnostics = breaker.get_diagnostics(tool_name);
            let retry_after = diagnostics
                .remaining_backoff
                .map(|backoff| format!(" retry_after={}s.", backoff.as_secs()))
                .unwrap_or_default();
            Some(format!(
                "Tool '{display_name}' is temporarily disabled due to high failure rate (Circuit Breaker OPEN).{retry_after}"
            ))
        } else {
            None
        }
    }

    /// Resolve whether a tool is standard, MCP, or unknown.
    ///
    /// Returns the tool routing information needed for execution.
    pub fn resolve_tool_route(&self, tool_name: &str) -> ToolRoute {
        let mut route = ToolRoute {
            needs_pty: false,
            tool_exists: false,
            is_mcp: false,
            mcp_provider: None,
            mcp_tool_name: None,
        };

        // Check standard tools first
        if let Some(registration) = self.inventory.registration_for(tool_name) {
            route.needs_pty = registration.uses_pty();
            route.tool_exists = true;
        }

        // Check canonical MCP format
        if let Some((provider, remote_tool)) = crate::utils::tool_name_parsing::parse_canonical_mcp_tool_name(tool_name)
        {
            route.needs_pty = true;
            route.tool_exists = true;
            route.is_mcp = true;
            route.mcp_provider = Some(provider.to_string());
            route.mcp_tool_name = Some(remote_tool.to_string());
        }

        route
    }

    /// Resolve discovery metadata after policy constraints, without recording or executing.
    pub(super) async fn resolve_execution_route(&self, requested_name: &str, tool_name: &str) -> ExecutionRoute {
        let mut route = self.resolve_tool_route(tool_name);
        let mut mcp_lookup_error = None;

        let mcp_client_opt = self.mcp_client.read().clone();
        if !route.is_mcp
            && let Some(mcp_client) = mcp_client_opt
        {
            let mut resolved_mcp_name = legacy_mcp_tool_name(requested_name)
                .map(str::to_string)
                .unwrap_or_else(|| tool_name.to_string());

            if let Some(alias_target) = self.resolve_mcp_tool_alias(&resolved_mcp_name).await
                && alias_target != resolved_mcp_name
            {
                trace!(
                    requested = %resolved_mcp_name,
                    resolved = %alias_target,
                    "Resolved MCP tool alias"
                );
                resolved_mcp_name = alias_target;
            }

            match mcp_client.has_mcp_tool(&resolved_mcp_name).await {
                Ok(true) => {
                    route.needs_pty = true;
                    route.tool_exists = true;
                    route.is_mcp = true;
                    route.mcp_provider = self.find_mcp_provider(&resolved_mcp_name).await;
                    route.mcp_tool_name = Some(resolved_mcp_name);
                }
                Ok(false) => {
                    // Don't modify tool_exists here - keep the result from standard tool check.
                    // Setting route.tool_exists = false would incorrectly override a valid standard tool.
                }
                Err(err) => {
                    warn!("Error checking MCP tool '{}': {}", resolved_mcp_name, err);
                    mcp_lookup_error = Some(err);
                }
            }
        }

        ExecutionRoute { route, mcp_lookup_error }
    }

    /// Check if a full-auto policy denies this tool.
    ///
    /// Returns `None` if allowed, or `Some(error_message)` if denied.
    pub async fn check_full_auto_denied(&self, tool_name: &str, display_name: &str) -> Option<String> {
        if self.is_denied_in_full_auto(tool_name).await {
            Some(format!("Tool '{display_name}' is not permitted while full-auto permission review is active"))
        } else {
            None
        }
    }
}

/// Result of tool route resolution.
pub struct ToolRoute {
    /// Whether the tool needs a PTY session.
    pub needs_pty: bool,
    /// Whether the tool exists in any registry.
    pub tool_exists: bool,
    /// Whether the tool is an MCP tool.
    pub is_mcp: bool,
    /// The MCP provider name, if applicable.
    pub mcp_provider: Option<String>,
    /// The remote MCP tool name, if applicable.
    pub mcp_tool_name: Option<String>,
}

/// Discovery errors coexist with standard routes so remote lookup cannot erase them.
pub(super) struct ExecutionRoute {
    pub(super) route: ToolRoute,
    pub(super) mcp_lookup_error: Option<anyhow::Error>,
}

#[cfg(test)]
mod tests;
