//! Sandbox configuration for VT Code
//!
//! Implements configuration for the sandbox system following the AI sandbox field guide's
//! three-question model:
//! - **Boundary**: What is shared between code and host
//! - **Policy**: What can code touch (files, network, devices, syscalls)
//! - **Lifecycle**: What survives between runs

use crate::env_helpers::default_true;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

/// Sandbox configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SandboxConfig {
    /// Enable sandboxing for command execution
    #[serde(default = "default_false")]
    pub enabled: bool,

    /// Default sandbox policy
    #[serde(default)]
    pub default_policy: SandboxPolicy,

    /// Network egress configuration
    #[serde(default)]
    pub network: NetworkConfig,

    /// Sensitive path blocking configuration
    #[serde(default)]
    pub sensitive_paths: SensitivePathsConfig,

    /// Resource limits configuration
    #[serde(default)]
    pub resource_limits: ResourceLimitsConfig,

    /// Linux-specific seccomp configuration
    #[serde(default)]
    pub seccomp: SeccompConfig,

    /// External sandbox configuration (Docker, MicroVM, etc.)
    #[serde(default)]
    pub external: ExternalSandboxConfig,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: default_false(),
            default_policy: SandboxPolicy::default(),
            network: NetworkConfig::default(),
            sensitive_paths: SensitivePathsConfig::default(),
            resource_limits: ResourceLimitsConfig::default(),
            seccomp: SeccompConfig::default(),
            external: ExternalSandboxConfig::default(),
        }
    }
}

/// Sandbox policy following the Codex model
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SandboxPolicy {
    /// Read-only access - safest policy
    #[default]
    ReadOnly,
    /// Write access within workspace only
    WorkspaceWrite,
    /// Full access - dangerous, requires explicit approval
    DangerFullAccess,
    /// External sandbox (Docker, MicroVM)
    External,
}

/// Network egress policy
///
/// Replaces the legacy `allow_all`/`block_all` bool pair with a single
/// three-state enum. Config files using the old bool fields are still accepted
/// for backward compatibility.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NetworkPolicy {
    /// Use the domain allowlist for network egress (default-deny outbound).
    #[default]
    AllowlistOnly,
    /// Allow any network access (legacy `allow_all = true`).
    AllowAll,
    /// Block all network access, ignoring any allowlist (legacy `block_all = true`).
    BlockAll,
}

/// Network egress configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize)]
pub struct NetworkConfig {
    /// Network egress policy.
    /// Defaults to [`NetworkPolicy::AllowlistOnly`] (default-deny, then allowlist).
    pub policy: NetworkPolicy,

    /// Domain allowlist for network egress.
    /// Following field guide: "Default-deny outbound network, then allowlist."
    #[serde(default)]
    pub allowlist: Vec<NetworkAllowlistEntryConfig>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            policy: NetworkPolicy::AllowlistOnly,
            allowlist: Vec::new(),
        }
    }
}

impl<'de> Deserialize<'de> for NetworkConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            Policy,
            Allowlist,
            AllowAll,
            BlockAll,
        }

        struct NetworkConfigVisitor;

        impl<'de> Visitor<'de> for NetworkConfigVisitor {
            type Value = NetworkConfig;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("NetworkConfig struct")
            }

            fn visit_map<V>(self, mut map: V) -> Result<NetworkConfig, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut policy: Option<NetworkPolicy> = None;
                let mut allowlist: Option<Vec<NetworkAllowlistEntryConfig>> = None;
                let mut allow_all: Option<bool> = None;
                let mut block_all: Option<bool> = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Policy => {
                            if policy.is_some() {
                                return Err(de::Error::duplicate_field("policy"));
                            }
                            policy = Some(map.next_value()?);
                        }
                        Field::Allowlist => {
                            if allowlist.is_some() {
                                return Err(de::Error::duplicate_field("allowlist"));
                            }
                            allowlist = Some(map.next_value()?);
                        }
                        Field::AllowAll => {
                            if allow_all.is_some() {
                                return Err(de::Error::duplicate_field("allow_all"));
                            }
                            allow_all = Some(map.next_value()?);
                        }
                        Field::BlockAll => {
                            if block_all.is_some() {
                                return Err(de::Error::duplicate_field("block_all"));
                            }
                            block_all = Some(map.next_value()?);
                        }
                    }
                }

                // Legacy bool fields take precedence for backward compatibility:
                // block_all > allow_all > policy field > default (AllowlistOnly).
                let resolved_policy = if block_all.unwrap_or(false) {
                    NetworkPolicy::BlockAll
                } else if allow_all.unwrap_or(false) {
                    NetworkPolicy::AllowAll
                } else {
                    policy.unwrap_or_default()
                };

                Ok(NetworkConfig {
                    policy: resolved_policy,
                    allowlist: allowlist.unwrap_or_default(),
                })
            }
        }

        deserializer.deserialize_map(NetworkConfigVisitor)
    }
}

