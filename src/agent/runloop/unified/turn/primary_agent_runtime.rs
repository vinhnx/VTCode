//! Shared runtime synchronization for primary-agent handoffs.
//!
//! Agent selection is used by both the inline interaction loop and the
//! plan-approval turn loop. Keeping hook, MCP, and discovery updates behind
//! this boundary prevents one handoff path from silently drifting from the
//! other.

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use vtcode_config::{SubagentSpec, builtin_subagents};
use vtcode_core::config::types::AgentConfig;
use vtcode_core::hooks::{LifecycleHookEngine, SessionStartTrigger};
use vtcode_core::llm::provider as uni;
use vtcode_core::notifications::set_global_notification_hook_engine;
use vtcode_core::primary_agent::{
    ActivePrimaryAgent, build_primary_agent_hook_config, build_primary_agent_runtime_config,
};
use vtcode_core::tools::registry::ToolRegistry;

use crate::agent::runloop::unified::async_mcp_manager::AsyncMcpManager;
use crate::agent::runloop::unified::session_setup::{active_deferred_tool_policy, refresh_tool_snapshot};
use crate::agent::runloop::unified::tool_catalog::ToolCatalogState;
use vtcode_core::config::loader::VTCodeConfig;

pub(crate) struct PrimaryAgentRuntimeSyncContext<'a> {
    pub(crate) config: &'a AgentConfig,
    pub(crate) vt_cfg: Option<&'a VTCodeConfig>,
    pub(crate) thread_id: &'a str,
    pub(crate) active_primary_agent: &'a ActivePrimaryAgent,
    pub(crate) lifecycle_hooks: &'a mut Option<LifecycleHookEngine>,
    pub(crate) async_mcp_manager: Option<&'a Arc<AsyncMcpManager>>,
    pub(crate) tool_registry: &'a mut ToolRegistry,
    pub(crate) tools: &'a Arc<RwLock<Vec<uni::ToolDefinition>>>,
    pub(crate) tool_catalog: &'a ToolCatalogState,
    pub(crate) mcp_catalog_initialized: &'a mut bool,
    pub(crate) pending_mcp_refresh: &'a mut bool,
    pub(crate) provider_client: &'a dyn uni::LLMProvider,
}

/// Keep the runtime permission snapshot aligned with the selected primary
/// agent. The snapshot is consulted before the active-agent fallback, so a
/// plan-to-build handoff must replace permissions inherited from the planning
/// agent or a prior full-auto runtime.
pub(crate) fn sync_primary_agent_permissions(
    vt_cfg: &mut Option<VTCodeConfig>,
    active_primary_agent: &ActivePrimaryAgent,
) {
    if let Some(cfg) = vt_cfg {
        cfg.runtime_agent_permissions = Some(active_primary_agent.permissions.clone());
    }
}

pub(crate) async fn sync_primary_agent_runtime(ctx: &mut PrimaryAgentRuntimeSyncContext<'_>) -> Result<()> {
    ctx.tool_registry
        .set_matrix_coordinator(ctx.active_primary_agent.name() == "coordinator");
    let Some(cfg) = ctx.vt_cfg else {
        *ctx.lifecycle_hooks = None;
        set_global_notification_hook_engine(None);
        return Ok(());
    };

    let transcript_path = match ctx.lifecycle_hooks.as_ref() {
        Some(hooks) => hooks.transcript_path().await,
        None => None,
    };
    let hooks_config = build_primary_agent_hook_config(&cfg.hooks, ctx.active_primary_agent);
    let workspace_gated = cfg.workspace_lifecycle_hooks.as_ref().is_some_and(|hooks| !hooks.is_empty())
        || ctx.active_primary_agent.contributes_workspace_controlled_hooks();
    let next = LifecycleHookEngine::new_with_session_gated(
        ctx.config.workspace.clone(),
        &hooks_config,
        SessionStartTrigger::Startup,
        ctx.thread_id,
        workspace_gated,
    )?;
    if let (Some(hooks), Some(path)) = (next.as_ref(), transcript_path) {
        hooks.update_transcript_path(Some(path)).await;
    }
    if let Some(hooks) = next.as_ref() {
        // A config reload or primary-agent switch rebuilds the engine. Carry an
        // in-memory (possibly session-only) approval forward when the command
        // set is unchanged, otherwise restore the persisted record; any
        // mismatch leaves the rebuilt engine fail-closed.
        vtcode_core::hooks::carry_or_restore_workspace_hook_approval(
            ctx.lifecycle_hooks.as_ref(),
            hooks,
            &ctx.config.workspace,
        )
        .await;
    }
    set_global_notification_hook_engine(next.clone());
    *ctx.lifecycle_hooks = next;

    if let Some(manager) = ctx.async_mcp_manager.filter(|_| cfg.mcp.enabled) {
        let merged_mcp = build_primary_agent_runtime_config(cfg, ctx.active_primary_agent).mcp;
        let restarted_mcp_runtime = manager.reconfigure_active_runtime(merged_mcp).await?;
        ctx.tool_registry.clear_mcp_client().await;
        *ctx.mcp_catalog_initialized = false;
        *ctx.pending_mcp_refresh = true;

        let deferred_tool_policy = active_deferred_tool_policy(ctx.config, Some(cfg), ctx.provider_client);
        refresh_tool_snapshot(
            ctx.tool_registry,
            ctx.tools,
            ctx.tool_catalog,
            ctx.config,
            Some(cfg),
            cfg.agent.tool_documentation_mode,
            &deferred_tool_policy,
        )
        .await;
        ctx.tool_catalog.mark_pending_refresh("primary_agent_mcp_reconfigure");

        if restarted_mcp_runtime {
            tracing::debug!("Restarted active MCP runtime after primary agent switch");
        }
    }

    Ok(())
}

