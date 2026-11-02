//! Reuse CLI capability validation and session startup flags for native controls.
use anyhow::{Context, Result, bail};
use std::path::PathBuf;
use vtcode_core::core::interfaces::session::PlanningEntrySource;
use vtcode_desktop_protocol::{PermissionMode, SessionOptionsSelectParams};

pub(super) fn supported_efforts(provider: &str, model: &str) -> Vec<String> {
    vtcode_config::models::model_catalog_entry(provider, model)
        .map(|entry| entry.reasoning_efforts.iter().map(|effort| (*effort).to_owned()).collect())
        .unwrap_or_default()
}

pub(super) fn validate(
    params: &SessionOptionsSelectParams,
    provider: &str,
    model: &str,
    fast_mode_supported: bool,
    modes: &[vtcode_config::SubagentSpec],
) -> Result<()> {
    if params
        .mode
        .as_deref()
        .is_some_and(|mode| !modes.iter().any(|spec| spec.is_primary() && spec.name == mode))
    {
        bail!("unsupported desktop agent mode");
    }
    if let Some(effort) = params.reasoning_effort.as_deref()
        && (vtcode_commons::reasoning::ReasoningEffortLevel::parse(effort).is_none()
            || !supported_efforts(provider, model).iter().any(|supported| supported == effort))
    {
        bail!("reasoning effort is not supported by the selected provider/model");
    }
    if params.fast_mode.is_some() && !fast_mode_supported {
        bail!("fast processing is not supported by the selected provider/model route");
    }
    // The confirmation dialog is client UI; the attestation is enforced here
    // so a request that skips the dialog cannot enable Full access.
    if params.permission_mode == Some(PermissionMode::FullAccess) && params.full_access_confirmed != Some(true) {
        bail!("confirm the Full access risk disclosure before enabling Full access");
    }
    if params == &SessionOptionsSelectParams::default() {
        bail!("choose a runtime option to change");
    }
    Ok(())
}

pub(super) async fn discover_modes(workspace: PathBuf) -> Result<Vec<vtcode_config::SubagentSpec>> {
    tokio::task::spawn_blocking(move || {
        let discovered = vtcode_config::discover_subagents(&vtcode_config::SubagentDiscoveryInput::new(workspace))
            .context("failed to discover CLI primary agents")?;
        let modes = discovered
            .effective
            .into_iter()
            .filter(vtcode_config::SubagentSpec::is_primary)
            .collect::<Vec<_>>();
        if modes.len() > 256 {
            bail!("too many primary agents for the desktop mode catalog");
        }
        Ok(modes)
    })
    .await
    .context("primary-agent discovery worker failed")?
}

pub(super) fn merge(current: &mut SessionOptionsSelectParams, next: SessionOptionsSelectParams) {
    if next.permission_mode.is_some() {
        current.permission_mode = next.permission_mode;
    }
    if next.mode.is_some() {
        current.mode = next.mode;
    }
    if next.reasoning_effort.is_some() {
        current.reasoning_effort = next.reasoning_effort;
    }
    if next.fast_mode.is_some() {
        current.fast_mode = next.fast_mode;
    }
}

pub(super) fn skip_confirmations(params: &SessionOptionsSelectParams, startup: bool) -> bool {
    params
        .permission_mode
        .map_or(startup, |mode| matches!(mode, PermissionMode::AutoApproveCommands | PermissionMode::FullAccess))
}

