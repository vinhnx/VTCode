use crate::config::VTCodeConfig;
use serde_json::json;

/// Apply backward-compatible defaults for new config sections.
pub fn apply_migration_defaults(config: &mut VTCodeConfig) {
    if config.tools.plugins.manifests.is_empty() {
        config.tools.plugins.manifests = vec!["~/.vtcode/plugins".into()];
    }
}

/// Emit a structured migration summary for callers.
pub fn migration_summary(config: &VTCodeConfig) -> serde_json::Value {
    json!({
        "plugins": {
            "enabled": config.tools.plugins.enabled,
            "manifests": config.tools.plugins.manifests,
        },
        "security": {
            "zero_trust": config.security.zero_trust_mode,
            "integrity_checks": config.security.integrity_checks,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_missing_defaults() {
        let mut config = VTCodeConfig::default();
        apply_migration_defaults(&mut config);
        assert!(!config.tools.plugins.manifests.is_empty());
    }
}
