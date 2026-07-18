use crate::config::VTCodeConfig;
use crate::config::constants::tools;
use crate::tools::tool_intent::{ToolMutationModel, builtin_tool_behavior};

/// Lifecycle stage for a feature gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureStage {
    /// Feature is stable and fully supported.
    Stable,
    /// Feature is in beta and may change without notice.
    Beta,
}

/// Generic feature gate with stage metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureGate {
    /// Whether the feature is currently enabled.
    pub enabled: bool,
    /// Lifecycle stage of the feature.
    pub stage: FeatureStage,
}

impl FeatureGate {
    /// Create a new feature gate with the given enabled state and stage.
    pub const fn new(enabled: bool, stage: FeatureStage) -> Self {
        Self { enabled, stage }
    }
}

/// Open Responses-specific feature gate data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenResponsesFeature {
    /// Whether Open Responses support is enabled.
    pub enabled: bool,
    /// Whether to emit Open Responses-specific lifecycle events.
    pub emit_events: bool,
    /// Whether to map tool calls to Open Responses format.
    pub map_tool_calls: bool,
    /// Whether to include reasoning content in responses.
    pub include_reasoning: bool,
    /// Lifecycle stage of the feature.
    pub stage: FeatureStage,
}

/// Immutable session-scoped feature flags derived from config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSet {
    /// Gate for the `request_user_input` tool.
    pub request_user_input: FeatureGate,
    /// Gate for automatic context compaction.
    pub auto_compaction: FeatureGate,
    /// Gate for Open Responses-specific features.
    pub open_responses: OpenResponsesFeature,
}

impl FeatureSet {
    /// Build a [`FeatureSet`] from the workspace configuration, falling back
    /// to defaults when no config is provided.
    pub fn from_config(config: Option<&VTCodeConfig>) -> Self {
        let default_config;
        let cfg = if let Some(cfg) = config {
            cfg
        } else {
            default_config = VTCodeConfig::default();
            &default_config
        };

        Self {
            request_user_input: FeatureGate::new(cfg.chat.ask_questions.enabled, FeatureStage::Stable),
            auto_compaction: FeatureGate::new(cfg.agent.harness.auto_compaction_enabled, FeatureStage::Beta),
            open_responses: OpenResponsesFeature {
                enabled: cfg.agent.open_responses.enabled,
                emit_events: cfg.agent.open_responses.enabled && cfg.agent.open_responses.emit_events,
                map_tool_calls: cfg.agent.open_responses.enabled && cfg.agent.open_responses.map_tool_calls,
                include_reasoning: cfg.agent.open_responses.enabled && cfg.agent.open_responses.include_reasoning,
                stage: FeatureStage::Beta,
            },
        }
    }

    /// Whether the `request_user_input` tool is available in the current context.
    pub fn request_user_input_enabled(&self, _planning_active: bool, interactive_session: bool) -> bool {
        interactive_session && self.request_user_input.enabled
    }

    /// Whether auto-compaction is enabled, requiring provider server-side support.
    pub fn auto_compaction_enabled(&self, supports_server_compaction: bool) -> bool {
        self.auto_compaction.enabled && supports_server_compaction
    }

    /// Whether a specific tool is allowed in the current planning mode.
    pub fn tool_enabled_for_mode(tool_name: &str, planning_active: bool, request_user_input_enabled: bool) -> bool {
        match tool_name {
            tools::REQUEST_USER_INPUT => request_user_input_enabled,
            _ if !planning_active => true,
            _ => {
                builtin_tool_behavior(tool_name)
                    .map(|behavior| !matches!(behavior.mutation_model, ToolMutationModel::Mutating))
                    .unwrap_or(false) // fail-closed: deny unknown tools in planning mode
            }
        }
    }

    /// Whether a tool name is allowed given the current feature gates and session state.
    pub fn allows_tool_name(&self, tool_name: &str, planning_active: bool, interactive_session: bool) -> bool {
        Self::tool_enabled_for_mode(
            tool_name,
            planning_active,
            self.request_user_input_enabled(planning_active, interactive_session),
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
        assert!(features.allows_tool_name(tools::TASK_TRACKER, true, false));
        assert!(features.allows_tool_name(tools::TASK_TRACKER, false, true));
    }

    #[test]
    fn request_user_input_honors_chat_setting_outside_planning_workflow() {
        let mut cfg = VTCodeConfig::default();
        cfg.chat.ask_questions.enabled = false;

        let features = FeatureSet::from_config(Some(&cfg));

        assert!(!features.request_user_input_enabled(false, true));
        assert!(!features.request_user_input_enabled(true, true));
    }

    #[test]
    fn planning_workflow_hides_mutating_only_tools_but_keeps_conditional_tools() {
        let cfg = VTCodeConfig::default();
        let features = FeatureSet::from_config(Some(&cfg));

        assert!(!features.allows_tool_name(tools::APPLY_PATCH, true, true));
        assert!(!features.allows_tool_name(tools::WRITE_FILE, true, true));
        assert!(features.allows_tool_name(tools::UNIFIED_FILE, true, true));
        assert!(features.allows_tool_name(tools::UNIFIED_EXEC, true, true));
        assert!(features.allows_tool_name(tools::TASK_TRACKER, true, true));
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