pub(super) fn planning_source(
    params: &SessionOptionsSelectParams,
    startup: PlanningEntrySource,
) -> PlanningEntrySource {
    match params.mode.as_deref() {
        Some("plan") => PlanningEntrySource::AgentSelection,
        Some("build") => PlanningEntrySource::None,
        _ => startup,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn desktop_modes_discover_cli_primary_agents_and_validate_custom_choices() {
        let directory = tempfile::tempdir().expect("workspace");
        let agents = directory.path().join(".vtcode/agents");
        std::fs::create_dir_all(&agents).expect("agent directory");
        std::fs::write(agents.join("custom.md"), "---\nname: custom-reviewer\ndescription: Workspace review\nmode: primary\npermissions:\n  default: ask\n---\nReview changes.").expect("primary agent");
        let modes = discover_modes(directory.path().to_path_buf()).await.expect("CLI discovery");
        for name in ["build", "plan", "auto", "duck", "coordinator", "custom-reviewer"] {
            assert!(modes.iter().any(|spec| spec.name == name), "missing mode {name}");
            let options = SessionOptionsSelectParams { mode: Some(name.into()), ..Default::default() };
            assert!(validate(&options, "unknown", "unknown", false, &modes).is_ok());
        }
        assert!(!modes.iter().any(|spec| spec.name == "explorer"), "delegated agents are not primary modes");
    }
    #[test]
    fn desktop_options_reject_unsupported_values_and_preserve_unselected_options() {
        let mut options = SessionOptionsSelectParams {
            permission_mode: Some(PermissionMode::AutoApproveCommands),
            mode: Some("plan".into()),
            reasoning_effort: None,
            fast_mode: None,
            full_access_confirmed: None,
        };
        assert!(skip_confirmations(&options, false));
        let full = SessionOptionsSelectParams {
            permission_mode: Some(PermissionMode::FullAccess),
            ..Default::default()
        };
        assert!(skip_confirmations(&full, false));
        assert_eq!(planning_source(&options, PlanningEntrySource::None), PlanningEntrySource::AgentSelection);
        merge(
            &mut options,
            SessionOptionsSelectParams {
                permission_mode: Some(PermissionMode::Ask),
                ..Default::default()
            },
        );
        assert!(!skip_confirmations(&options, true));
        assert_eq!(options.mode.as_deref(), Some("plan"));
        assert!(validate(&options, "unknown", "unknown", false, &vtcode_config::builtin_subagents()).is_ok());
        options.reasoning_effort = Some("high".into());
        assert!(validate(&options, "unknown", "unknown", false, &vtcode_config::builtin_subagents()).is_err());
        options.reasoning_effort = None;
        options.mode = Some("build".into());
        assert_eq!(planning_source(&options, PlanningEntrySource::CliFlag), PlanningEntrySource::None);
        options.mode = Some("arbitrary-agent".into());
        assert!(validate(&options, "openai", "gpt-5", false, &vtcode_config::builtin_subagents()).is_err());
        assert!(
            validate(
                &SessionOptionsSelectParams::default(),
                "openai",
                "gpt-5",
                false,
                &vtcode_config::builtin_subagents()
            )
            .is_err()
        );
        let effort = SessionOptionsSelectParams {
            reasoning_effort: Some("high".into()),
            ..Default::default()
        };
        assert!(validate(&effort, "openai", "gpt-5.6-sol", false, &vtcode_config::builtin_subagents()).is_ok());
        let invalid = SessionOptionsSelectParams {
            reasoning_effort: Some("maximum".into()),
            ..Default::default()
        };
        assert!(validate(&invalid, "openai", "gpt-5.6-sol", false, &vtcode_config::builtin_subagents()).is_err());
        let fast = SessionOptionsSelectParams { fast_mode: Some(true), ..Default::default() };
        assert!(validate(&fast, "openai", "gpt-6.1-sol", false, &vtcode_config::builtin_subagents()).is_err());
        assert!(validate(&fast, "openai", "gpt-6.1-sol", true, &vtcode_config::builtin_subagents()).is_ok());
    }

    #[test]
    fn desktop_options_require_confirmation_attestation_for_full_access() {
        let modes = vtcode_config::builtin_subagents();
        let unattested = SessionOptionsSelectParams {
            permission_mode: Some(PermissionMode::FullAccess),
            ..Default::default()
        };
        assert!(validate(&unattested, "openai", "gpt-5", false, &modes).is_err());
        let declined = SessionOptionsSelectParams {
            permission_mode: Some(PermissionMode::FullAccess),
            full_access_confirmed: Some(false),
            ..Default::default()
        };
        assert!(validate(&declined, "openai", "gpt-5", false, &modes).is_err());
        let attested = SessionOptionsSelectParams {
            permission_mode: Some(PermissionMode::FullAccess),
            full_access_confirmed: Some(true),
            ..Default::default()
        };
        assert!(validate(&attested, "openai", "gpt-5", false, &modes).is_ok());
        // The attestation is one-shot: merges never carry it into stored options.
        let mut stored = SessionOptionsSelectParams::default();
        merge(&mut stored, attested);
        assert_eq!(stored.permission_mode, Some(PermissionMode::FullAccess));
        assert_eq!(stored.full_access_confirmed, None);
        // Other modes are unaffected by the gate.
        let ask = SessionOptionsSelectParams {
            permission_mode: Some(PermissionMode::Ask),
            ..Default::default()
        };
        assert!(validate(&ask, "openai", "gpt-5", false, &modes).is_ok());
    }
}
