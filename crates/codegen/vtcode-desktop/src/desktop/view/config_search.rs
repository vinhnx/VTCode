//! Config catalog search: precomputed corpus and exact-terms matching.
//!
//! The corpus holds one lowercased haystack per setting, built once per
//! snapshot epoch so per-keystroke filtering is allocation-free substring
//! checks. `build_config_catalog_cache` (in `configuration.rs`) is the only
//! grouping consumer.

use super::configuration::{config_group_name, value_summary};
use super::*;

/// Lowercased whitespace-separated search terms; every term must match.
/// Unicode lowering mirrors the CLI's `normalize_query`.
pub(super) fn config_search_terms(query: &str) -> Vec<String> {
    query.split_whitespace().map(|term| term.to_lowercase()).collect()
}

/// Lowercased per-setting search haystacks, built once per settings
/// snapshot so per-keystroke filtering is substring checks with no
/// allocation. `haystacks[i]` covers `snapshot.settings[i]`.
pub(super) struct ConfigSearchCorpus {
    pub(super) epoch: u64,
    pub(super) haystacks: Vec<String>,
}

impl ConfigSearchCorpus {
    pub(super) fn build(snapshot: &protocol::ConfigSettingsSnapshot, epoch: u64) -> Self {
        Self {
            epoch,
            haystacks: snapshot.settings.iter().map(setting_haystack).collect(),
        }
    }

    pub(super) fn is_fresh(&self, snapshot: &protocol::ConfigSettingsSnapshot, epoch: u64) -> bool {
        self.epoch == epoch && self.haystacks.len() == snapshot.settings.len()
    }
}

/// Lowercased searchable text for one setting: path, label, value, group,
/// documentation, origin, default, and options, mirroring the CLI's
/// exact-terms filter.
fn setting_haystack(setting: &protocol::ConfigSetting) -> String {
    let mut haystack = String::new();
    haystack.push_str(&setting.path);
    haystack.push(' ');
    if let Some(label) = setting.label.as_deref() {
        haystack.push_str(label);
        haystack.push(' ');
    }
    haystack.push_str(&value_summary(&setting.value));
    haystack.push(' ');
    haystack.push_str(config_group_name(setting));
    haystack.push(' ');
    if let Some(description) = setting.description.as_deref() {
        haystack.push_str(description);
        haystack.push(' ');
    }
    if let Some(origin) = setting.origin.as_deref() {
        haystack.push_str(origin);
        haystack.push(' ');
    }
    if let Some(default) = setting.default_value.as_deref() {
        haystack.push_str(default);
        haystack.push(' ');
    }
    for option in &setting.options {
        haystack.push_str(option);
        haystack.push(' ');
    }
    haystack.to_lowercase()
}

/// Whether a precomputed haystack matches every search term.
pub(super) fn haystack_matches_terms(haystack: &str, terms: &[String]) -> bool {
    terms.iter().all(|term| haystack.contains(term))
}

/// Whether a setting matches every search term across its path, label,
/// value, group, documentation, origin, default, and options, mirroring the
/// CLI's exact-terms filter.
pub(super) fn setting_matches_query(setting: &protocol::ConfigSetting, terms: &[String]) -> bool {
    if terms.is_empty() {
        return true;
    }
    haystack_matches_terms(&setting_haystack(setting), terms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn config_search_folds_unicode_case_without_stripping_accents() {
        let accented = protocol::ConfigSetting {
            path: "agent.break_mode".into(),
            value: json!(true),
            origin: None,
            label: Some("Café mode".into()),
            type_name: Some("bool".into()),
            default_value: None,
            description: None,
            options: Vec::new(),
            group: None,
            group_description: None,
            editable: true,
        };
        assert!(setting_matches_query(&accented, &config_search_terms("CAFÉ")));
        assert!(setting_matches_query(&accented, &config_search_terms("café")));
        assert!(
            !setting_matches_query(&accented, &config_search_terms("CAFE")),
            "folding is case-only; accents still distinguish terms"
        );
    }
}
