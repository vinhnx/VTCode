//! Session mode (primary-agent) resolution and persistence policy.
//!
//! This module isolates the rules that decide which primary agent ("mode") is
//! active for a session, independent of the large `session_setup` orchestration
//! flow. It is pure and unit-testable: given discovered specs, config, and an
//! optional resumed mode, it returns the effective [`ActivePrimaryAgentState`].
//!
//! Resolution precedence (highest to lowest):
//! 1. Full-auto without an explicit config agent -> the `auto` agent.
//! 2. An explicitly configured `default_primary_agent` -> that agent.
//! 3. A resumed session's own `primary_agent` -> that agent (user preference).
//! 4. The config `default_primary_agent`, else the built-in `build` agent.

use anyhow::{Result, bail};
use vtcode_config::{SubagentSpec, VTCodeConfig};
use vtcode_core::{ActivePrimaryAgentState, constants::defaults::DEFAULT_PRIMARY_AGENT_NAME};

/// Inputs to [`resolve_session_primary_agent`].
///
/// Construct with [`SessionModeInput::new`] and refine with the `with_*`
/// setters. The builder is a parse-don't-validate boundary: the resolver
/// consumes the whole request rather than N positional arguments.
pub(crate) struct SessionModeInput<'a> {
    specs: &'a [SubagentSpec],
    vt_cfg: Option<&'a VTCodeConfig>,
    full_auto: bool,
    primary_agent_explicitly_configured: bool,
    resumed_primary_agent: Option<String>,
}

impl<'a> SessionModeInput<'a> {
    #[must_use]
    pub fn new(specs: &'a [SubagentSpec]) -> Self {
        Self {
            specs,
            vt_cfg: None,
            full_auto: false,
            primary_agent_explicitly_configured: false,
            resumed_primary_agent: None,
        }
    }

    #[must_use]
    pub fn with_config(mut self, vt_cfg: Option<&'a VTCodeConfig>) -> Self {
        self.vt_cfg = vt_cfg;
        self
    }

    #[must_use]
    pub fn with_full_auto(mut self, full_auto: bool) -> Self {
        self.full_auto = full_auto;
        self
    }

    #[must_use]
    pub fn with_explicit_config(mut self, explicit: bool) -> Self {
        self.primary_agent_explicitly_configured = explicit;
        self
    }

    #[must_use]
    pub fn with_resumed(mut self, resumed: Option<String>) -> Self {
        self.resumed_primary_agent = resumed;
        self
    }
}

/// Resolves the active primary agent ("mode") for a session.
pub(crate) fn resolve_session_primary_agent(
    input: SessionModeInput<'_>,
) -> Result<ActivePrimaryAgentState> {
    let SessionModeInput {
        specs,
        vt_cfg,
        full_auto,
        primary_agent_explicitly_configured,
        resumed_primary_agent,
    } = input;

    if full_auto && !primary_agent_explicitly_configured {
        let mut active = ActivePrimaryAgentState::default();
        if active.select_from_specs(specs, "auto").is_err() {
            bail!(
                "Full-auto needs the defaulted 'auto' primary agent, but no effective primary agent named 'auto' was discovered. Configure default_primary_agent explicitly or add an 'auto' primary agent."
            );
        }
        return Ok(active);
    }

    // A resumed session's own mode wins over the config default (per user
    // preference), unless the user explicitly configured a primary agent.
    if !primary_agent_explicitly_configured {
        if let Some(resumed) = resumed_primary_agent.filter(|name| !name.trim().is_empty()) {
            return Ok(ActivePrimaryAgentState::from_specs_with_default(
                specs,
                resumed.trim(),
            ));
        }
    }

    let default_primary_agent = vt_cfg
        .map(|cfg| cfg.default_primary_agent.as_str())
        .unwrap_or(DEFAULT_PRIMARY_AGENT_NAME);
    Ok(ActivePrimaryAgentState::from_specs_with_default(
        specs,
        default_primary_agent,
    ))
}

