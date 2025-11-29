//! Configuration loading optimizer to eliminate redundant serialization
//!
//! This module provides direct struct manipulation for configuration overrides
//! instead of the inefficient serialize->modify->deserialize pattern.

use anyhow::{Context, Result};
use std::str::FromStr;
use vtcode_config::loader::VTCodeConfig;

/// Direct configuration override application without serialization round-trips
pub struct ConfigOptimizer;

impl ConfigOptimizer {
    /// Apply configuration overrides directly to the struct
    pub fn apply_overrides(config: &mut VTCodeConfig, overrides: &[(String, String)]) -> Result<()> {
        for (key, raw_value) in overrides {
            Self::apply_override(config, key, raw_value)
                .with_context(|| format!("Failed to apply override for key '{}'", key))?;
        }

        // Validate the updated configuration
        config.validate()
            .context("Configuration overrides failed validation")?;

        Ok(())
    }

    /// Apply a single override to the configuration struct
    fn apply_override(config: &mut VTCodeConfig, key: &str, raw_value: &str) -> Result<()> {
        let segments: Vec<&str> = key
            .split('.')
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .collect();

        if segments.is_empty() {
            return Err(anyhow::anyhow!("Empty configuration key"));
        }

        match segments[0] {
            "agent" => Self::apply_agent_override(config, &segments[1..], raw_value)?,
            "mcp" => Self::apply_mcp_override(config, &segments[1..], raw_value)?,
            "tools" => Self::apply_tools_override(config, &segments[1..], raw_value)?,
            "ui" => Self::apply_ui_override(config, &segments[1..], raw_value)?,
            "debug" => Self::apply_debug_override(config, &segments[1..], raw_value)?,
            "security" => Self::apply_security_override(config, &segments[1..], raw_value)?,
            _ => {
                // Fallback to reflection-based override for unknown paths
                Self::apply_reflection_override(config, &segments, raw_value)?;
            }
        }

        Ok(())
    }

    /// Apply agent-related overrides
    fn apply_agent_override(config: &mut VTCodeConfig, path: &[&str], value: &str) -> Result<()> {
        match path {
            ["provider"] => {
                config.agent.provider = value.to_string();
            }
            ["default_model"] => {
                config.agent.default_model = value.to_string();
            }
            ["api_key_env"] => {
                config.agent.api_key_env = Some(value.to_string());
            }
            ["temperature"] => {
                config.agent.temperature = Some(parse_f64(value)?);
            }
            ["max_tokens"] => {
                config.agent.max_tokens = Some(parse_u32(value)?);
            }
            ["reasoning_effort"] => {
                config.agent.reasoning_effort = parse_research_effort(value)?;
            }
            ["max_tool_loops"] => {
                config.agent.max_tool_loops = parse_u32(value)?;
            }
            ["auto_retry"] => {
                config.agent.auto_retry = parse_bool(value)?;
            }
            ["parallel_tool_calls"] => {
                config.agent.parallel_tool_calls = parse_bool(value)?;
            }
            _ => {
                return Err(anyhow::anyhow!("Unknown agent configuration path: {:?}", path));
            }
        }
        Ok(())
    }

    /// Apply MCP-related overrides
    fn apply_mcp_override(config: &mut VTCodeConfig, path: &[&str], value: &str) -> Result<()> {
        match path {
            ["enabled"] => {
                config.mcp.enabled = parse_bool(value)?;
            }
            ["max_concurrent_connections"] => {
                config.mcp.max_concurrent_connections = parse_usize(value)?;
            }
            ["request_timeout_seconds"] => {
                config.mcp.request_timeout_seconds = parse_u64(value)?;
            }
            ["retry_attempts"] => {
                config.mcp.retry_attempts = parse_u32(value)?;
            }
            ["experimental_use_rmcp_client"] => {
                config.mcp.experimental_use_rmcp_client = parse_bool(value)?;
            }
            _ => {
                return Err(anyhow::anyhow!("Unknown MCP configuration path: {:?}", path));
            }
        }
        Ok(())
    }

    /// Apply tools-related overrides
    fn apply_tools_override(config: &mut VTCodeConfig, path: &[&str], value: &str) -> Result<()> {
        match path {
            ["command_timeout_seconds"] => {
                config.tools.command_timeout_seconds = parse_u64(value)?;
            }
            ["max_output_size"] => {
                config.tools.max_output_size = parse_usize(value)?;
            }
            ["max_concurrent_commands"] => {
                config.tools.max_concurrent_commands = parse_usize(value)?;
            }
            ["auto_approve_commands"] => {
                config.tools.auto_approve_commands = parse_bool(value)?;
            }
            _ => {
                return Err(anyhow::anyhow!("Unknown tools configuration path: {:?}", path));
            }
        }
        Ok(())
    }

