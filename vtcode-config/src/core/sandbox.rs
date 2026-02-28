//! Sandbox configuration for VT Code
//!
//! Implements configuration for the sandbox system following the AI sandbox field guide's
//! three-question model:
//! - **Boundary**: What is shared between code and host
//! - **Policy**: What can code touch (files, network, devices, syscalls)
//! - **Lifecycle**: What survives between runs

use serde::{Deserialize, Serialize};

/// Sandbox configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SandboxConfig {
    /// Enable sandboxing for command execution
    #[serde(default = "default_false")]
    pub enabled: bool,

    /// Default sandbox mode
    #[serde(default)]
    pub default_mode: SandboxMode,

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
            default_mode: SandboxMode::default(),
            network: NetworkConfig::default(),
            sensitive_paths: SensitivePathsConfig::default(),
            resource_limits: ResourceLimitsConfig::default(),
            seccomp: SeccompConfig::default(),
            external: ExternalSandboxConfig::default(),
        }
    }
}

/// Sandbox mode following the Codex model
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SandboxMode {
    /// Read-only access - safest mode
    #[default]
    ReadOnly,
    /// Write access within workspace only
    WorkspaceWrite,
    /// Full access - dangerous, requires explicit approval
    DangerFullAccess,
    /// External sandbox (Docker, MicroVM)
    External,
}

/// Network egress configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct NetworkConfig {
    /// Allow any network access (legacy mode)
    #[serde(default)]
    pub allow_all: bool,

    /// Domain allowlist for network egress
    /// Following field guide: "Default-deny outbound network, then allowlist."
    #[serde(default)]
    pub allowlist: Vec<NetworkAllowlistEntryConfig>,

    /// Block all network access (overrides allowlist)
    #[serde(default)]
    pub block_all: bool,
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

    /// Network mode
    #[serde(default = "default_network_mode")]
    pub network_mode: String,
}

fn default_docker_image() -> String {
    "ubuntu:22.04".to_string()
}

fn default_network_mode() -> String {
    "none".to_string()
}

impl Default for DockerSandboxConfig {
    fn default() -> Self {
        Self {
            image: default_docker_image(),
            memory_limit: String::new(),
            cpu_limit: String::new(),
            network_mode: default_network_mode(),
        }
    }
}

/// MicroVM sandbox configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MicroVMSandboxConfig {
    /// VMM to use (firecracker, cloud-hypervisor)
    #[serde(default)]
    pub vmm: String,

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
            vmm: String::new(),
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

#[inline]
const fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_config_default() {
        let config = SandboxConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.default_mode, SandboxMode::ReadOnly);
    }

    #[test]
    fn test_network_config_default() {
        let config = NetworkConfig::default();
        assert!(!config.allow_all);
        assert!(!config.block_all);
        assert!(config.allowlist.is_empty());
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