/// Network allowlist entry
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NetworkAllowlistEntryConfig {
    /// Domain pattern (e.g., "api.github.com", "*.npmjs.org")
    pub domain: String,
    /// Port (defaults to 443)
    #[serde(default = "default_https_port")]
    pub port: u16,
}

fn default_https_port() -> u16 {
    443
}

/// Sensitive paths configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SensitivePathsConfig {
    /// Use default sensitive paths (SSH, AWS, etc.)
    #[serde(default = "default_true")]
    pub use_defaults: bool,

    /// Additional paths to block
    #[serde(default)]
    pub additional: Vec<String>,

    /// Paths to explicitly allow (overrides defaults)
    #[serde(default)]
    pub exceptions: Vec<String>,
}

impl Default for SensitivePathsConfig {
    fn default() -> Self {
        Self {
            use_defaults: default_true(),
            additional: Vec::new(),
            exceptions: Vec::new(),
        }
    }
}

/// Resource limits configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ResourceLimitsConfig {
    /// Preset resource limits profile
    #[serde(default)]
    pub preset: ResourceLimitsPreset,

    /// Custom memory limit in MB (0 = use preset)
    #[serde(default)]
    pub max_memory_mb: u64,

    /// Custom max processes (0 = use preset)
    #[serde(default)]
    pub max_pids: u32,

    /// Custom disk write limit in MB (0 = use preset)
    #[serde(default)]
    pub max_disk_mb: u64,

    /// Custom CPU time limit in seconds (0 = use preset)
    #[serde(default)]
    pub cpu_time_secs: u64,

    /// Custom wall clock timeout in seconds (0 = use preset)
    #[serde(default)]
    pub timeout_secs: u64,
}

/// Resource limits preset
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceLimitsPreset {
    /// No limits
    Unlimited,
    /// Conservative limits for untrusted code
    Conservative,
    /// Moderate limits for semi-trusted code
    #[default]
    Moderate,
    /// Generous limits for trusted code
    Generous,
    /// Custom limits (use individual settings)
    Custom,
}

/// Linux seccomp configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SeccompConfig {
    /// Enable seccomp filtering (Linux only)
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Seccomp profile preset
    #[serde(default)]
    pub profile: SeccompProfilePreset,

    /// Additional syscalls to block
    #[serde(default)]
    pub additional_blocked: Vec<String>,

    /// Log blocked syscalls instead of killing process (for debugging)
    #[serde(default)]
    pub log_only: bool,
}

impl Default for SeccompConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            profile: SeccompProfilePreset::default(),
            additional_blocked: Vec::new(),
            log_only: false,
        }
    }
}

/// Seccomp profile preset
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SeccompProfilePreset {
    /// Strict profile - blocks most dangerous syscalls
    #[default]
    Strict,
    /// Permissive profile - only blocks critical syscalls
    Permissive,
    /// Disabled - no syscall filtering
    Disabled,
}

/// External sandbox configuration (Docker, MicroVM)
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ExternalSandboxConfig {
    /// Type of external sandbox
    #[serde(default)]
    pub sandbox_type: ExternalSandboxType,

    /// Docker-specific settings
    #[serde(default)]
    pub docker: DockerSandboxConfig,

    /// MicroVM-specific settings
    #[serde(default)]
    pub microvm: MicroVMSandboxConfig,
}

/// External sandbox type
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExternalSandboxType {
    /// No external sandbox
    #[default]
    None,
    /// Docker container
    Docker,
    /// MicroVM (Firecracker, cloud-hypervisor)
    MicroVM,
    /// gVisor container runtime
    GVisor,
}

/// Docker sandbox configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DockerSandboxConfig {
    /// Docker image to use
    #[serde(default = "default_docker_image")]
    pub image: String,

    /// Memory limit for container
    #[serde(default)]
    pub memory_limit: String,

    /// CPU limit for container
    #[serde(default)]
    pub cpu_limit: String,
}

fn default_docker_image() -> String {
    "ubuntu:22.04".to_string()
}

impl Default for DockerSandboxConfig {
    fn default() -> Self {
        Self {
            image: default_docker_image(),
            memory_limit: String::new(),
            cpu_limit: String::new(),
        }
    }
}

/// MicroVM provider (VMM) type
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum MicroVmProvider {
    /// No VMM configured
    #[default]
    #[serde(rename = "")]
    None,
    /// Firecracker VMM
    Firecracker,
    /// Cloud Hypervisor VMM
    CloudHypervisor,
    /// Forward-compatible catch-all for unknown VMM values
    #[serde(other)]
    Unknown,
}

