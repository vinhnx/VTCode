use std::sync::{Arc, RwLock};

use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::ReasoningEffortLevel;

use crate::agent::runloop::model_picker::{ModelPickerState, ModelSelectionResult};

/// A completed UI selection waiting for the next request boundary.
pub(crate) enum SessionSettingsControl {
    Model {
        picker: Box<ModelPickerState>,
        selection: ModelSelectionResult,
    },
}

/// Live provider/model/reasoning/config snapshot shared between the idle
/// interaction loop and the busy-turn settings task.
///
/// The busy picker (`session_setup/ui/active_settings.rs`) runs concurrently
/// with an in-flight request, so it cannot borrow the turn config; the idle
/// interaction loop applies `/model` changes to that config itself. Publishing
/// the applied values here keeps a busy picker opened after an idle change from
/// showing stale "current" badges and reasoning defaults.
///
/// `vt_cfg` carries the picker's config-derived chrome (custom providers,
/// provider order, auth routes, `service_tier`). It is refreshed only on a live
/// config reload, since it is too large to clone on every loop iteration.
#[derive(Debug, Clone)]
pub(crate) struct SettingsSnapshot {
    pub provider: String,
    pub model: String,
    pub effort: ReasoningEffortLevel,
    pub vt_cfg: Option<Arc<VTCodeConfig>>,
}

pub(crate) type SharedSettingsSnapshot = Arc<RwLock<SettingsSnapshot>>;

/// Create a shared snapshot seeded from the session's initial settings.
pub(crate) fn shared_settings_snapshot(
    provider: impl Into<String>,
    model: impl Into<String>,
    effort: ReasoningEffortLevel,
    vt_cfg: Option<&VTCodeConfig>,
) -> SharedSettingsSnapshot {
    Arc::new(RwLock::new(SettingsSnapshot {
        provider: provider.into(),
        model: model.into(),
        effort,
        vt_cfg: vt_cfg.map(|cfg| Arc::new(cfg.clone())),
    }))
}

/// Read the current snapshot, recovering from a poisoned lock.
pub(crate) fn read_settings_snapshot(shared: &SharedSettingsSnapshot) -> SettingsSnapshot {
    let guard = shared.read().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.clone()
}

/// Publish the current settings, skipping the write when nothing changed.
///
/// The interaction loop calls this every iteration, so the read-only fast path
/// avoids re-allocating the two strings when the model is unchanged.
pub(crate) fn publish_settings_if_changed(
    shared: &SharedSettingsSnapshot,
    provider: &str,
    model: &str,
    effort: ReasoningEffortLevel,
) {
    let unchanged = {
        let current = shared.read().unwrap_or_else(|poisoned| poisoned.into_inner());
        current.provider == provider && current.model == model && current.effort == effort
    };
    if unchanged {
        return;
    }

    let mut guard = shared.write().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.provider.clear();
    guard.provider.push_str(provider);
    guard.model.clear();
    guard.model.push_str(model);
    guard.effort = effort;
}

