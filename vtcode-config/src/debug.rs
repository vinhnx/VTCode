//! Debug and tracing configuration

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Trace level for structured logging
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TraceLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl TraceLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "error" => Some(Self::Error),
            "warn" => Some(Self::Warn),
            "info" => Some(Self::Info),
            "debug" => Some(Self::Debug),
            "trace" => Some(Self::Trace),
            _ => None,
        }
    }
}

impl Default for TraceLevel {
    fn default() -> Self {
        Self::Info
    }
}

impl std::fmt::Display for TraceLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for TraceLevel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        if let Some(parsed) = Self::parse(&raw) {
            Ok(parsed)
        } else {
            tracing::warn!(
                input = raw,
                "Invalid trace level; falling back to default (info)"
            );
            Ok(Self::default())
        }
    }
}

/// Debug and tracing configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DebugConfig {
    /// Enable structured logging for development and troubleshooting
    #[serde(default)]
    pub enable_tracing: bool,

    /// Trace level (error, warn, info, debug, trace)
    #[serde(default)]
    pub trace_level: TraceLevel,

    /// List of tracing targets to enable
    /// Examples: "vtcode_core::agent", "vtcode_core::tools", "vtcode::*"
    #[serde(default)]
    pub trace_targets: Vec<String>,

    /// Directory for debug logs
    #[serde(default)]
    pub debug_log_dir: Option<String>,

    /// Maximum size of debug logs before rotating (in MB)
    #[serde(default = "default_max_debug_log_size_mb")]
    pub max_debug_log_size_mb: u64,

    /// Maximum age of debug logs to keep (in days)
    #[serde(default = "default_max_debug_log_age_days")]
    pub max_debug_log_age_days: u32,
}

fn default_max_debug_log_size_mb() -> u64 {
    50
}

fn default_max_debug_log_age_days() -> u32 {
    7
}

impl Default for DebugConfig {
    fn default() -> Self {
        Self {
            enable_tracing: false,
            trace_level: TraceLevel::Info,
            trace_targets: Vec::new(),
            debug_log_dir: None,
            max_debug_log_size_mb: 50,
            max_debug_log_age_days: 7,
        }
    }
}

impl DebugConfig {
    /// Get the debug log directory, expanding ~ to home directory
    pub fn debug_log_path(&self) -> PathBuf {
        self.debug_log_dir
            .as_ref()
            .and_then(|dir| {
                if dir.starts_with("~") {
                    dirs::home_dir()
                        .map(|home| home.join(dir.trim_start_matches('~').trim_start_matches('/')))
                } else {
                    Some(PathBuf::from(dir))
                }
            })
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .map(|home| home.join(".vtcode/debug"))
                    .unwrap_or_else(|| PathBuf::from(".vtcode/debug"))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_level_parsing() {
        assert_eq!(TraceLevel::parse("error"), Some(TraceLevel::Error));
        assert_eq!(TraceLevel::parse("WARN"), Some(TraceLevel::Warn));
        assert_eq!(TraceLevel::parse("info"), Some(TraceLevel::Info));
        assert_eq!(TraceLevel::parse("DEBUG"), Some(TraceLevel::Debug));
        assert_eq!(TraceLevel::parse("trace"), Some(TraceLevel::Trace));
        assert_eq!(TraceLevel::parse("invalid"), None);
    }

    #[test]
    fn test_debug_config_default() {
        let cfg = DebugConfig::default();
        assert!(!cfg.enable_tracing);
        assert_eq!(cfg.trace_level, TraceLevel::Info);
        assert!(cfg.trace_targets.is_empty());
        assert_eq!(cfg.max_debug_log_size_mb, 50);
        assert_eq!(cfg.max_debug_log_age_days, 7);
    }
}