impl MicroVmProvider {
    /// Returns the string representation of this VMM provider.
    pub fn as_str(&self) -> &str {
        match self {
            Self::None => "",
            Self::Firecracker => "firecracker",
            Self::CloudHypervisor => "cloud-hypervisor",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for MicroVmProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// MicroVM sandbox configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MicroVMSandboxConfig {
    /// VMM to use (firecracker, cloud-hypervisor)
    #[serde(default)]
    pub vmm: MicroVmProvider,

    /// Kernel image path
    #[serde(default)]
    pub kernel_path: String,

    /// Root filesystem path
    #[serde(default)]
    pub rootfs_path: String,

    /// Memory size in MB
    #[serde(default = "default_microvm_memory")]
    pub memory_mb: u64,

    /// Number of vCPUs
    #[serde(default = "default_vcpus")]
    pub vcpus: u32,
}

fn default_microvm_memory() -> u64 {
    512
}

fn default_vcpus() -> u32 {
    1
}

impl Default for MicroVMSandboxConfig {
    fn default() -> Self {
        Self {
            vmm: MicroVmProvider::None,
            kernel_path: String::new(),
            rootfs_path: String::new(),
            memory_mb: default_microvm_memory(),
            vcpus: default_vcpus(),
        }
    }
}

#[inline]
const fn default_false() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_config_default() {
        let config = SandboxConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.default_policy, SandboxPolicy::ReadOnly);
    }

    #[test]
    fn test_sandbox_config_parses_default_policy() {
        let config: SandboxConfig = toml::from_str(
            r#"
enabled = true
default_policy = "workspace_write"
"#,
        )
        .expect("sandbox config with default_policy should parse");

        assert!(config.enabled);
        assert_eq!(config.default_policy, SandboxPolicy::WorkspaceWrite);
    }

    #[test]
    fn test_sandbox_config_serializes_default_policy() {
        let config = SandboxConfig {
            default_policy: SandboxPolicy::DangerFullAccess,
            ..SandboxConfig::default()
        };

        let toml = toml::to_string(&config).expect("sandbox config should serialize");

        assert!(toml.contains("default_policy = \"danger_full_access\""));
        let removed_field = format!("default_{}", "mode");
        assert!(!toml.contains(&removed_field));
    }

    #[test]
    fn test_sandbox_config_ignores_unknown_fields_for_forward_compatibility() {
        // Unknown fields are silently ignored so that a config written by a newer
        // vtcode version does not break older binaries.
        let removed_field = format!("default_{}", "mode");
        let input = format!(
            r#"
enabled = true
{removed_field} = "workspace_write"
"#,
        );
        let config: SandboxConfig = toml::from_str(&input)
            .expect("sandbox config should accept unknown fields for forward compatibility");
        assert!(config.enabled);
    }

    #[test]
    fn test_network_config_default() {
        let config = NetworkConfig::default();
        assert_eq!(config.policy, NetworkPolicy::AllowlistOnly);
        assert!(config.allowlist.is_empty());
    }

    #[test]
    fn test_network_config_policy_field() {
        let config: NetworkConfig =
            toml::from_str(r#"policy = "allow_all""#).expect("policy field should parse");
        assert_eq!(config.policy, NetworkPolicy::AllowAll);
    }

    #[test]
    fn test_network_config_legacy_allow_all() {
        let config: NetworkConfig =
            toml::from_str(r#"allow_all = true"#).expect("legacy allow_all should parse");
        assert_eq!(config.policy, NetworkPolicy::AllowAll);
    }

    #[test]
    fn test_network_config_legacy_block_all() {
        let config: NetworkConfig =
            toml::from_str(r#"block_all = true"#).expect("legacy block_all should parse");
        assert_eq!(config.policy, NetworkPolicy::BlockAll);
    }

    #[test]
    fn test_network_config_legacy_block_all_overrides_allow_all() {
        let config: NetworkConfig = toml::from_str(
            r#"
allow_all = true
block_all = true
"#,
        )
        .expect("legacy bool combination should parse");
        assert_eq!(config.policy, NetworkPolicy::BlockAll);
    }

    #[test]
    fn test_resource_limits_config_default() {
        let config = ResourceLimitsConfig::default();
        assert_eq!(config.preset, ResourceLimitsPreset::Moderate);
    }

    #[test]
    fn test_seccomp_config_default() {
        let config = SeccompConfig::default();
        assert!(config.enabled);
        assert_eq!(config.profile, SeccompProfilePreset::Strict);
    }
}