    /// Apply UI-related overrides
    fn apply_ui_override(config: &mut VTCodeConfig, path: &[&str], value: &str) -> Result<()> {
        match path {
            ["mode"] => {
                config.ui.mode = parse_ui_mode(value)?;
            }
            ["show_provider_names"] => {
                config.ui.show_provider_names = parse_bool(value)?;
            }
            ["max_mcp_events"] => {
                config.ui.max_mcp_events = parse_usize(value)?;
            }
            _ => {
                return Err(anyhow::anyhow!("Unknown UI configuration path: {:?}", path));
            }
        }
        Ok(())
    }

    /// Apply debug-related overrides
    fn apply_debug_override(config: &mut VTCodeConfig, path: &[&str], value: &str) -> Result<()> {
        match path {
            ["enable_tracing"] => {
                config.debug.enable_tracing = parse_bool(value)?;
            }
            ["trace_level"] => {
                config.debug.trace_level = parse_trace_level(value)?;
            }
            ["trace_targets"] => {
                config.debug.trace_targets = value.split(',').map(|s| s.trim().to_string()).collect();
            }
            _ => {
                return Err(anyhow::anyhow!("Unknown debug configuration path: {:?}", path));
            }
        }
        Ok(())
    }

    /// Apply security-related overrides
    fn apply_security_override(config: &mut VTCodeConfig, path: &[&str], value: &str) -> Result<()> {
        match path {
            ["validation", "max_argument_size"] => {
                config.security.validation.max_argument_size = parse_u32(value)?;
            }
            ["validation", "path_traversal_protection"] => {
                config.security.validation.path_traversal_protection = parse_bool(value)?;
            }
            ["validation", "command_injection_protection"] => {
                config.security.validation.command_injection_protection = parse_bool(value)?;
            }
            _ => {
                return Err(anyhow::anyhow!("Unknown security configuration path: {:?}", path));
            }
        }
        Ok(())
    }

    /// Fallback to reflection-based override using JSON manipulation
    fn apply_reflection_override(config: &mut VTCodeConfig, path: &[&str], value: &str) -> Result<()> {
        // Convert to JSON for easier manipulation
        let mut json_value = serde_json::to_value(config)
            .context("Failed to serialize configuration to JSON")?;

        // Apply override to JSON
        apply_json_override(&mut json_value, path, value)?;

        // Convert back to config
        let updated: VTCodeConfig = serde_json::from_value(json_value)
            .context("Failed to deserialize configuration from JSON")?;

        *config = updated;
        Ok(())
    }

    /// Parse a boolean value
    fn parse_bool(value: &str) -> Result<bool> {
        value.parse::<bool>()
            .with_context(|| format!("Invalid boolean value: '{}'", value))
    }

    /// Parse an integer value
    fn parse_i64(value: &str) -> Result<i64> {
        value.parse::<i64>()
            .with_context(|| format!("Invalid integer value: '{}'", value))
    }

    /// Parse an unsigned integer value
    fn parse_u32(value: &str) -> Result<u32> {
        value.parse::<u32>()
            .with_context(|| format!("Invalid u32 value: '{}'", value))
    }

    /// Parse an unsigned integer value
    fn parse_u64(value: &str) -> Result<u64> {
        value.parse::<u64>()
            .with_context(|| format!("Invalid u64 value: '{}'", value))
    }

    /// Parse an unsigned integer value
    fn parse_usize(value: &str) -> Result<usize> {
        value.parse::<usize>()
            .with_context(|| format!("Invalid usize value: '{}'", value))
    }

    /// Parse a floating point value
    fn parse_f64(value: &str) -> Result<f64> {
        value.parse::<f64>()
            .with_context(|| format!("Invalid f64 value: '{}'", value))
    }

    /// Parse research effort level
    fn parse_research_effort(value: &str) -> Result<vtcode_config::types::ResearchEffortLevel> {
        vtcode_config::types::ResearchEffortLevel::from_str(value)
            .with_context(|| format!("Invalid research effort level: '{}'", value))
    }