pub(crate) async fn load_primary_agent_specs(
    tool_registry: &ToolRegistry,
    workspace: &Path,
) -> Result<Vec<SubagentSpec>> {
    if let Some(controller) = tool_registry.subagent_controller() {
        let specs = controller
            .effective_specs()
            .await
            .into_iter()
            .filter(SubagentSpec::is_primary)
            .collect::<Vec<_>>();
        if !specs.is_empty() {
            return Ok(specs);
        }
    }

    let discovered =
        vtcode_config::discover_subagents(&vtcode_config::SubagentDiscoveryInput::new(workspace.to_path_buf()))
            .with_context(|| format!("Failed to discover primary agents in {}", workspace.display()))?;
    Ok(discovered.effective.into_iter().filter(SubagentSpec::is_primary).collect())
}

pub(crate) fn builtin_primary_agent_specs() -> Vec<SubagentSpec> {
    builtin_subagents().into_iter().filter(SubagentSpec::is_primary).collect()
}

/// Resolve the agent that is allowed to execute an approved plan.
///
/// Planning can begin while a read-only primary agent is active, and the
/// configured default may itself be read-only. Approval is a hard boundary:
/// it must never hand an implementation directive to an agent whose spec
/// cannot expose mutation tools. Prefer the requested/prior agent when it is
/// write-capable, then the configured default, then the built-in execution
/// agents, and finally any other write-capable primary agent.
///
/// The `plan` agent is excluded by name, not merely by its permission
/// heuristic: it permits read-only `bash` so `exec_command` stays on the
/// wire while planning, which makes `is_read_only()` false — but selecting
/// it always re-enters the planning workflow, so it can never execute an
/// approved plan regardless of list ordering.
pub(crate) fn resolve_approved_plan_execution_agent(
    requested_agent: Option<&str>,
    configured_default_agent: Option<&str>,
    specs: &[SubagentSpec],
) -> Option<String> {
    let execution_capable =
        |spec: &SubagentSpec| spec.is_primary() && !spec.is_read_only() && !spec.name.eq_ignore_ascii_case("plan");
    for candidate in [requested_agent, configured_default_agent, Some("build"), Some("auto")]
        .into_iter()
        .flatten()
    {
        if let Some(spec) = specs.iter().find(|spec| {
            execution_capable(spec)
                && (spec.name.eq_ignore_ascii_case(candidate)
                    || spec.aliases.iter().any(|alias| alias.eq_ignore_ascii_case(candidate)))
        }) {
            return Some(spec.name.clone());
        }
    }

    specs.iter().find(|spec| execution_capable(spec)).map(|spec| spec.name.clone())
}

#[cfg(test)]
mod tests {
    use super::{resolve_approved_plan_execution_agent, sync_primary_agent_permissions};
    use vtcode_config::core::permissions::{AgentPermissionsConfig, PermissionDefault};
    use vtcode_config::{builtin_primary_auto_agent, builtin_primary_build_agent, builtin_primary_duck_agent};
    use vtcode_core::config::loader::VTCodeConfig;
    use vtcode_core::primary_agent::ActivePrimaryAgent;

    #[test]
    fn read_only_requested_agent_falls_back_to_build() {
        let specs = vec![builtin_primary_duck_agent(), builtin_primary_build_agent()];

        assert_eq!(
            resolve_approved_plan_execution_agent(Some("duck"), Some("duck"), &specs),
            Some("build".to_string())
        );
    }

    #[test]
    fn write_capable_requested_agent_is_preserved() {
        let specs = vec![builtin_primary_auto_agent(), builtin_primary_build_agent()];

        assert_eq!(
            resolve_approved_plan_execution_agent(Some("auto"), Some("build"), &specs),
            Some("auto".to_string())
        );
    }

    #[test]
    fn all_read_only_agents_produce_no_execution_handoff() {
        let specs = vec![builtin_primary_duck_agent()];

        assert_eq!(resolve_approved_plan_execution_agent(Some("duck"), None, &specs), None);
    }

    #[test]
    fn plan_agent_is_never_an_execution_handoff_target() {
        // The plan agent permits read-only bash (so exec_command stays on the
        // wire while planning), which flips `is_read_only()` — but selecting
        // it re-enters the planning workflow, so it must never receive an
        // approved-plan implementation directive, even when it is the
        // requested agent or the only apparent fallback.
        let specs = vec![vtcode_config::builtin_plan_agent(), builtin_primary_build_agent()];
        assert_eq!(
            resolve_approved_plan_execution_agent(Some("plan"), Some("plan"), &specs),
            Some("build".to_string())
        );

        let plan_only = vec![vtcode_config::builtin_plan_agent()];
        assert_eq!(resolve_approved_plan_execution_agent(Some("plan"), None, &plan_only), None);
    }

    #[test]
    fn plan_handoff_replaces_stale_runtime_permissions() {
        let mut vt_cfg = Some(VTCodeConfig {
            runtime_agent_permissions: Some(AgentPermissionsConfig::new(PermissionDefault::Deny)),
            ..VTCodeConfig::default()
        });
        let build = ActivePrimaryAgent::from_spec(&builtin_primary_build_agent());

        sync_primary_agent_permissions(&mut vt_cfg, &build);

        assert_eq!(
            vt_cfg.expect("runtime config should remain present").runtime_agent_permissions,
            Some(build.permissions)
        );
    }
}
