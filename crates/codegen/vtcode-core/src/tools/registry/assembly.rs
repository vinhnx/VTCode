use rustc_hash::FxHashMap;

use crate::config::constants::tools;
use crate::tool_policy::ToolPolicy;
use crate::tools::handlers::{SessionToolCatalog, ToolCallError};
use crate::tools::names::canonical_tool_name;

use super::{ToolMetadata, ToolRegistration};

#[derive(Debug, Clone, PartialEq)]
pub(super) struct PublicToolResolution {
    registration_name: String,
    default_permission: ToolPolicy,
}

impl PublicToolResolution {
    fn new(registration_name: String, default_permission: ToolPolicy) -> Self {
        Self { registration_name, default_permission }
    }

    pub(super) fn registration_name(&self) -> &str {
        self.registration_name.as_str()
    }

    pub(super) fn default_permission(&self) -> &ToolPolicy {
        &self.default_permission
    }
}

pub(super) struct ToolAssembly {
    policy_seed_metadata: FxHashMap<String, ToolMetadata>,
    catalog: SessionToolCatalog,
    public_routes: FxHashMap<String, PublicToolResolution>,
}

impl ToolAssembly {
    pub(super) fn empty() -> Self {
        Self::from_registrations(Vec::new())
    }

    pub(super) fn from_registrations(registrations: Vec<ToolRegistration>) -> Self {
        let registration_metadata = registrations
            .iter()
            .map(|registration| (registration.name().to_string(), registration.metadata().clone()))
            .collect::<FxHashMap<_, _>>();
        let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
        let public_routes = build_public_routes(&catalog);
        let policy_seed_metadata = catalog
            .entries()
            .iter()
            .filter_map(|entry| {
                registration_metadata
                    .get(&entry.registration_name)
                    .cloned()
                    .map(|metadata| (entry.registration_name.clone(), metadata))
            })
            .collect();
        Self { policy_seed_metadata, catalog, public_routes }
    }

    pub(super) fn policy_seed_metadata(&self) -> &FxHashMap<String, ToolMetadata> {
        &self.policy_seed_metadata
    }

    pub(super) fn catalog(&self) -> &SessionToolCatalog {
        &self.catalog
    }

    pub(super) fn resolve_public_tool(&self, requested_name: &str) -> Result<PublicToolResolution, ToolCallError> {
        // Match the first candidate before allocating fallback name variants.
        // Candidate construction unwraps once at entry and again on insertion.
        let first_candidate = strip_wrapping_quotes(strip_wrapping_quotes(requested_name));
        if !first_candidate.is_empty()
            && let Some(resolution) = self.public_routes.get(first_candidate)
        {
            return Ok(resolution.clone());
        }
        public_tool_name_candidates(requested_name)
            .into_iter()
            .find_map(|candidate| self.public_routes.get(&candidate).cloned())
            .ok_or_else(|| ToolCallError::respond(format!("Unknown tool: {}", canonical_tool_name(requested_name))))
    }
}

pub(super) fn public_tool_name_candidates(name: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let raw = strip_wrapping_quotes(name);
    if raw.is_empty() {
        return candidates;
    }

    push_candidate(&mut candidates, raw);

    if let Some((lhs, rhs)) = raw.split_once("<|channel|>") {
        push_candidate(&mut candidates, rhs);
        push_candidate(&mut candidates, lhs);
    }

    if let Some((_, suffix)) = raw.rsplit_once(':') {
        push_candidate(&mut candidates, suffix);
    }

    candidates
}

fn build_public_routes(catalog: &SessionToolCatalog) -> FxHashMap<String, PublicToolResolution> {
    let mut public_routes = FxHashMap::default();

    for entry in catalog.entries() {
        if is_removed_public_tool_name(entry.public_name.as_str()) {
            continue;
        }

        let resolution = PublicToolResolution::new(entry.registration_name.clone(), entry.default_permission.clone());
        public_routes.insert(entry.public_name.clone(), resolution.clone());
        public_routes
            .entry(entry.registration_name.clone())
            .or_insert_with(|| resolution.clone());
        for alias in &entry.aliases {
            if is_removed_public_tool_name(alias) {
                continue;
            }
            public_routes.entry(alias.clone()).or_insert_with(|| resolution.clone());
        }
    }

    public_routes
}

