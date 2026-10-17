//! Wire-facing tool-list filtering.
//!
//! Narrows the tool catalog snapshot down to what a given turn is actually
//! allowed to send: primary-agent tool policy, effective permissions, and
//! (for client-local deferred-tool policies) deferred-tool omission from the
//! wire payload.
//!
//! Invariant: deferred tool definitions are omitted from the wire ONLY when
//! the ClientLocal policy is active for the turn (see
//! [`super::snapshot::TurnRequestSnapshot::client_local_tool_deferral`]).
//! Hosted (Anthropic/OpenAI) payloads always keep the full set of deferred
//! tool definitions on the wire; this module must never filter those out.

use std::sync::Arc;

use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::core::agent::harness_kernel::SessionToolCatalogSnapshot;
use vtcode_core::llm::provider::{self as uni};
use vtcode_core::permissions::{build_advertised_permission_requests, evaluate_effective_permissions};
use vtcode_core::{ActivePrimaryAgent, apply_primary_agent_tool_policy};

pub(super) fn uses_out_of_band_copilot_tools(provider_name: &str) -> bool {
    provider_name.eq_ignore_ascii_case(vtcode_core::copilot::COPILOT_PROVIDER_KEY)
}

pub(super) fn apply_primary_agent_policy_to_tool_snapshot(
    snapshot: SessionToolCatalogSnapshot,
    active_primary_agent: &ActivePrimaryAgent,
    workspace: &std::path::Path,
    vt_cfg: Option<&vtcode_core::config::loader::VTCodeConfig>,
) -> SessionToolCatalogSnapshot {
    let filtered = apply_primary_agent_tool_policy(snapshot.snapshot, active_primary_agent);
    let filtered = apply_permission_policy_to_tools(filtered, active_primary_agent, workspace, vt_cfg);
    SessionToolCatalogSnapshot::new(
        snapshot.version,
        snapshot.epoch,
        snapshot.planning_active,
        snapshot.request_user_input_enabled,
        filtered,
        snapshot.cache_hit,
    )
}