/// Publish the live workspace config used by the busy-turn picker.
///
/// Called at the interaction loop's live-reload boundary, where `ctx.vt_cfg`
/// may have been replaced. Deliberately separate from
/// [`publish_settings_if_changed`]: that helper runs every loop iteration and
/// must not deep-clone the whole config, whereas this one only runs when the
/// on-disk config actually changed.
pub(crate) fn publish_vt_cfg(shared: &SharedSettingsSnapshot, vt_cfg: Option<&VTCodeConfig>) {
    let next = vt_cfg.map(|cfg| Arc::new(cfg.clone()));
    let mut guard = shared.write().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.vt_cfg = next;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_snapshot_starts_with_seed() {
        let shared = shared_settings_snapshot("anthropic", "claude-sonnet", ReasoningEffortLevel::Low, None);
        let snapshot = read_settings_snapshot(&shared);
        assert_eq!(snapshot.provider, "anthropic");
        assert_eq!(snapshot.model, "claude-sonnet");
        assert_eq!(snapshot.effort, ReasoningEffortLevel::Low);
        assert!(snapshot.vt_cfg.is_none());
    }

    #[test]
    fn shared_snapshot_seeds_live_config() {
        let cfg = VTCodeConfig {
            default_primary_agent: "seed-agent".to_string(),
            ..Default::default()
        };
        let shared = shared_settings_snapshot("openai", "gpt-5", ReasoningEffortLevel::Medium, Some(&cfg));
        let snapshot = read_settings_snapshot(&shared);
        assert_eq!(snapshot.vt_cfg.as_deref().map(|cfg| cfg.default_primary_agent.as_str()), Some("seed-agent"));
    }

    #[test]
    fn publish_vt_cfg_replaces_and_clears() {
        let shared = shared_settings_snapshot("openai", "gpt-5", ReasoningEffortLevel::Medium, None);

        let cfg = VTCodeConfig {
            default_primary_agent: "live-agent".to_string(),
            ..Default::default()
        };
        publish_vt_cfg(&shared, Some(&cfg));
        assert_eq!(
            read_settings_snapshot(&shared)
                .vt_cfg
                .as_deref()
                .map(|cfg| cfg.default_primary_agent.as_str()),
            Some("live-agent")
        );

        // A reload that drops the workspace config must clear the snapshot too.
        publish_vt_cfg(&shared, None);
        assert!(read_settings_snapshot(&shared).vt_cfg.is_none());
    }

    #[test]
    fn publish_settings_if_changed_preserves_vt_cfg() {
        // Asymmetric guard: the per-iteration settings publish must never
        // clobber the config published by the rare reload path.
        let cfg = VTCodeConfig {
            default_primary_agent: "live-agent".to_string(),
            ..Default::default()
        };
        let shared = shared_settings_snapshot("openai", "gpt-5", ReasoningEffortLevel::Medium, Some(&cfg));
        let seeded = read_settings_snapshot(&shared).vt_cfg.expect("seeded vt_cfg");

        publish_settings_if_changed(&shared, "anthropic", "claude-opus", ReasoningEffortLevel::High);

        let after = read_settings_snapshot(&shared).vt_cfg.expect("vt_cfg retained");
        assert!(Arc::ptr_eq(&seeded, &after), "settings publish must not replace the live config");
    }

    #[test]
    fn publish_if_changed_updates_all_fields() {
        let shared = shared_settings_snapshot("openai", "gpt-5", ReasoningEffortLevel::Medium, None);
        publish_settings_if_changed(&shared, "anthropic", "claude-opus", ReasoningEffortLevel::High);
        let snapshot = read_settings_snapshot(&shared);
        assert_eq!(snapshot.provider, "anthropic");
        assert_eq!(snapshot.model, "claude-opus");
        assert_eq!(snapshot.effort, ReasoningEffortLevel::High);
    }

    #[test]
    fn publish_if_changed_skips_when_equal() {
        let shared = shared_settings_snapshot("openai", "gpt-5", ReasoningEffortLevel::Medium, None);
        publish_settings_if_changed(&shared, "openai", "gpt-5", ReasoningEffortLevel::Medium);
        let snapshot = read_settings_snapshot(&shared);
        assert_eq!(snapshot.provider, "openai");
        assert_eq!(snapshot.model, "gpt-5");
        assert_eq!(snapshot.effort, ReasoningEffortLevel::Medium);
    }

    #[test]
    fn publish_if_changed_detects_effort_only_change() {
        // Asymmetric inputs: same provider/model but a different effort must
        // still publish, otherwise the busy picker keeps the old default.
        let shared = shared_settings_snapshot("openai", "gpt-5", ReasoningEffortLevel::Medium, None);
        publish_settings_if_changed(&shared, "openai", "gpt-5", ReasoningEffortLevel::XHigh);
        assert_eq!(read_settings_snapshot(&shared).effort, ReasoningEffortLevel::XHigh);
    }
}