fn is_removed_public_tool_name(name: &str) -> bool {
    let name = name.trim();
    [
        tools::UNIFIED_EXEC,
        tools::UNIFIED_FILE,
        tools::UNIFIED_SEARCH,
        tools::LIST_FILES,
        tools::READ_FILE,
        tools::WRITE_FILE,
        tools::EDIT_FILE,
        tools::CREATE_FILE,
        tools::DELETE_FILE,
        tools::MOVE_FILE,
        tools::COPY_FILE,
        tools::SEARCH_REPLACE,
        tools::FILE_OP,
    ]
    .iter()
    .any(|removed| removed.eq_ignore_ascii_case(name))
}

fn strip_wrapping_quotes(value: &str) -> &str {
    value.trim().trim_matches('"').trim_matches('\'').trim_matches('`')
}

fn strip_tool_namespace_prefix(value: &str) -> &str {
    for prefix in [
        "functions.",
        "function.",
        "tools.",
        "tool.",
        "assistant.",
        "recipient_name.",
    ] {
        if let Some(stripped) = value.strip_prefix(prefix) {
            return stripped;
        }
    }
    value
}

fn push_candidate(candidates: &mut Vec<String>, value: &str) {
    let trimmed = strip_wrapping_quotes(value);
    if trimmed.is_empty() {
        return;
    }

    if !candidates.iter().any(|existing| existing == trimmed) {
        candidates.push(trimmed.to_string());
    }

    let stripped = strip_tool_namespace_prefix(trimmed);
    if stripped != trimmed && !candidates.iter().any(|existing| existing == stripped) {
        candidates.push(stripped.to_string());
    }

    let lowered = stripped.trim().to_ascii_lowercase();
    if !lowered.is_empty() && !candidates.iter().any(|existing| existing == &lowered) {
        candidates.push(lowered.clone());
    }

    let underscored = lowered.replace([' ', '-'], "_");
    if !underscored.is_empty() && !candidates.iter().any(|existing| existing == &underscored) {
        candidates.push(underscored);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::CapabilityLevel;
    use crate::tool_policy::ToolPolicy;
    use crate::tools::registry::ToolRegistration;
    use futures::future::BoxFuture;
    use serde_json::{Value, json};

    fn noop_executor<'a>(
        _registry: &'a crate::tools::registry::ToolRegistry,
        _args: Value,
    ) -> BoxFuture<'a, anyhow::Result<Value>> {
        Box::pin(async move { Ok(json!({"success": true})) })
    }

    #[test]
    fn public_lookup_preserves_first_candidate_precedence_and_permissions() {
        let literal = ToolRegistration::new("literal_tool", CapabilityLevel::Bash, true, noop_executor)
            .with_description("Literal aliases")
            .with_parameter_schema(json!({"type": "object"}))
            .with_permission(ToolPolicy::Prompt)
            .with_aliases([
                "Read action",
                "read_action",
                "functions.fallback_tool",
                "left<|channel|>right",
                "prefix:suffix",
            ]);
        let fallback = ToolRegistration::new("fallback_tool", CapabilityLevel::Bash, true, noop_executor)
            .with_description("Fallback aliases")
            .with_parameter_schema(json!({"type": "object"}))
            .with_permission(ToolPolicy::Deny)
            .with_aliases(["read action", "right", "suffix", r#""literal_tool""#]);
        let assembly = ToolAssembly::from_registrations(vec![literal, fallback]);

        for name in [
            "literal_tool",
            " \tliteral_tool\u{2003}",
            "\"literal_tool\"",
            "'literal_tool'",
            "`literal_tool`",
            r#"`"literal_tool"`"#,
            "Read action",
            "read_action",
            "functions.fallback_tool",
            "left<|channel|>right",
            "prefix:suffix",
        ] {
            let resolution = assembly.resolve_public_tool(name).unwrap();
            assert_eq!(resolution.registration_name(), "literal_tool", "{name}");
            assert_eq!(resolution.default_permission(), &ToolPolicy::Prompt, "{name}");
        }
        for name in [
            "fallback_tool",
            "READ ACTION",
            "functions.FALLBACK_TOOL",
            "unknown<|channel|>right",
            "unknown:suffix",
            "right",
            "suffix",
        ] {
            let resolution = assembly.resolve_public_tool(name).unwrap();
            assert_eq!(resolution.registration_name(), "fallback_tool", "{name}");
            assert_eq!(resolution.default_permission(), &ToolPolicy::Deny, "{name}");
        }
        for name in ["", " \t\u{2003}", "functions.not_registered", "READ ACTİON"] {
            let error = assembly.resolve_public_tool(name).unwrap_err();
            assert!(error.to_string().contains(&format!("Unknown tool: {name}")), "{error}");
        }
    }

    #[test]
    fn removed_public_names_preserve_ascii_case_and_whitespace_boundaries() {
        for name in [
            "command_session_internal",
            "file_operation_internal",
            "search_dispatch_internal",
            "list_files",
            "read_file",
            "write_file",
            "edit_file",
            "create_file",
            "delete_file",
            "move_file",
            "copy_file",
            "search_replace",
            "file_op",
        ] {
            assert!(is_removed_public_tool_name(name), "{name}");
            let padded_uppercase = format!("\u{2003}{}\t", name.to_ascii_uppercase());
            assert!(is_removed_public_tool_name(&padded_uppercase), "{padded_uppercase:?}");
        }
        for name in [
            "",
            "read_file_extra",
            "files_read",
            "'read_file'",
            "read-file",
            "unifiedexec",
            "read_fİle",
            "file_ops",
            "unified_exec",
            "unified_file",
            "unified_search",
        ] {
            assert!(!is_removed_public_tool_name(name), "{name}");
        }
    }

    #[test]
    fn public_lookup_keeps_padded_mixed_case_retired_aliases_hidden() {
        let registration = ToolRegistration::new("visible_tool", CapabilityLevel::Bash, true, noop_executor)
            .with_description("Visible tool with retired aliases")
            .with_parameter_schema(json!({"type": "object"}))
            .with_permission(ToolPolicy::Allow)
            .with_aliases([
                "  READ_FILE  ",
                "Edit_File",
                "file_operation_internal",
                " FILE_OP ",
                "custom_alias",
            ]);
        let assembly = ToolAssembly::from_registrations(vec![registration]);
        for name in [
            "  READ_FILE  ",
            "READ_FILE",
            "read_file",
            "Edit_File",
            "edit_file",
            "file_operation_internal",
            " FILE_OP ",
            "file_op",
        ] {
            assert!(assembly.resolve_public_tool(name).is_err(), "{name}");
        }
        let resolution = assembly.resolve_public_tool("custom_alias").unwrap();
        assert_eq!(resolution.registration_name(), "visible_tool");
        assert_eq!(resolution.default_permission(), &ToolPolicy::Allow);
    }

    #[test]
    fn public_routes_keep_exact_aliases_only() {
        let registration = ToolRegistration::new("visible_tool", CapabilityLevel::Bash, true, noop_executor)
            .with_description("Run commands")
            .with_parameter_schema(json!({"type": "object"}))
            .with_permission(ToolPolicy::Prompt)
            .with_aliases(["visible label", "visible_alias"]);

        let assembly = ToolAssembly::from_registrations(vec![registration]);

        assert_eq!(
            assembly
                .resolve_public_tool("visible label")
                .ok()
                .map(|resolution| resolution.registration_name().to_string()),
            Some("visible_tool".to_string())
        );
        assert_eq!(
            assembly
                .resolve_public_tool("visible_alias")
                .ok()
                .map(|resolution| resolution.registration_name().to_string()),
            Some("visible_tool".to_string())
        );
        assembly.resolve_public_tool("missing_alias").unwrap_err();
    }

    #[test]
    fn public_routes_resolve_canonical_mcp_registration_names() {
        let registration =
            ToolRegistration::new("mcp::time::get_current_time", CapabilityLevel::Bash, true, noop_executor)
                .with_description("Get the current time")
                .with_parameter_schema(json!({"type": "object"}))
                .with_permission(ToolPolicy::Prompt)
                .with_aliases(["mcp__time__get_current_time"]);

        let assembly = ToolAssembly::from_registrations(vec![registration]);

        for requested_name in ["mcp__time__get_current_time", "mcp::time::get_current_time"] {
            let resolution = assembly
                .resolve_public_tool(requested_name)
                .unwrap_or_else(|err| panic!("expected {requested_name} to resolve, got error: {err}"));
            assert_eq!(resolution.registration_name(), "mcp::time::get_current_time");
        }
    }

    #[test]
    fn folded_lifecycle_aliases_resolve_to_their_new_parents() {
        // `wait_agent` / `close_agent` were folded into `agent`
        // (action='wait'/'close'), and `mcp_connect_server` /
        // `mcp_disconnect_server` were folded into `mcp`
        // (action='connect'/'disconnect'). The legacy names must keep
        // resolving through the public-route alias table to their new
        // canonical registration so old callers still work.
        use super::super::builtins::builtin_tool_registrations;

        let registrations = builtin_tool_registrations(None);
        let assembly = ToolAssembly::from_registrations(registrations);

        for (alias, expected_parent) in [
            (tools::WAIT_AGENT, tools::AGENT),
            (tools::CLOSE_AGENT, tools::AGENT),
            (tools::MCP_CONNECT_SERVER, tools::MCP),
            (tools::MCP_DISCONNECT_SERVER, tools::MCP),
        ] {
            let resolved = assembly
                .resolve_public_tool(alias)
                .unwrap_or_else(|err| panic!("expected {alias} to resolve, got error: {err}"));
            assert_eq!(
                resolved.registration_name(),
                expected_parent,
                "expected legacy alias {alias} to resolve to {expected_parent}"
            );
        }

        // The legacy names must not themselves be standalone catalog entries
        // (`entry.public_name` is always the canonical registration name,
        // never one of the folded alias names).
        for legacy_name in [
            tools::WAIT_AGENT,
            tools::CLOSE_AGENT,
            tools::MCP_CONNECT_SERVER,
            tools::MCP_DISCONNECT_SERVER,
        ] {
            assert!(
                assembly
                    .catalog()
                    .entries()
                    .iter()
                    .all(|entry| entry.public_name != legacy_name),
                "expected {legacy_name} to not be a standalone catalog entry"
            );
        }
    }

    #[test]
    fn public_tool_name_candidates_keep_lowercase_human_label() {
        let candidates = public_tool_name_candidates("Exec code");
        assert!(candidates.iter().any(|candidate| candidate == "exec code"));
        assert!(candidates.iter().any(|candidate| candidate == "exec_code"));
    }

    #[test]
    fn public_tool_name_candidates_strip_tool_prefixes() {
        let candidates = public_tool_name_candidates("functions.read_file");
        assert!(candidates.iter().any(|candidate| candidate == "read_file"));
    }

    #[test]
    fn public_routes_reject_removed_file_surface_names_and_aliases() {
        let legacy_registration =
            ToolRegistration::new(tools::UNIFIED_FILE, CapabilityLevel::Editing, true, noop_executor)
                .with_description("Legacy file surface")
                .with_parameter_schema(json!({"type": "object"}))
                .with_permission(ToolPolicy::Prompt)
                .with_aliases([tools::READ_FILE, tools::WRITE_FILE]);
        let custom_registration =
            ToolRegistration::new("custom_tool", CapabilityLevel::CodeSearch, true, noop_executor)
                .with_description("Custom tool")
                .with_parameter_schema(json!({"type": "object"}))
                .with_permission(ToolPolicy::Allow)
                .with_aliases([tools::DELETE_FILE, "custom_alias"]);

        let assembly = ToolAssembly::from_registrations(vec![legacy_registration, custom_registration]);

        for removed_name in [
            tools::UNIFIED_EXEC,
            tools::UNIFIED_FILE,
            tools::UNIFIED_SEARCH,
            tools::LIST_FILES,
            tools::READ_FILE,
            tools::WRITE_FILE,
            tools::DELETE_FILE,
        ] {
            assembly
                .resolve_public_tool(removed_name)
                .expect_err("removed file surface should not resolve publicly");
        }

        assert_eq!(
            assembly
                .resolve_public_tool("custom_alias")
                .ok()
                .map(|resolution| resolution.registration_name().to_string()),
            Some("custom_tool".to_string())
        );
    }
}
