use crate::config::VTCodeConfig;
use crate::config::constants::tools;

/// Lifecycle stage for a feature gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureStage {
    Stable,
    Beta,
}

/// Generic feature gate with stage metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureGate {
    pub enabled: bool,
    pub stage: FeatureStage,
}

impl FeatureGate {
    pub const fn new(enabled: bool, stage: FeatureStage) -> Self {
        Self { enabled, stage }
    }
}

/// Open Responses-specific feature gate data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenResponsesFeature {
    pub enabled: bool,
    pub emit_events: bool,
    pub map_tool_calls: bool,
    pub include_reasoning: bool,
    pub stage: FeatureStage,
}

/// Immutable session-scoped feature flags derived from config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSet {
    pub request_user_input: FeatureGate,
    pub auto_compaction: FeatureGate,
    pub open_responses: OpenResponsesFeature,
}

impl FeatureSet {
    pub fn from_config(config: Option<&VTCodeConfig>) -> Self {
        let default_config;
        let cfg = if let Some(cfg) = config {
            cfg
        } else {
            default_config = VTCodeConfig::default();
            &default_config
        };

        Self {
            request_user_input: FeatureGate::new(
                cfg.chat.ask_questions.enabled,
                FeatureStage::Stable,
            ),
            auto_compaction: FeatureGate::new(
                cfg.agent.harness.auto_compaction_enabled,
                FeatureStage::Beta,
            ),
            open_responses: OpenResponsesFeature {
                enabled: cfg.agent.open_responses.enabled,
                emit_events: cfg.agent.open_responses.enabled
                    && cfg.agent.open_responses.emit_events,
                map_tool_calls: cfg.agent.open_responses.enabled
                    && cfg.agent.open_responses.map_tool_calls,
                include_reasoning: cfg.agent.open_responses.enabled
                    && cfg.agent.open_responses.include_reasoning,
                stage: FeatureStage::Beta,
            },
        }
    }

    pub fn request_user_input_enabled(&self, plan_mode: bool, interactive_session: bool) -> bool {
        interactive_session && (plan_mode || self.request_user_input.enabled)
    }

    pub fn auto_compaction_enabled(&self, supports_server_compaction: bool) -> bool {
        self.auto_compaction.enabled && supports_server_compaction
    }

    pub fn tool_enabled_for_mode(
        tool_name: &str,
        plan_mode: bool,
        request_user_input_enabled: bool,
    ) -> bool {
        match tool_name {
            tools::REQUEST_USER_INPUT => request_user_input_enabled,
            tools::PLAN_TASK_TRACKER => plan_mode,
            _ => true,
        }
    }

    pub fn allows_tool_name(
        &self,
        tool_name: &str,
        plan_mode: bool,
        interactive_session: bool,
    ) -> bool {
        Self::tool_enabled_for_mode(
            tool_name,
            plan_mode,
            self.request_user_input_enabled(plan_mode, interactive_session),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{FeatureSet, FeatureStage};
    use crate::config::VTCodeConfig;
    use crate::config::constants::tools;

    #[test]
    fn request_user_input_requires_interactive_session() {
        let cfg = VTCodeConfig::default();
        let features = FeatureSet::from_config(Some(&cfg));

        assert!(features.request_user_input_enabled(false, true));
        assert!(features.request_user_input_enabled(true, true));
        assert!(!features.request_user_input_enabled(true, false));
        assert!(features.allows_tool_name(tools::REQUEST_USER_INPUT, false, true));
        assert!(features.allows_tool_name(tools::REQUEST_USER_INPUT, true, true));
        assert!(!features.allows_tool_name(tools::REQUEST_USER_INPUT, true, false));
        assert!(features.allows_tool_name(tools::PLAN_TASK_TRACKER, true, false));
        assert!(!features.allows_tool_name(tools::PLAN_TASK_TRACKER, false, true));
    }

    #[test]
    fn request_user_input_honors_chat_setting_outside_plan_mode() {
        let mut cfg = VTCodeConfig::default();
        cfg.chat.ask_questions.enabled = false;

        let features = FeatureSet::from_config(Some(&cfg));

        assert!(!features.request_user_input_enabled(false, true));
        assert!(features.request_user_input_enabled(true, true));
    }

    #[test]
    fn auto_compaction_requires_provider_support() {
        let mut cfg = VTCodeConfig::default();
        cfg.agent.harness.auto_compaction_enabled = true;

        let features = FeatureSet::from_config(Some(&cfg));

        assert!(!features.auto_compaction_enabled(false));
        assert!(features.auto_compaction_enabled(true));
        assert_eq!(features.auto_compaction.stage, FeatureStage::Beta);
    }

    #[test]
    fn open_responses_gate_tracks_emit_settings() {
        let mut cfg = VTCodeConfig::default();
        cfg.agent.open_responses.enabled = true;
        cfg.agent.open_responses.emit_events = false;
        cfg.agent.open_responses.map_tool_calls = true;
        cfg.agent.open_responses.include_reasoning = true;

        let features = FeatureSet::from_config(Some(&cfg));

        assert!(features.open_responses.enabled);
        assert!(!features.open_responses.emit_events);
        assert!(features.open_responses.map_tool_calls);
        assert!(features.open_responses.include_reasoning);
    }
}
