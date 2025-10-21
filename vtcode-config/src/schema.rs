#![cfg(feature = "schema")]

use anyhow::{Context, Result};
use schemars::{schema::RootSchema, schema_for};

use crate::loader::VTCodeConfig;

/// Generate the JSON Schema describing the `vtcode.toml` configuration surface.
pub fn vtcode_config_schema() -> RootSchema {
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
    serde_json::to_string_pretty(&value)
        .context("failed to serialize vtcode-config schema to pretty JSON string")
}
