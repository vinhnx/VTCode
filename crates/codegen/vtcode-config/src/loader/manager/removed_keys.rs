//! Detection of schema key paths that were removed but still parse.
//!
//! Serde ignores unknown fields, so a retired setting can survive in an old
//! config file without failing to deserialize. These helpers let the repair
//! path flag and report such keys; kept in their own module so `manager.rs`
//! stays focused on the load/layer/validation flow.

/// Key paths that were removed from the schema but still parse (serde ignores
/// unknown fields). Detection is pure so tests can pin the exact paths.
pub(in crate::loader) const REMOVED_CONFIG_KEY_PATHS: &[&[&str]] = &[
    &["agent", "harness", "max_tool_calls_per_turn"],
    &["tools", "max_tool_loops"],
    &["automation", "full_auto", "auto_grant_tool_limits"],
    &["telemetry", "sample_interval_ms"],
    &["telemetry", "retention_days"],
    &["telemetry", "bottleneck_tracing"],
    &["ui", "show_timeline_pane"],
    &["context", "trim_to_percent"],
    &["prompt_cache", "providers", "anthropic", "default_ttl_seconds"],
];

pub(in crate::loader) fn removed_config_key_paths_present(value: &toml::Value) -> Vec<&'static [&'static str]> {
    REMOVED_CONFIG_KEY_PATHS
        .iter()
        .copied()
        .filter(|key_path| key_path.iter().try_fold(value, |cursor, key| cursor.get(*key)).is_some())
        .collect()
}