/// Filter tools by effective permissions. Tools where ALL advertised permission
/// requests are denied by the agent's permissions are hidden. This mirrors the
/// AgentRunner's `is_tool_exposed` check so both paths agree on what the model
/// sees.
///
/// When the active agent has `PermissionDefault::Auto`, the
/// `automation.full_auto.allowed_tools` config is also enforced so that
/// interactive `auto` matches the `--full-auto` CLI blast radius.
fn apply_permission_policy_to_tools(
    tools: Option<Arc<Vec<vtcode_core::llm::provider::ToolDefinition>>>,
    active_primary_agent: &ActivePrimaryAgent,
    workspace: &std::path::Path,
    vt_cfg: Option<&vtcode_core::config::loader::VTCodeConfig>,
) -> Option<Arc<Vec<vtcode_core::llm::provider::ToolDefinition>>> {
    use std::collections::HashSet;
    use vtcode_config::core::permissions::PermissionDefault;
    use vtcode_core::permissions::ResolvedPermissionDecision;

    let tools = tools?;
    let Some(cfg) = vt_cfg else {
        return Some(tools);
    };
    let current_dir = std::env::current_dir().unwrap_or_else(|_| workspace.to_path_buf());
    let agent_permissions = &active_primary_agent.permissions;

    // When the auto agent is active, enforce the full-auto allow-list from
    // config so interactive auto has the same blast radius as --full-auto.
    // An empty allowlist means no execution tools are allowed (matching CLI
    // behaviour); workflow-coordination tools (`task_tracker`,
    // `start_planning`, `request_user_input`) still bypass the gate below.
    // A wildcard ["*"] means unrestricted.
    let full_auto_allowlist: Option<&[String]> = if agent_permissions.default == PermissionDefault::Auto {
        let allowed = &cfg.automation.full_auto.allowed_tools;
        if cfg.automation.full_auto.enabled && !allowed.iter().any(|t| t == "*") {
            Some(allowed.as_slice())
        } else {
            None
        }
    } else {
        None
    };

    // Config-drift guard: allowlist entries are matched by exact tool name.
    // Flag entries that match nothing in the emitted catalog so a stale
    // `[automation.full_auto] allowed_tools` can never silently gut the
    // model's toolset (the failure this guard addresses produced a catalog
    // that had collapsed to `["web_fetch"]` with no diagnostic anywhere).
    let allowlist_matches_emitted = if let Some(allowlist) = full_auto_allowlist {
        let emitted: HashSet<&str> = tools.iter().map(|tool| tool.function_name()).collect();
        let stale: Vec<&str> = allowlist
            .iter()
            .map(String::as_str)
            .filter(|allowed| !emitted.contains(allowed))
            .collect();
        if !stale.is_empty() {
            tracing::warn!(
                target: "vtcode.tool_shaping",
                stale_allowlist = ?stale,
                allowed_tools = ?allowlist,
                emitted_tools = ?tools.iter().map(|tool| tool.function_name()).collect::<Vec<_>>(),
                "automation.full_auto.allowed_tools references tool names not present in the emitted catalog"
            );
        }
        allowlist.iter().any(|allowed| emitted.contains(allowed.as_str()))
    } else {
        true
    };

    let allows_tool = |tool: &uni::ToolDefinition| {
        let name = tool.function_name();

        // Enforce full-auto allow-list if present. Workflow-coordination
        // tools skip this gate only (they were already subject to the
        // mode/primary-agent filters upstream, so this never injects a
        // tool — it only keeps an already-visible one).
        if let Some(allowlist) = full_auto_allowlist
            && !allowlist.iter().any(|allowed| allowed == name)
            && !tool_names::is_workflow_coordination_tool(name)
        {
            return false;
        }

        let requests = build_advertised_permission_requests(workspace, &current_dir, name);
        if requests.is_empty() {
            return true;
        }
        // Hide the tool only when ALL advertised actions are denied.
        let all_denied = requests.iter().all(|request| {
            evaluate_effective_permissions(&cfg.permissions, agent_permissions, workspace, &current_dir, request)
                == ResolvedPermissionDecision::Deny
        });
        !all_denied
    };
    let filtered: Vec<_> = if let Some(first_rejected) = tools.iter().position(|tool| !allows_tool(tool)) {
        tools
            .iter()
            .take(first_rejected)
            .chain(tools.iter().skip(first_rejected + 1).filter(|tool| allows_tool(tool)))
            .cloned()
            .collect()
    } else if !tools.is_empty() {
        return Some(tools);
    } else {
        Vec::new()
    };

    // Fail loud instead of handing the model a zero-tool catalog. A non-empty
    // full-auto allowlist whose entries match no emitted tool is pure config
    // drift (every entry is stale), so degrade to the unrestricted catalog
    // rather than an unusable empty one. An intentionally empty allowlist
    // ("no tools allowed") and permission-denial collapses are left untouched.
    if filtered.is_empty()
        && let Some(allowlist) = full_auto_allowlist
        && !allowlist.is_empty()
        && !allowlist_matches_emitted
    {
        tracing::warn!(
            target: "vtcode.tool_shaping",
            allowed_tools = ?allowlist,
            "automation.full_auto.allowed_tools matched no emitted tool; falling back to the full catalog"
        );
        return Some(tools);
    }

    (!filtered.is_empty()).then(|| Arc::new(filtered))
}

