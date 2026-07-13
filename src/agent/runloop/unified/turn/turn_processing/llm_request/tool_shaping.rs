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

use vtcode_core::core::agent::harness_kernel::SessionToolCatalogSnapshot;
use vtcode_core::llm::provider::{self as uni};
use vtcode_core::permissions::{
    build_advertised_permission_requests, evaluate_effective_permissions,
};
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
    let filtered =
        apply_permission_policy_to_tools(filtered, active_primary_agent, workspace, vt_cfg);
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
    // An empty allowlist means no tools are allowed (matching CLI behaviour);
    // a wildcard ["*"] means unrestricted.
    let full_auto_allowlist: Option<&[String]> =
        if agent_permissions.default == PermissionDefault::Auto {
            let allowed = &cfg.automation.full_auto.allowed_tools;
            if cfg.automation.full_auto.enabled && !allowed.iter().any(|t| t == "*") {
                Some(allowed.as_slice())
            } else {
                None
            }
        } else {
            None
        };

    let filtered: Vec<_> = tools
        .iter()
        .filter(|tool| {
            let name = tool.function_name();

            // Enforce full-auto allow-list if present.
            if let Some(allowlist) = full_auto_allowlist
                && !allowlist.iter().any(|allowed| allowed == name)
            {
                return false;
            }

            let requests = build_advertised_permission_requests(workspace, &current_dir, name);
            if requests.is_empty() {
                return true;
            }
            // Hide the tool only when ALL advertised actions are denied.
            let all_denied = requests.iter().all(|request| {
                evaluate_effective_permissions(
                    &cfg.permissions,
                    agent_permissions,
                    workspace,
                    &current_dir,
                    request,
                ) == ResolvedPermissionDecision::Deny
            });
            !all_denied
        })
        .cloned()
        .collect();

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
    Some(Arc::new(
        tools
            .iter()
            .filter(|tool| tool.defer_loading != Some(true))
            .cloned()
            .collect(),
    ))
}
