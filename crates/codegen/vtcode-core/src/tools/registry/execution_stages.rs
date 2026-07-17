//! Extracted pipeline stages from `execute_tool_ref_internal`.
//!
//! Each stage is a focused, testable function that handles one step of the
//! tool execution pipeline. The main function in `execution_facade.rs`
//! orchestrates these stages.
//!
//! # Pipeline Stages
//!
//! 1. **resolve_tool_name** — alias resolution and canonical name lookup
//! 2. **check_planning_workflow** — enforce read-only during planning
//! 3. **check_circuit_breaker** — reject calls when breaker is open

use serde_json::Value;

use super::ToolRegistry;

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

    /// Check if a tool call should be denied due to an active planning workflow.
    ///
    /// Returns `None` if the call is allowed, or `Some(denial_message)` if denied.
    pub fn check_planning_workflow_for(
        &self,
        tool_name: &str,
        args: &Value,
        display_name: &str,
    ) -> Option<String> {
        if self.is_planning_active() && !self.is_planning_active_allowed(tool_name, args) {
            Some(crate::tools::error_messages::agent_execution::planning_workflow_denial_message(
                display_name,
            ))
        } else {
            None
        }
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
        if let Some((provider, remote_tool)) =
            crate::utils::tool_name_parsing::parse_canonical_mcp_tool_name(tool_name)
        {
            route.needs_pty = true;
            route.tool_exists = true;
            route.is_mcp = true;
            route.mcp_provider = Some(provider.to_string());
            route.mcp_tool_name = Some(remote_tool.to_string());
        }

        route
    }

    /// Check if a full-auto policy denies this tool.
    ///
    /// Returns `None` if allowed, or `Some(error_message)` if denied.
    pub async fn check_full_auto_denied(
        &self,
        tool_name: &str,
        display_name: &str,
    ) -> Option<String> {
        // Delegate to the existing method on ToolSecurity (implemented via trait_impls).
        if self.is_allowed_in_full_auto(tool_name).await {
            None
        } else {
            Some(format!(
                "Tool '{display_name}' is not permitted while full-auto permission review is active"
            ))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_tool_name_is_alias_when_names_differ() {
        let resolved = ResolvedToolName {
            canonical: "read_file".to_string(),
            display: "cat (alias for read_file)".to_string(),
            is_alias: true,
        };
        assert!(resolved.is_alias);
        assert_eq!(resolved.canonical, "read_file");
        assert!(resolved.display.contains("alias"));
    }

    #[test]
    fn resolved_tool_name_not_alias_when_names_match() {
        let resolved = ResolvedToolName {
            canonical: "read_file".to_string(),
            display: "read_file".to_string(),
            is_alias: false,
        };
        assert!(!resolved.is_alias);
        assert_eq!(resolved.canonical, resolved.display);
    }
}
