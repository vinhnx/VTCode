use anyhow::{Context, Result};
use std::path::Path;
use vtcode_config::{SubagentDiscoveryInput, SubagentSpec, discover_subagents};
use vtcode_core::config::VTCodeConfig;
use vtcode_core::{ActivePrimaryAgent, build_primary_agent_runtime_config};

use crate::agent::runloop::unified::session_setup::active_primary_agent_from_specs_for_mode;

#[derive(Debug)]
pub(crate) struct FullAutoPrimaryAgentRuntime {
    pub(crate) vt_cfg: VTCodeConfig,
    pub(crate) active_primary_agent: ActivePrimaryAgent,
}

pub(crate) fn resolve_full_auto_primary_agent_runtime(
    workspace: &Path,
    vt_cfg: &VTCodeConfig,
    primary_agent_explicitly_configured: bool,
) -> Result<FullAutoPrimaryAgentRuntime> {
    let discovered = discover_subagents(&SubagentDiscoveryInput::new(workspace.to_path_buf()))
        .with_context(|| {
            format!(
                "Failed to discover primary agents in {}",
                workspace.display()
            )
        })?;

    resolve_full_auto_primary_agent_runtime_from_specs(
        &discovered.effective,
        vt_cfg,
        primary_agent_explicitly_configured,
    )
}

fn resolve_full_auto_primary_agent_runtime_from_specs(
    specs: &[SubagentSpec],
    vt_cfg: &VTCodeConfig,
    primary_agent_explicitly_configured: bool,
) -> Result<FullAutoPrimaryAgentRuntime> {
    let active = active_primary_agent_from_specs_for_mode(
        specs,
        Some(vt_cfg),
        true,
        primary_agent_explicitly_configured,
    )?;
    let active_primary_agent = active.active().clone();
    let mut runtime_vt_cfg = build_primary_agent_runtime_config(vt_cfg, &active_primary_agent);
    runtime_vt_cfg.runtime_agent_permissions = Some(active_primary_agent.permissions.clone());

    Ok(FullAutoPrimaryAgentRuntime {
        vt_cfg: runtime_vt_cfg,
        active_primary_agent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_config::{
        builtin_primary_auto_agent, builtin_primary_build_agent,
        core::permissions::{AgentPermissionsConfig, PermissionDefault},
    };
    use vtcode_core::config::types::ReasoningEffortLevel;

    #[test]
    fn defaulted_full_auto_uses_effective_custom_auto() {
        let mut auto = builtin_primary_auto_agent();
        auto.prompt = "Custom auto instructions".to_string();
        auto.model = Some("gpt-5".to_string());
        auto.reasoning_effort = Some("high".to_string());
        auto.permissions = AgentPermissionsConfig::new(PermissionDefault::Deny);

        let resolved = resolve_full_auto_primary_agent_runtime_from_specs(
            &[auto],
            &VTCodeConfig::default(),
            false,
        )
        .expect("defaulted auto should resolve");

        assert_eq!(resolved.active_primary_agent.identity.name, "auto");
        assert_eq!(
            resolved.active_primary_agent.instructions,
            "Custom auto instructions"
        );
        assert_eq!(resolved.vt_cfg.agent.default_model, "gpt-5");
        assert_eq!(
            resolved.vt_cfg.agent.reasoning_effort,
            ReasoningEffortLevel::High
        );
        assert_eq!(
            resolved
                .vt_cfg
                .runtime_agent_permissions
                .as_ref()
                .expect("agent permissions")
                .default,
            PermissionDefault::Deny
        );
    }

    #[test]
    fn explicit_duck_is_honoured_for_full_auto() {
        let cfg = VTCodeConfig {
            default_primary_agent: "duck".to_string(),
            ..VTCodeConfig::default()
        };

        let resolved = resolve_full_auto_primary_agent_runtime_from_specs(
            &[builtin_primary_auto_agent()],
            &cfg,
            true,
        )
        .expect("explicit duck should resolve");

        assert_eq!(resolved.active_primary_agent.identity.name, "duck");
        assert_eq!(
            resolved
                .vt_cfg
                .runtime_agent_permissions
                .as_ref()
                .expect("agent permissions")
                .default,
            PermissionDefault::Deny
        );
    }

    #[test]
    fn missing_defaulted_auto_fails_fast() {
        let err = resolve_full_auto_primary_agent_runtime_from_specs(
            &[builtin_primary_build_agent()],
            &VTCodeConfig::default(),
            false,
        )
        .expect_err("missing auto should fail");

        assert!(
            err.to_string()
                .contains("no effective primary agent named 'auto' was discovered")
        );
    }
}
