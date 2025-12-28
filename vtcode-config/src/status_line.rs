use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum StatusLineMode {
    Auto,
    Command,
    Hidden,
}

impl Default for StatusLineMode {
    fn default() -> Self {
        StatusLineMode::Auto
    }
}

impl std::str::FromStr for StatusLineMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "auto" => Ok(StatusLineMode::Auto),
            "command" => Ok(StatusLineMode::Command),
            "hidden" => Ok(StatusLineMode::Hidden),
            _ => Err(format!("Invalid status line mode: {}", s)),
        }
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StatusLineConfig {
    #[serde(default = "default_status_line_mode")]
    pub mode: StatusLineMode,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default = "default_status_line_refresh_interval_ms")]
    pub refresh_interval_ms: u64,
    #[serde(default = "default_status_line_command_timeout_ms")]
    pub command_timeout_ms: u64,
}

impl Default for StatusLineConfig {
    fn default() -> Self {
        Self {
            mode: default_status_line_mode(),
            command: None,
            refresh_interval_ms: default_status_line_refresh_interval_ms(),
            command_timeout_ms: default_status_line_command_timeout_ms(),
        }
    }
}

fn default_status_line_mode() -> StatusLineMode {
    StatusLineMode::Auto
}

fn default_status_line_refresh_interval_ms() -> u64 {
    crate::constants::ui::STATUS_LINE_REFRESH_INTERVAL_MS
}

fn default_status_line_command_timeout_ms() -> u64 {
    crate::constants::ui::STATUS_LINE_COMMAND_TIMEOUT_MS
}