/// Drops tools with `defer_loading == Some(true)` from the wire-facing tool
/// list. Only ever called when [`super::snapshot::TurnRequestSnapshot::client_local_tool_deferral`]
/// is true, i.e. no provider-hosted tool search is active for this turn --
/// hosted policies (Anthropic/OpenAI) never reach this function and always
/// see the full deferred definitions on the wire, per the safety
/// requirement that their payloads stay byte-identical to today.
///
/// This operates on the already-cloned `Arc` returned by the tool-snapshot
/// pipeline, not on `ctx.tools` or `SessionToolCatalogState`'s caches, so it
/// cannot disturb the local search index or the `note_tool_references`
/// un-defer round trip -- those consumers see the unfiltered list via
/// `TurnRequestBuildResult::runtime_tools`, which stays unfiltered.
pub(super) fn client_local_wire_tools(
    tools: Option<Arc<Vec<uni::ToolDefinition>>>,
) -> Option<Arc<Vec<uni::ToolDefinition>>> {
    let tools = tools?;
    if !tools.iter().any(|tool| tool.defer_loading == Some(true)) {
        return Some(tools);
    }
    Some(Arc::new(tools.iter().filter(|tool| tool.defer_loading != Some(true)).cloned().collect()))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use vtcode_config::builtin_primary_auto_agent;
    use vtcode_core::ActivePrimaryAgent;
    use vtcode_core::config::loader::VTCodeConfig;
    use vtcode_core::llm::provider::ToolDefinition;

    use super::apply_permission_policy_to_tools;

    const WORKSPACE: &str = "/workspace";

    fn tool(name: &str) -> ToolDefinition {
        ToolDefinition::function(name.to_string(), name.to_string(), serde_json::json!({}))
    }

    fn catalog(names: &[&str]) -> Option<Arc<Vec<ToolDefinition>>> {
        Some(Arc::new(names.iter().map(|n| tool(n)).collect()))
    }

    fn names(filtered: Option<Arc<Vec<ToolDefinition>>>) -> Vec<String> {
        let mut out: Vec<String> = filtered
            .map(|tools| tools.iter().map(|t| t.function_name().to_string()).collect())
            .unwrap_or_default();
        out.sort();
        out
    }

    fn auto_agent() -> ActivePrimaryAgent {
        ActivePrimaryAgent::from_spec(&builtin_primary_auto_agent())
    }

    fn cfg_with_full_auto(allowed_tools: Vec<String>) -> VTCodeConfig {
        let mut cfg = VTCodeConfig::default();
        cfg.automation.full_auto.enabled = true;
        cfg.automation.full_auto.allowed_tools = allowed_tools;
        cfg
    }

    #[test]
    fn stale_only_allowlist_collapses_and_falls_back_to_full_catalog() {
        // Every allowlist entry is a legacy tool name that no emitted tool
        // matches: the filter would collapse the catalog to zero. The guard
        // must warn and degrade to the full catalog instead.
        let cfg = cfg_with_full_auto(vec!["read_file".into(), "list_files".into(), "grep_file".into()]);
        let result = apply_permission_policy_to_tools(
            catalog(&["exec_command", "code_search", "web_fetch"]),
            &auto_agent(),
            PathBuf::from(WORKSPACE).as_path(),
            Some(&cfg),
        );
        assert_eq!(names(result), vec!["code_search", "exec_command", "web_fetch"]);
    }

    #[test]
    fn curated_allowlist_keeps_all_matching_tools() {
        // The curated current set from the config fix: every name maps to an
        // emitted tool, so all six survive unchanged.
        let cfg = cfg_with_full_auto(vec![
            "exec_command".into(),
            "write_stdin".into(),
            "apply_patch".into(),
            "code_search".into(),
            "web_fetch".into(),
            "request_user_input".into(),
        ]);
        let original = catalog(&[
            "exec_command",
            "write_stdin",
            "apply_patch",
            "code_search",
            "web_fetch",
            "request_user_input",
        ])
        .expect("catalog");
        let result = apply_permission_policy_to_tools(
            Some(Arc::clone(&original)),
            &auto_agent(),
            PathBuf::from(WORKSPACE).as_path(),
            Some(&cfg),
        );
        assert!(Arc::ptr_eq(result.as_ref().expect("unchanged catalog"), &original));
        assert_eq!(
            names(result),
            vec![
                "apply_patch",
                "code_search",
                "exec_command",
                "request_user_input",
                "web_fetch",
                "write_stdin",
            ]
        );
    }

    #[test]
    fn empty_allowlist_means_no_tools() {
        let cfg = cfg_with_full_auto(vec![]);
        let result = apply_permission_policy_to_tools(
            catalog(&["exec_command", "code_search"]),
            &auto_agent(),
            PathBuf::from(WORKSPACE).as_path(),
            Some(&cfg),
        );
        assert!(result.is_none(), "an empty full-auto allowlist must allow no tools");
    }

    #[test]
    fn wildcard_allowlist_is_unrestricted() {
        let cfg = cfg_with_full_auto(vec!["*".into()]);
        let result = apply_permission_policy_to_tools(
            catalog(&["exec_command", "code_search", "web_fetch"]),
            &auto_agent(),
            PathBuf::from(WORKSPACE).as_path(),
            Some(&cfg),
        );
        assert_eq!(names(result), vec!["code_search", "exec_command", "web_fetch"]);
    }

    #[test]
    fn non_auto_agent_ignores_full_auto_allowlist() {
        // The build agent (Ask default) must not be constrained by the
        // full-auto allowlist even when it is enabled with stale names.
        let cfg = cfg_with_full_auto(vec!["read_file".into(), "list_files".into()]);
        let build = ActivePrimaryAgent::from_spec(&vtcode_config::builtin_primary_build_agent());
        let result = apply_permission_policy_to_tools(
            catalog(&["exec_command", "code_search", "web_fetch"]),
            &build,
            PathBuf::from(WORKSPACE).as_path(),
            Some(&cfg),
        );
        assert_eq!(names(result), vec!["code_search", "exec_command", "web_fetch"]);
    }

    #[test]
    fn full_auto_disabled_ignores_allowlist() {
        let mut cfg = cfg_with_full_auto(vec!["read_file".into(), "list_files".into()]);
        cfg.automation.full_auto.enabled = false;
        let result = apply_permission_policy_to_tools(
            catalog(&["exec_command", "code_search", "web_fetch"]),
            &auto_agent(),
            PathBuf::from(WORKSPACE).as_path(),
            Some(&cfg),
        );
        assert_eq!(names(result), vec!["code_search", "exec_command", "web_fetch"]);
    }

    #[test]
    fn workflow_coordination_tools_survive_allowlist_that_omits_them() {
        // Regression guard: `task_tracker` was stripped from the Auto-mode
        // wire catalog because the full-auto allow-list omitted it, while a
        // non-workflow tool outside the list must still be filtered.
        let cfg = cfg_with_full_auto(vec!["exec_command".into()]);
        let result = apply_permission_policy_to_tools(
            catalog(&[
                "exec_command",
                "code_search",
                "task_tracker",
                "start_planning",
                "request_user_input",
            ]),
            &auto_agent(),
            PathBuf::from(WORKSPACE).as_path(),
            Some(&cfg),
        );
        assert_eq!(names(result), vec!["exec_command", "request_user_input", "start_planning", "task_tracker"]);
    }

    #[test]
    fn empty_allowlist_still_exposes_workflow_coordination_tools() {
        // Workflow coordination is control-plane, not execution blast radius:
        // even an intentionally empty allow-list keeps the tracker visible so
        // the model can report progress instead of going silent.
        let cfg = cfg_with_full_auto(vec![]);
        let result = apply_permission_policy_to_tools(
            catalog(&["exec_command", "task_tracker"]),
            &auto_agent(),
            PathBuf::from(WORKSPACE).as_path(),
            Some(&cfg),
        );
        assert_eq!(names(result), vec!["task_tracker"]);
    }

    #[test]
    fn permission_filter_preserves_order_and_the_original_catalog() {
        let mut cfg = cfg_with_full_auto(vec!["*".into()]);
        cfg.permissions.deny = vec!["Edit".into()];
        let original = catalog(&["web_fetch", "apply_patch", "code_search", "exec_command"]).expect("catalog");
        let result = apply_permission_policy_to_tools(
            Some(Arc::clone(&original)),
            &auto_agent(),
            PathBuf::from(WORKSPACE).as_path(),
            Some(&cfg),
        )
        .expect("surviving tools");

        assert!(!Arc::ptr_eq(&result, &original));
        assert_eq!(
            result.iter().map(ToolDefinition::function_name).collect::<Vec<_>>(),
            ["web_fetch", "code_search", "exec_command"]
        );
        assert_eq!(
            original.iter().map(ToolDefinition::function_name).collect::<Vec<_>>(),
            ["web_fetch", "apply_patch", "code_search", "exec_command"]
        );
    }

    #[test]
    fn empty_catalog_preserves_config_and_stale_allowlist_behavior() {
        let workspace = PathBuf::from(WORKSPACE);
        let agent = auto_agent();
        let empty = Arc::new(Vec::new());
        let no_config = apply_permission_policy_to_tools(Some(Arc::clone(&empty)), &agent, &workspace, None)
            .expect("no-config passthrough");
        assert!(Arc::ptr_eq(&no_config, &empty));
        assert!(
            apply_permission_policy_to_tools(
                Some(Arc::clone(&empty)),
                &agent,
                &workspace,
                Some(&VTCodeConfig::default())
            )
            .is_none()
        );

        let cfg = cfg_with_full_auto(vec!["stale_tool".into()]);
        let stale_fallback = apply_permission_policy_to_tools(Some(Arc::clone(&empty)), &agent, &workspace, Some(&cfg))
            .expect("stale allowlist fallback");
        assert!(Arc::ptr_eq(&stale_fallback, &empty));
        assert!(apply_permission_policy_to_tools(None, &agent, &workspace, Some(&cfg)).is_none());
    }
}
