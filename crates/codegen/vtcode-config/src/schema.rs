use anyhow::{Context, Result};
use schemars::{Schema, schema_for};

use crate::loader::VTCodeConfig;

/// Generate the JSON Schema describing the `vtcode.toml` configuration surface.
pub fn vtcode_config_schema() -> Schema {
    schema_for!(VTCodeConfig)
}

/// Render the configuration schema as a `serde_json::Value` for downstream tooling.
pub fn vtcode_config_schema_json() -> Result<serde_json::Value> {
    let schema = vtcode_config_schema();
    serde_json::to_value(schema).context("failed to serialize vtcode-config schema to JSON value")
}

/// Render the configuration schema as a pretty-printed JSON string.
pub fn vtcode_config_schema_pretty() -> Result<String> {
    let value = vtcode_config_schema_json()?;
    serde_json::to_string_pretty(&value).context("failed to serialize vtcode-config schema to pretty JSON string")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_exposes_prompt_cache_ttl_and_custom_capability() {
        let schema = vtcode_config_schema_json().unwrap();
        assert_eq!(schema.pointer("/$defs/PromptCacheTtl/enum"), Some(&serde_json::json!(["30m", "12h"])));
        for definition in ["CustomProviderConfig", "CustomProviderProfileConfig"] {
            assert!(
                schema
                    .pointer(&format!("/$defs/{definition}/properties/supports_prompt_cache_options"))
                    .is_some()
            );
        }
        assert!(
            schema
                .pointer("/$defs/OpenAIPromptCacheSettings/properties/prompt_cache_ttl")
                .is_some()
        );
    }
}
