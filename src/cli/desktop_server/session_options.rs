//! Reuse CLI capability validation and session startup flags for native controls.
use anyhow::{Result, bail};
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
) -> Result<()> {
    if params.mode.as_deref().is_some_and(|mode| !matches!(mode, "build" | "plan")) {
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
    if params == &SessionOptionsSelectParams::default() {
        bail!("choose a runtime option to change");
    }
    Ok(())
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
        .map_or(startup, |mode| mode == PermissionMode::AutoApproveCommands)
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
    #[test]
    fn desktop_options_reject_unsupported_values_and_preserve_unselected_options() {
        let mut options = SessionOptionsSelectParams {
            permission_mode: Some(PermissionMode::AutoApproveCommands),
            mode: Some("plan".into()),
            reasoning_effort: None,
            fast_mode: None,
        };
        assert!(skip_confirmations(&options, false));
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
        assert!(validate(&options, "unknown", "unknown", false).is_ok());
        options.reasoning_effort = Some("high".into());
        assert!(validate(&options, "unknown", "unknown", false).is_err());
        options.reasoning_effort = None;
        options.mode = Some("build".into());
        assert_eq!(planning_source(&options, PlanningEntrySource::CliFlag), PlanningEntrySource::None);
        options.mode = Some("arbitrary-agent".into());
        assert!(validate(&options, "openai", "gpt-5", false).is_err());
        assert!(validate(&SessionOptionsSelectParams::default(), "openai", "gpt-5", false).is_err());
        let effort = SessionOptionsSelectParams {
            reasoning_effort: Some("high".into()),
            ..Default::default()
        };
        assert!(validate(&effort, "openai", "gpt-5.6-sol", false).is_ok());
        let invalid = SessionOptionsSelectParams {
            reasoning_effort: Some("maximum".into()),
            ..Default::default()
        };
        assert!(validate(&invalid, "openai", "gpt-5.6-sol", false).is_err());
        let fast = SessionOptionsSelectParams { fast_mode: Some(true), ..Default::default() };
        assert!(validate(&fast, "openai", "gpt-6.1-sol", false).is_err());
        assert!(validate(&fast, "openai", "gpt-6.1-sol", true).is_ok());
    }
}