    /// Parse trace level
    fn parse_trace_level(value: &str) -> Result<tracing::Level> {
        match value.to_lowercase().as_str() {
            "error" => Ok(tracing::Level::ERROR),
            "warn" => Ok(tracing::Level::WARN),
            "info" => Ok(tracing::Level::INFO),
            "debug" => Ok(tracing::Level::DEBUG),
            "trace" => Ok(tracing::Level::TRACE),
            _ => Err(anyhow::anyhow!("Invalid trace level: '{}'", value)),
        }
    }

    /// Parse UI mode
    fn parse_ui_mode(value: &str) -> Result<vtcode_config::mcp::McpUiMode> {
        vtcode_config::mcp::McpUiMode::from_str(value)
            .with_context(|| format!("Invalid UI mode: '{}'", value))
    }
}

/// Apply override to JSON value using path segments
fn apply_json_override(target: &mut serde_json::Value, path: &[&str], value: &str) -> Result<()> {
    let parsed_value = parse_json_value(value)?;
    
    let mut current = target;
    for (i, segment) in path.iter().enumerate() {
        if i == path.len() - 1 {
            // Last segment - set the value
            if let Some(obj) = current.as_object_mut() {
                obj.insert(segment.to_string(), parsed_value.clone());
            } else if let Some(arr) = current.as_array_mut() {
                if let Ok(index) = segment.parse::<usize>() {
                    if index < arr.len() {
                        arr[index] = parsed_value.clone();
                    } else {
                        return Err(anyhow::anyhow!("Array index out of bounds: {}", index));
                    }
                } else {
                    return Err(anyhow::anyhow!("Invalid array index: {}", segment));
                }
            } else {
                return Err(anyhow::anyhow!("Cannot set value on non-object/array"));
            }
        } else {
            // Navigate to the next level
            if let Some(obj) = current.as_object_mut() {
                if !obj.contains_key(*segment) {
                    obj.insert(segment.to_string(), serde_json::Value::Object(serde_json::Map::new()));
                }
                current = obj.get_mut(*segment).unwrap();
            } else {
                return Err(anyhow::anyhow!("Path segment '{}' not found", segment));
            }
        }
    }
    
    Ok(())
}

/// Parse a JSON value from string
fn parse_json_value(raw: &str) -> Result<serde_json::Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(serde_json::Value::String(String::new()));
    }

    // Try to parse as JSON first
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return Ok(json);
    }

    // Try to parse as TOML
    if let Ok(toml) = toml::from_str::<toml::Value>(trimmed) {
        return Ok(toml_to_json(&toml));
    }

    // Fall back to string
    Ok(serde_json::Value::String(trimmed.to_owned()))
}

/// Convert TOML value to JSON value
fn toml_to_json(toml: &toml::Value) -> serde_json::Value {
    match toml {
        toml::Value::String(s) => serde_json::Value::String(s.clone()),
        toml::Value::Integer(i) => serde_json::Value::Number((*i).into()),
        toml::Value::Float(f) => {
            if let Some(num) = serde_json::Number::from_f64(*f) {
                serde_json::Value::Number(num)
            } else {
                serde_json::Value::Null
            }
        }
        toml::Value::Boolean(b) => serde_json::Value::Bool(*b),
        toml::Value::Datetime(d) => serde_json::Value::String(d.to_string()),
        toml::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(toml_to_json).collect())
        }
        toml::Value::Table(table) => {
            let mut map = serde_json::Map::new();
            for (k, v) in table {
                map.insert(k.clone(), toml_to_json(v));
            }
            serde_json::Value::Object(map)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bool() {
        assert!(parse_bool("true").unwrap());
        assert!(!parse_bool("false").unwrap());
        assert!(parse_bool("TRUE").is_err());
    }

    #[test]
    fn test_parse_numeric() {
        assert_eq!(parse_u32("42").unwrap(), 42);
        assert_eq!(parse_f64("3.14").unwrap(), 3.14);
        assert!(parse_u32("not_a_number").is_err());
    }

    #[test]
    fn test_agent_override() {
        let mut config = VTCodeConfig::default();
        
        ConfigOptimizer::apply_agent_override(&mut config, &["provider"], "openai").unwrap();
        assert_eq!(config.agent.provider, "openai");
        
        ConfigOptimizer::apply_agent_override(&mut config, &["temperature"], "0.7").unwrap();
        assert_eq!(config.agent.temperature, Some(0.7));
    }

    #[test]
    fn test_json_override() {
        let mut json = serde_json::json!({
            "agent": {
                "provider": "gemini"
            }
        });
        
        apply_json_override(&mut json, &["agent", "provider"], "openai").unwrap();
        assert_eq!(json["agent"]["provider"], "openai");
    }
}