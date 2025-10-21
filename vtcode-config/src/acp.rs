use crate::constants::env::acp::AgentClientProtocolEnvKey;
use serde::{Deserialize, Serialize};

fn parse_env_bool(key: AgentClientProtocolEnvKey, default: bool) -> bool {
    std::env::var(key.as_str())
        .ok()
        .and_then(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "1" | "true" | "yes" | "on" => Some(true),
                "0" | "false" | "no" | "off" => Some(false),
                _ => None,
            }
        })
        .unwrap_or(default)
}

fn default_enabled() -> bool {
    parse_env_bool(AgentClientProtocolEnvKey::Enabled, false)
}

fn default_zed_enabled() -> bool {
    parse_env_bool(AgentClientProtocolEnvKey::ZedEnabled, default_enabled())
}

fn default_zed_tools_read_file_enabled() -> bool {
    parse_env_bool(AgentClientProtocolEnvKey::ZedToolsReadFileEnabled, true)
}

fn default_zed_tools_list_files_enabled() -> bool {
    parse_env_bool(AgentClientProtocolEnvKey::ZedToolsListFilesEnabled, true)
}

fn parse_env_trust_mode(
    key: AgentClientProtocolEnvKey,
    default: AgentClientProtocolZedWorkspaceTrustMode,
) -> AgentClientProtocolZedWorkspaceTrustMode {
    std::env::var(key.as_str())
        .ok()
        .and_then(|value| AgentClientProtocolZedWorkspaceTrustMode::from_env_value(&value))
        .unwrap_or(default)
}

fn default_zed_workspace_trust_mode() -> AgentClientProtocolZedWorkspaceTrustMode {
    parse_env_trust_mode(
        AgentClientProtocolEnvKey::ZedWorkspaceTrust,
        AgentClientProtocolZedWorkspaceTrustMode::FullAuto,
    )
}

fn default_transport() -> AgentClientProtocolTransport {
    AgentClientProtocolTransport::Stdio
}

/// Agent Client Protocol configuration root
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentClientProtocolConfig {
    /// Globally enable the ACP bridge
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Zed IDE integration settings
    #[serde(default)]
    pub zed: AgentClientProtocolZedConfig,
}

impl Default for AgentClientProtocolConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            zed: AgentClientProtocolZedConfig::default(),
        }
    }
}

/// Transport options supported by the ACP bridge
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentClientProtocolTransport {
    /// Communicate over stdio (spawned process model)
    Stdio,
}

impl Default for AgentClientProtocolTransport {
    fn default() -> Self {
        default_transport()
    }
}

/// Zed-specific configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentClientProtocolZedConfig {
    /// Enable Zed integration
    #[serde(default = "default_zed_enabled")]
    pub enabled: bool,

    /// Transport used to communicate with the Zed client
    #[serde(default = "default_transport")]
    pub transport: AgentClientProtocolTransport,

    /// Tool toggles exposed through the Zed bridge
    #[serde(default)]
    pub tools: AgentClientProtocolZedToolsConfig,

    /// Desired workspace trust level when running under ACP
    #[serde(default = "default_zed_workspace_trust_mode")]
    pub workspace_trust: AgentClientProtocolZedWorkspaceTrustMode,
}

impl Default for AgentClientProtocolZedConfig {
    fn default() -> Self {
        Self {
            enabled: default_zed_enabled(),
            transport: default_transport(),
            tools: AgentClientProtocolZedToolsConfig::default(),
            workspace_trust: default_zed_workspace_trust_mode(),
        }
    }
}

/// Zed bridge tool configuration flags
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentClientProtocolZedToolsConfig {
    /// Toggle the read_file function bridge
    #[serde(default = "default_zed_tools_read_file_enabled")]
    pub read_file: bool,

    /// Toggle the list_files function bridge
    #[serde(default = "default_zed_tools_list_files_enabled")]
    pub list_files: bool,
}

impl Default for AgentClientProtocolZedToolsConfig {
    fn default() -> Self {
        Self {
            read_file: default_zed_tools_read_file_enabled(),
            list_files: default_zed_tools_list_files_enabled(),
        }
    }
}

/// Workspace trust configuration for the Zed ACP bridge
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentClientProtocolZedWorkspaceTrustMode {
    /// Maintain full automation trust
    FullAuto,
    /// Restrict to tools policy safeguards
    ToolsPolicy,
}

impl AgentClientProtocolZedWorkspaceTrustMode {
    fn from_env_value(value: &str) -> Option<Self> {
        let normalized = value.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "full_auto" | "full-auto" | "full" => Some(Self::FullAuto),
            "tools_policy" | "tools-policy" | "tools" => Some(Self::ToolsPolicy),
            _ => None,
        }
    }

    /// Resolve the workspace trust level represented by this configuration.
    pub fn to_workspace_trust_level(self) -> WorkspaceTrustLevel {
        match self {
            Self::FullAuto => WorkspaceTrustLevel::FullAuto,
            Self::ToolsPolicy => WorkspaceTrustLevel::ToolsPolicy,
        }
    }
}

/// Workspace trust levels exposed through the Agent Client Protocol configuration.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceTrustLevel {
    ToolsPolicy,
    FullAuto,
}

impl Default for WorkspaceTrustLevel {
    fn default() -> Self {
        Self::ToolsPolicy
    }
}

impl std::fmt::Display for WorkspaceTrustLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkspaceTrustLevel::ToolsPolicy => write!(f, "tools policy"),
            WorkspaceTrustLevel::FullAuto => write!(f, "full auto"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_use_stdio_transport() {
        let cfg = AgentClientProtocolConfig::default();
        assert!(matches!(
            cfg.zed.transport,
            AgentClientProtocolTransport::Stdio
        ));
        assert!(cfg.zed.tools.read_file);
        assert!(cfg.zed.tools.list_files);
        assert!(matches!(
            cfg.zed.workspace_trust,
            AgentClientProtocolZedWorkspaceTrustMode::FullAuto
        ));
    }
}
