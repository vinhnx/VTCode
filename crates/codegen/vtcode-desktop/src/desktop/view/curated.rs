//! Curated Configuration rows for the desktop CLI config page.
//!
//! The searchable catalog stays the complete browser; this module pins the
//! handful of settings most users change to one overview group. Every row
//! reuses an existing edit path: the typed editor dialog, the inline boolean
//! toggle, or the session permissions palette.

use super::composer_controls::capitalize_agent_label;
use super::configuration::value_summary;
use super::*;

/// Synthetic group shown first in the configuration rail.
pub(super) const CURATED_GROUP_NAME: &str = "Configuration";

pub(super) const SANDBOX_ENABLED_PATH: &str = "sandbox.enabled";
pub(super) const SANDBOX_POLICY_PATH: &str = "sandbox.default_policy";
pub(super) const NETWORK_POLICY_PATH: &str = "sandbox.network.policy";
pub(super) const WEB_SEARCH_PROVIDER_PATH: &str = "tools.web_search.provider";
pub(super) const VERBOSITY_PATH: &str = "agent.verbosity";
pub(super) const REASONING_DISPLAY_PATH: &str = "ui.reasoning_display_mode";
pub(super) const COMMUNICATION_STYLE_PATH: &str = "ui.communication_style";

/// Network policies behind the allow-network-access toggle.
pub(super) const NETWORK_ALLOW_ALL: &str = "allow_all";
pub(super) const NETWORK_ALLOWLIST_ONLY: &str = "allowlist_only";
pub(super) const NETWORK_BLOCK_ALL: &str = "block_all";
const SANDBOX_WORKSPACE_WRITE: &str = "workspace_write";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CuratedRowKind {
    /// Session permission edited through the existing permissions palette.
    PermissionPolicy,
    /// Catalog value edited through the existing typed editor dialog.
    ConfigValue,
    /// Network policy flipped between allow-all and allowlist-only.
    NetworkToggle,
    /// Boolean catalog value flipped with an inline switch.
    BoolToggle,
}

#[derive(Clone, Debug)]
pub(super) struct CuratedRow {
    pub(super) id: &'static str,
    pub(super) title: String,
    pub(super) description: String,
    pub(super) path: Option<&'static str>,
    pub(super) kind: CuratedRowKind,
}

/// Curated rows in display order.
pub(super) fn curated_rows() -> [CuratedRow; 8] {
    [
        CuratedRow {
            id: "sandboxing",
            title: rust_i18n::t!("app.configuration.row_sandboxing_title").to_string(),
            description: rust_i18n::t!("app.configuration.row_sandboxing_description").to_string(),
            path: Some(SANDBOX_ENABLED_PATH),
            kind: CuratedRowKind::BoolToggle,
        },
        CuratedRow {
            id: "approval",
            title: rust_i18n::t!("app.configuration.row_approval_title").to_string(),
            description: rust_i18n::t!("app.configuration.row_approval_description").to_string(),
            path: None,
            kind: CuratedRowKind::PermissionPolicy,
        },
        CuratedRow {
            id: "sandbox",
            title: rust_i18n::t!("app.configuration.row_sandbox_title").to_string(),
            description: rust_i18n::t!("app.configuration.row_sandbox_description").to_string(),
            path: Some(SANDBOX_POLICY_PATH),
            kind: CuratedRowKind::ConfigValue,
        },
        CuratedRow {
            id: "network",
            title: rust_i18n::t!("app.configuration.row_network_title").to_string(),
            description: rust_i18n::t!("app.configuration.row_network_description").to_string(),
            path: Some(NETWORK_POLICY_PATH),
            kind: CuratedRowKind::NetworkToggle,
        },
        CuratedRow {
            id: "web-search",
            title: rust_i18n::t!("app.configuration.row_web_search_title").to_string(),
            description: rust_i18n::t!("app.configuration.row_web_search_description").to_string(),
            path: Some(WEB_SEARCH_PROVIDER_PATH),
            kind: CuratedRowKind::ConfigValue,
        },
        CuratedRow {
            id: "verbosity",
            title: rust_i18n::t!("app.configuration.row_verbosity_title").to_string(),
            description: rust_i18n::t!("app.configuration.row_verbosity_description").to_string(),
            path: Some(VERBOSITY_PATH),
            kind: CuratedRowKind::ConfigValue,
        },
        CuratedRow {
            id: "reasoning",
            title: rust_i18n::t!("app.configuration.row_reasoning_title").to_string(),
            description: rust_i18n::t!("app.configuration.row_reasoning_description").to_string(),
            path: Some(REASONING_DISPLAY_PATH),
            kind: CuratedRowKind::ConfigValue,
        },
        CuratedRow {
            id: "communication",
            title: rust_i18n::t!("app.configuration.row_communication_title").to_string(),
            description: rust_i18n::t!("app.configuration.row_communication_description").to_string(),
            path: Some(COMMUNICATION_STYLE_PATH),
            kind: CuratedRowKind::ConfigValue,
        },
    ]
}