/// Positional convenience wrapper around [`resolve_session_primary_agent`],
/// retained for existing call sites and tests. New code should use
/// [`SessionModeInput`] directly.
pub(crate) fn active_primary_agent_from_specs_for_mode(
    specs: &[SubagentSpec],
    vt_cfg: Option<&VTCodeConfig>,
    full_auto: bool,
    primary_agent_explicitly_configured: bool,
    resumed_primary_agent: Option<String>,
) -> Result<ActivePrimaryAgentState> {
    resolve_session_primary_agent(
        SessionModeInput::new(specs)
            .with_config(vt_cfg)
            .with_full_auto(full_auto)
            .with_explicit_config(primary_agent_explicitly_configured)
            .with_resumed(resumed_primary_agent),
    )
}

/// Convenience wrapper selecting the mode with no resume/full-auto context.
/// Test-only: used by `init.rs` unit tests.
#[cfg(test)]
pub(crate) fn active_primary_agent_from_specs(
    specs: &[SubagentSpec],
    vt_cfg: Option<&VTCodeConfig>,
) -> Result<ActivePrimaryAgentState> {
    active_primary_agent_from_specs_for_mode(specs, vt_cfg, false, false, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_config::{
        AgentMode, SubagentSource, SubagentSpec,
        core::permissions::{AgentPermissionsConfig, PermissionDefault},
    };

    fn spec(name: &str) -> SubagentSpec {
        SubagentSpec {
            name: name.to_string(),
            description: format!("{name} description"),
            prompt: format!("{name} instructions"),
            tools: None,
            disallowed_tools: Vec::new(),
            model: None,
            color: None,
            reasoning_effort: None,
            permissions: AgentPermissionsConfig::new(PermissionDefault::Deny),
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
            mode: AgentMode::Primary,
            max_turns: None,
            nickname_candidates: Vec::new(),
            initial_prompt: None,
            memory: None,
            isolation: None,
            aliases: Vec::new(),
            source: SubagentSource::ProjectVtcode,
            file_path: None,
            warnings: Vec::new(),
            tool_policy_overrides: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn resumed_mode_wins_over_config_default() {
        let cfg = VTCodeConfig {
            default_primary_agent: "builder".to_string(),
            ..VTCodeConfig::default()
        };
        let active = resolve_session_primary_agent(
            SessionModeInput::new(&[spec("plan"), spec("builder")])
                .with_config(Some(&cfg))
                .with_resumed(Some("plan".to_string())),
        )
        .expect("resumed primary agent");

        assert_eq!(active.active().name(), "plan");
    }

    #[test]
    fn explicit_config_overrides_resumed_mode() {
        let cfg = VTCodeConfig {
            default_primary_agent: "builder".to_string(),
            ..VTCodeConfig::default()
        };
        let active = resolve_session_primary_agent(
            SessionModeInput::new(&[spec("plan"), spec("builder")])
                .with_config(Some(&cfg))
                .with_explicit_config(true)
                .with_resumed(Some("plan".to_string())),
        )
        .expect("configured primary agent");

        assert_eq!(active.active().name(), "builder");
    }

    #[test]
    fn missing_resumed_mode_falls_back_to_config_default() {
        let cfg = VTCodeConfig {
            default_primary_agent: "builder".to_string(),
            ..VTCodeConfig::default()
        };
        let active = resolve_session_primary_agent(
            SessionModeInput::new(&[spec("builder")])
                .with_config(Some(&cfg))
                .with_resumed(Some(" ".to_string())),
        )
        .expect("fallback primary agent");

        assert_eq!(active.active().name(), "builder");
    }

    #[test]
    fn full_auto_without_explicit_selects_effective_auto() {
        let active = resolve_session_primary_agent(
            SessionModeInput::new(&[spec("auto")]).with_full_auto(true),
        )
        .expect("auto");

        assert_eq!(active.active().name(), "auto");
    }
}