pub(super) fn find_setting<'a>(
    snapshot: Option<&'a protocol::ConfigSettingsSnapshot>,
    path: &str,
) -> Option<&'a protocol::ConfigSetting> {
    snapshot?.settings.iter().find(|setting| setting.path == path)
}

/// Render a snake_case config value the way the curated rows display it.
pub(super) fn humanize_config_value(value: &str) -> String {
    capitalize_agent_label(&value.replace('_', " "))
}

/// Display text for a curated config value: humanized strings, an unset
/// marker, or the shared truncated summary for anything else.
pub(super) fn setting_display_value(setting: &protocol::ConfigSetting) -> String {
    match &setting.value {
        Value::String(value) => humanize_config_value(value),
        Value::Null => rust_i18n::t!(i18n::CONFIGURATION_UNSET).to_string(),
        value => value_summary(value),
    }
}

/// Whether the effective network policy currently allows all egress.
pub(super) fn network_access_enabled(snapshot: Option<&protocol::ConfigSettingsSnapshot>) -> bool {
    find_setting(snapshot, NETWORK_POLICY_PATH)
        .and_then(|setting| setting.value.as_str())
        .is_some_and(|policy| policy == NETWORK_ALLOW_ALL)
}

/// The network toggle only applies when sandboxing is on with the
/// workspace-write policy; other policies decide egress on their own.
pub(super) fn network_toggle_available(snapshot: Option<&protocol::ConfigSettingsSnapshot>) -> bool {
    let sandbox_enabled = find_setting(snapshot, SANDBOX_ENABLED_PATH)
        .and_then(|setting| setting.value.as_bool())
        .unwrap_or(false);
    let workspace_write = find_setting(snapshot, SANDBOX_POLICY_PATH)
        .and_then(|setting| setting.value.as_str())
        .is_some_and(|policy| policy == SANDBOX_WORKSPACE_WRITE);
    sandbox_enabled && workspace_write
}

pub(super) fn network_toggle_value(enabled: bool) -> Value {
    Value::String(
        if enabled {
            NETWORK_ALLOW_ALL
        } else {
            NETWORK_ALLOWLIST_ONLY
        }
        .to_string(),
    )
}

/// Whether the effective network policy blocks all egress. The toggle has no
/// blocked position, so callers must disable it rather than misreport OFF:
/// flipping through the toggle would silently replace `block_all`.
pub(super) fn network_policy_blocked(snapshot: Option<&protocol::ConfigSettingsSnapshot>) -> bool {
    find_setting(snapshot, NETWORK_POLICY_PATH)
        .and_then(|setting| setting.value.as_str())
        .is_some_and(|policy| policy == NETWORK_BLOCK_ALL)
}

/// Levels shown in model controls: the user's selected set intersected with
/// route support, plus the active effort when the route supports it, so hiding
/// the active level never strands the knob. Order follows the service list. An
/// effort the route does not support stays excluded so the caller can offer
/// explicit valid choices instead of a meaningless knob position.
pub(super) fn visible_effort_levels(supported: &[String], selected: &[String], current: &str) -> Vec<String> {
    supported
        .iter()
        .filter(|level| level.as_str() == current || selected.iter().any(|entry| entry == *level))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(settings: Vec<(&str, Value)>) -> protocol::ConfigSettingsSnapshot {
        protocol::ConfigSettingsSnapshot {
            settings: settings
                .into_iter()
                .map(|(path, value)| protocol::ConfigSetting {
                    path: path.to_string(),
                    value,
                    origin: None,
                    label: None,
                    type_name: None,
                    default_value: None,
                    description: None,
                    options: Vec::new(),
                    group: None,
                    group_description: None,
                    editable: true,
                })
                .collect(),
            target_versions: Vec::new(),
            omitted_sensitive: 0,
            omitted_large: 0,
            omitted_limit: 0,
        }
    }

    #[test]
    fn curated_rows_cover_the_overview_settings_without_duplicate_ids_or_paths() {
        let rows = curated_rows();
        assert_eq!(rows.len(), 8);
        let mut ids = std::collections::BTreeSet::new();
        let mut paths = std::collections::BTreeSet::new();
        for row in &rows {
            assert!(ids.insert(row.id), "duplicate curated row id");
            assert!(!row.title.is_empty());
            assert!(!row.description.is_empty());
            if let Some(path) = row.path {
                assert!(paths.insert(path), "duplicate curated setting path");
            }
        }
        assert_eq!(paths.len(), 7, "only the approval row has no catalog path");
    }

    #[test]
    fn humanized_values_read_like_labels_not_identifiers() {
        assert_eq!(humanize_config_value("workspace_write"), "Workspace write");
        assert_eq!(humanize_config_value("read_only"), "Read only");
        assert_eq!(humanize_config_value("auto"), "Auto");
        assert_eq!(humanize_config_value(""), "");
    }

    #[test]
    fn network_access_is_on_only_for_allow_all() {
        for (policy, expected) in [
            (serde_json::json!("allow_all"), true),
            (serde_json::json!("allowlist_only"), false),
            (serde_json::json!("block_all"), false),
            (serde_json::json!(true), false),
            (serde_json::json!(null), false),
        ] {
            let snapshot = snapshot(vec![(NETWORK_POLICY_PATH, policy)]);
            assert_eq!(network_access_enabled(Some(&snapshot)), expected);
        }
        assert!(!network_access_enabled(None));
        assert!(!network_access_enabled(Some(&snapshot(Vec::new()))));
    }

    #[test]
    fn blocked_network_policy_is_detected_for_the_toggle() {
        let blocked = snapshot(vec![(NETWORK_POLICY_PATH, serde_json::json!("block_all"))]);
        assert!(network_policy_blocked(Some(&blocked)));
        for policy in ["allow_all", "allowlist_only"] {
            let snapshot = snapshot(vec![(NETWORK_POLICY_PATH, serde_json::json!(policy))]);
            assert!(!network_policy_blocked(Some(&snapshot)));
        }
        assert!(!network_policy_blocked(None));
        assert!(!network_policy_blocked(Some(&snapshot(Vec::new()))));
    }

    #[test]
    fn setting_display_value_humanizes_strings_and_marks_unset() {
        let string = protocol::ConfigSetting {
            path: SANDBOX_POLICY_PATH.to_string(),
            value: serde_json::json!("workspace_write"),
            origin: None,
            label: None,
            type_name: None,
            default_value: None,
            description: None,
            options: Vec::new(),
            group: None,
            group_description: None,
            editable: true,
        };
        assert_eq!(setting_display_value(&string), "Workspace write");
        let unset = protocol::ConfigSetting { value: Value::Null, ..string.clone() };
        assert_eq!(setting_display_value(&unset), "<unset>");
    }

    #[test]
    fn network_toggle_needs_an_enabled_workspace_write_sandbox() {
        let available = snapshot(vec![
            (SANDBOX_ENABLED_PATH, serde_json::json!(true)),
            (SANDBOX_POLICY_PATH, serde_json::json!("workspace_write")),
        ]);
        assert!(network_toggle_available(Some(&available)));
        for settings in [
            vec![
                (SANDBOX_ENABLED_PATH, serde_json::json!(false)),
                (SANDBOX_POLICY_PATH, serde_json::json!("workspace_write")),
            ],
            vec![
                (SANDBOX_ENABLED_PATH, serde_json::json!(true)),
                (SANDBOX_POLICY_PATH, serde_json::json!("read_only")),
            ],
            vec![
                (SANDBOX_ENABLED_PATH, serde_json::json!(true)),
                (SANDBOX_POLICY_PATH, serde_json::json!("danger_full_access")),
            ],
            vec![(SANDBOX_POLICY_PATH, serde_json::json!("workspace_write"))],
            vec![(SANDBOX_ENABLED_PATH, serde_json::json!(true))],
        ] {
            assert!(!network_toggle_available(Some(&snapshot(settings))));
        }
        assert!(!network_toggle_available(None));
    }

    #[test]
    fn visible_efforts_follow_service_order_and_keep_the_current_level() {
        let supported = ["minimal", "low", "medium", "high"].map(str::to_string).to_vec();
        let selected = ["high", "low"].map(str::to_string).to_vec();
        assert_eq!(
            visible_effort_levels(&supported, &selected, "low"),
            vec!["low".to_string(), "high".to_string()],
            "selection order must not reorder the knob"
        );
        assert_eq!(
            visible_effort_levels(&supported, &selected, "medium"),
            vec!["low".to_string(), "medium".to_string(), "high".to_string()],
            "the active level stays visible in service position"
        );
        assert_eq!(
            visible_effort_levels(&supported, &[], "max"),
            Vec::<String>::new(),
            "an unsupported active level stays excluded for explicit choices"
        );
        assert!(visible_effort_levels(&[], &selected, "medium").is_empty());
        assert!(visible_effort_levels(&[], &[], "").is_empty());
    }
}
