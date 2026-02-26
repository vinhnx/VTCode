//! Sandbox policy definitions
//!
//! Defines the isolation levels for command execution, following the Codex model.
//! Implements the "three-question model" from the AI sandbox field guide:
//! - **Boundary**: What is shared between code and host (kernel-enforced via Seatbelt/Landlock)
//! - **Policy**: What can code touch (files, network, devices, syscalls)
//! - **Lifecycle**: What survives between runs (session-scoped approvals)

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A root directory that may be written to under the sandbox policy.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WritableRoot {
    /// Absolute path to the writable directory.
    pub root: PathBuf,
}

impl WritableRoot {
    /// Create a new writable root from a path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { root: path.into() }
    }
}

/// Network allowlist entry for domain-based egress control.
///
/// Following the field guide's recommendation: "Default-deny outbound network, then allowlist."
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NetworkAllowlistEntry {
    /// Domain pattern (e.g., "api.github.com", "*.npmjs.org")
    pub domain: String,
    /// Optional port (defaults to 443 for HTTPS)
    #[serde(default = "default_https_port")]
    pub port: u16,
    /// Protocol (tcp or udp, defaults to tcp)
    #[serde(default = "default_protocol")]
    pub protocol: String,
}

fn default_https_port() -> u16 {
    443
}

fn default_protocol() -> String {
    "tcp".to_string()
}

impl NetworkAllowlistEntry {
    /// Create a new allowlist entry for HTTPS access to a domain.
    pub fn https(domain: impl Into<String>) -> Self {
        Self {
            domain: domain.into(),
            port: 443,
            protocol: "tcp".to_string(),
        }
    }

    /// Create a new allowlist entry with custom port.
    pub fn with_port(domain: impl Into<String>, port: u16) -> Self {
        Self {
            domain: domain.into(),
            port,
            protocol: "tcp".to_string(),
        }
    }

    /// Check if a domain matches this entry (supports wildcard prefix).
    pub fn matches(&self, domain: &str, port: u16) -> bool {
        if self.port != port {
            return false;
        }
        if self.domain.starts_with("*.") {
            let suffix = &self.domain[1..]; // Keep the dot
            domain.ends_with(suffix) || domain == &self.domain[2..]
        } else {
            domain == self.domain
        }
    }
}

/// Default sensitive paths that should be blocked from sandboxed processes.
///
/// Following the field guide's warning about "policy leakage":
/// "If your sandbox can read ~/.ssh or mount host volumes, it can leak credentials."
pub const DEFAULT_SENSITIVE_PATHS: &[&str] = &[
    // SSH keys and configuration
    "~/.ssh",
    // AWS credentials
    "~/.aws",
    // Google Cloud credentials
    "~/.config/gcloud",
    // Azure credentials
    "~/.azure",
    // Kubernetes config (contains cluster credentials)
    "~/.kube",
    // Docker config (may contain registry auth)
    "~/.docker",
    // NPM tokens
    "~/.npmrc",
    // PyPI tokens
    "~/.pypirc",
    // GitHub CLI tokens
    "~/.config/gh",
    // Generic secrets directory
    "~/.secrets",
    // Gnupg keys
    "~/.gnupg",
    // 1Password CLI
    "~/.config/op",
    // Vault tokens
    "~/.vault-token",
    // Terraform credentials
    "~/.terraform.d/credentials.tfrc.json",
    // Cargo registry tokens
    "~/.cargo/credentials.toml",
    // Git credentials
    "~/.git-credentials",
    // Netrc (may contain passwords)
    "~/.netrc",
];

/// Sensitive path entry for blocking access to credential locations.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SensitivePath {
    /// Path pattern (supports ~ for home directory)
    pub path: String,
    /// Whether to block read access (true by default)
    #[serde(default = "default_true")]
    pub block_read: bool,
    /// Whether to block write access (true by default)
    #[serde(default = "default_true")]
    pub block_write: bool,
}

fn default_true() -> bool {
    true
}

impl SensitivePath {
    /// Create a new sensitive path entry that blocks both read and write.
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            block_read: true,
            block_write: true,
        }
    }

    /// Create a sensitive path entry that only blocks write access.
    pub fn write_only(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            block_read: false,
            block_write: true,
        }
    }

    /// Expand ~ to the user's home directory.
    pub fn expand_path(&self) -> PathBuf {
        if self.path.starts_with("~/")
            && let Some(home) = dirs::home_dir()
        {
            return home.join(&self.path[2..]);
        } else if self.path == "~"
            && let Some(home) = dirs::home_dir()
        {
            return home;
        }
        PathBuf::from(&self.path)
    }

    /// Check if a given path matches this sensitive path pattern.
    pub fn matches(&self, path: &Path) -> bool {
        let expanded = self.expand_path();
        path.starts_with(&expanded)
    }
}

/// Get the default sensitive paths as SensitivePath entries.
pub fn default_sensitive_paths() -> Vec<SensitivePath> {
    DEFAULT_SENSITIVE_PATHS
        .iter()
        .map(|p| SensitivePath::new(*p))
        .collect()
}

/// Resource limits for sandboxed execution.
///
/// Following the field guide's recommendation for resource accounting:
/// "CPU, memory, disk, timeouts, and PIDs."
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Maximum memory usage in megabytes (0 = unlimited).
    #[serde(default)]
    pub max_memory_mb: u64,

    /// Maximum number of processes/threads (0 = unlimited).
    /// Prevents fork bombs.
    #[serde(default)]
    pub max_pids: u32,

    /// Maximum disk write in megabytes (0 = unlimited).
    #[serde(default)]
    pub max_disk_mb: u64,

    /// CPU time limit in seconds (0 = unlimited).
    #[serde(default)]
    pub cpu_time_secs: u64,

    /// Wall clock timeout in seconds (0 = use default).
    #[serde(default)]
    pub timeout_secs: u64,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_memory_mb: 0,  // Unlimited by default
            max_pids: 0,       // Unlimited by default
            max_disk_mb: 0,    // Unlimited by default
            cpu_time_secs: 0,  // Unlimited by default
            timeout_secs: 300, // 5 minute wall clock default
        }
    }
}

impl ResourceLimits {
    /// Create new resource limits with all values unlimited.
    pub fn unlimited() -> Self {
        Self {
            max_memory_mb: 0,
            max_pids: 0,
            max_disk_mb: 0,
            cpu_time_secs: 0,
            timeout_secs: 0,
        }
    }

    /// Create conservative limits suitable for untrusted code.
    /// Following field guide: "Resource limits: CPU, memory, disk, timeouts, and PIDs."
    pub fn conservative() -> Self {
        Self {
            max_memory_mb: 512, // 512MB memory
            max_pids: 64,       // 64 processes (prevents fork bombs)
            max_disk_mb: 1024,  // 1GB disk writes
            cpu_time_secs: 60,  // 1 minute CPU time
            timeout_secs: 120,  // 2 minute wall clock
        }
    }

    /// Create moderate limits for semi-trusted code.
    pub fn moderate() -> Self {
        Self {
            max_memory_mb: 2048, // 2GB memory
            max_pids: 256,       // 256 processes
            max_disk_mb: 4096,   // 4GB disk writes
            cpu_time_secs: 300,  // 5 minutes CPU time
            timeout_secs: 600,   // 10 minute wall clock
        }
    }

    /// Create generous limits for trusted internal code.
    pub fn generous() -> Self {
        Self {
            max_memory_mb: 8192, // 8GB memory
            max_pids: 1024,      // 1024 processes
            max_disk_mb: 16384,  // 16GB disk writes
            cpu_time_secs: 0,    // Unlimited CPU time
            timeout_secs: 3600,  // 1 hour wall clock
        }
    }

    /// Builder: set memory limit.
    pub fn with_memory_mb(mut self, mb: u64) -> Self {
        self.max_memory_mb = mb;
        self
    }

    /// Builder: set PID limit.
    pub fn with_max_pids(mut self, pids: u32) -> Self {
        self.max_pids = pids;
        self
    }

    /// Builder: set disk limit.
    pub fn with_disk_mb(mut self, mb: u64) -> Self {
        self.max_disk_mb = mb;
        self
    }

    /// Builder: set CPU time limit.
    pub fn with_cpu_time_secs(mut self, secs: u64) -> Self {
        self.cpu_time_secs = secs;
        self
    }

    /// Builder: set timeout.
    pub fn with_timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Check if any limits are set.
    pub fn has_limits(&self) -> bool {
        self.max_memory_mb > 0
            || self.max_pids > 0
            || self.max_disk_mb > 0
            || self.cpu_time_secs > 0
    }

    /// Get the effective timeout in seconds.
    pub fn effective_timeout_secs(&self) -> u64 {
        if self.timeout_secs > 0 {
            self.timeout_secs
        } else {
            300 // Default 5 minutes
        }
    }
}

/// Syscalls that should be blocked in seccomp-bpf profiles.
///
/// Following the field guide: "A tight seccomp profile blocks syscalls that expand
/// kernel attack surface or enable escalation."
pub const BLOCKED_SYSCALLS: &[&str] = &[
    // Debugging/tracing - can be used to escape sandboxes
    "ptrace",
    // Mounting - can change filesystem namespace
    "mount",
    "umount",
    "umount2",
    // Kernel module loading
    "init_module",
    "finit_module",
    "delete_module",
    // Kernel replacement
    "kexec_load",
    "kexec_file_load",
    // BPF - can be used for sandbox escape
    "bpf",
    // Performance events - information leakage risk
    "perf_event_open",
    // Userfaultfd - can be used for race conditions
    "userfaultfd",
    // Process VM operations
    "process_vm_readv",
    "process_vm_writev",
    // Reboot/power
    "reboot",
    // Swap manipulation
    "swapon",
    "swapoff",
    // System time manipulation
    "settimeofday",
    "clock_settime",
    "adjtimex",
    // Keyring manipulation
    "add_key",
    "request_key",
    "keyctl",
    // IO permission
    "ioperm",
    "iopl",
    // Raw I/O port access
    "iopl",
    // Acct - process accounting manipulation
    "acct",
    // Quota manipulation
    "quotactl",
    // Namespace creation (can bypass restrictions)
    "unshare",
    "setns",
    // Personality - can enable legacy modes
    "personality",
];

/// Syscalls that require argument filtering (not fully blocked).
pub const FILTERED_SYSCALLS: &[&str] = &[
    // clone/clone3: filter to prevent new namespaces
    "clone", "clone3", // ioctl: filter to block dangerous device ioctls
    "ioctl",  // prctl: filter to block dangerous operations
    "prctl",  // socket: filter to enforce network policy
    "socket",
];

/// Seccomp profile configuration for Linux sandboxing.
///
/// Used alongside Landlock for defense-in-depth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeccompProfile {
    /// Syscalls to block entirely.
    #[serde(default = "default_blocked_syscalls")]
    pub blocked_syscalls: Vec<String>,

    /// Whether to allow new namespace creation (usually false for sandboxes).
    #[serde(default)]
    pub allow_namespaces: bool,

    /// Whether to allow network socket creation (controlled separately).
    #[serde(default)]
    pub allow_network_sockets: bool,

    /// Whether to log blocked syscalls instead of killing the process.
    #[serde(default)]
    pub log_only: bool,
}

fn default_blocked_syscalls() -> Vec<String> {
    BLOCKED_SYSCALLS.iter().map(|s| s.to_string()).collect()
}

impl Default for SeccompProfile {
    fn default() -> Self {
        Self {
            blocked_syscalls: default_blocked_syscalls(),
            allow_namespaces: false,
            allow_network_sockets: false,
            log_only: false,
        }
    }
}

impl SeccompProfile {
    /// Create a strict profile blocking all dangerous syscalls.
    pub fn strict() -> Self {
        Self {
            blocked_syscalls: default_blocked_syscalls(),
            allow_namespaces: false,
            allow_network_sockets: false,
            log_only: false,
        }
    }

    /// Create a permissive profile for semi-trusted code.
    pub fn permissive() -> Self {
        Self {
            blocked_syscalls: vec![
                "ptrace".to_string(),
                "kexec_load".to_string(),
                "kexec_file_load".to_string(),
                "reboot".to_string(),
            ],
            allow_namespaces: false,
            allow_network_sockets: true,
            log_only: false,
        }
    }

    /// Create a logging-only profile for debugging.
    pub fn logging() -> Self {
        Self {
            blocked_syscalls: default_blocked_syscalls(),
            allow_namespaces: false,
            allow_network_sockets: false,
            log_only: true,
        }
    }

    /// Builder: add a syscall to block.
    pub fn block_syscall(mut self, syscall: impl Into<String>) -> Self {
        let syscall = syscall.into();
        if !self.blocked_syscalls.contains(&syscall) {
            self.blocked_syscalls.push(syscall);
        }
        self
    }

    /// Builder: allow network sockets.
    pub fn with_network(mut self) -> Self {
        self.allow_network_sockets = true;
        self
    }

    /// Builder: enable log-only mode.
    pub fn with_logging(mut self) -> Self {
        self.log_only = true;
        self
    }

    /// Check if a syscall is blocked by this profile.
    pub fn is_blocked(&self, syscall: &str) -> bool {
        self.blocked_syscalls.iter().any(|s| s == syscall)
    }

    /// Generate a JSON representation for the sandbox helper.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

/// Sandbox policy determining what operations are permitted during execution.
///
/// This follows the Codex sandboxing model with three main variants:
/// - **ReadOnly**: Only read operations allowed (safe for viewing files)
/// - **WorkspaceWrite**: Can write within specified directories
/// - **DangerFullAccess**: No restrictions (dangerous, requires explicit approval)
///
/// The field guide's three-question model:
/// 1. What is shared between this code and the host? (boundary)
/// 2. What can the code touch? (policy - this enum)
/// 3. What survives between runs? (lifecycle)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[derive(Default)]
pub enum SandboxPolicy {
    /// No write access to the filesystem; network access disabled.
    #[default]
    ReadOnly,

    /// Write access limited to the specified roots; network controlled by allowlist.
    WorkspaceWrite {
        /// Directories where write access is permitted.
        writable_roots: Vec<WritableRoot>,

        /// Whether network access is allowed (legacy boolean, use network_allowlist for fine-grained control).
        #[serde(default)]
        network_access: bool,

        /// Domain-based network egress allowlist.
        /// When non-empty, only connections to these destinations are permitted.
        /// Following field guide: "Default-deny outbound network, then allowlist."
        #[serde(default)]
        network_allowlist: Vec<NetworkAllowlistEntry>,

        /// Sensitive paths to block (credentials, SSH keys, cloud configs).
        /// Following field guide: prevents "policy leakage" of credentials.
        /// Defaults to DEFAULT_SENSITIVE_PATHS if None.
        #[serde(default)]
        sensitive_paths: Option<Vec<SensitivePath>>,

        /// Resource limits (memory, PIDs, disk, CPU).
        /// Following field guide: prevents fork bombs, memory exhaustion.
        #[serde(default)]
        resource_limits: ResourceLimits,

        /// Seccomp-BPF profile for Linux syscall filtering.
        /// Following field guide: "Landlock + seccomp is the recommended Linux pattern."
        #[serde(default)]
        seccomp_profile: SeccompProfile,

        /// Exclude the TMPDIR environment variable from writable roots.
        #[serde(default)]
        exclude_tmpdir_env_var: bool,

        /// Exclude /tmp from writable roots.
        #[serde(default)]
        exclude_slash_tmp: bool,
    },

    /// Full access - no sandbox restrictions applied.
    /// Use with extreme caution.
    DangerFullAccess,

    /// External sandbox - the caller is responsible for sandbox setup.
    ExternalSandbox {
        /// Description of the external sandbox mechanism.
        description: String,
    },
}

impl SandboxPolicy {
    /// Create a read-only policy.
    pub fn read_only() -> Self {
        Self::ReadOnly
    }

    /// Create a new read-only policy (alias for backwards compatibility).
    pub fn new_read_only_policy() -> Self {
        Self::ReadOnly
    }

    /// Create a workspace-write policy with specified roots.
    /// Uses default sensitive path blocking and strict seccomp profile.
    pub fn workspace_write(writable_roots: Vec<PathBuf>) -> Self {
        Self::WorkspaceWrite {
            writable_roots: writable_roots.into_iter().map(WritableRoot::new).collect(),
            network_access: false,
            network_allowlist: Vec::new(),
            sensitive_paths: None, // Uses defaults
            resource_limits: ResourceLimits::default(),
            seccomp_profile: SeccompProfile::strict(),
            exclude_tmpdir_env_var: true,
            exclude_slash_tmp: true,
        }
    }

    /// Create a workspace-write policy with network allowlist.
    pub fn workspace_write_with_network(
        writable_roots: Vec<PathBuf>,
        network_allowlist: Vec<NetworkAllowlistEntry>,
    ) -> Self {
        Self::WorkspaceWrite {
            writable_roots: writable_roots.into_iter().map(WritableRoot::new).collect(),
            network_access: !network_allowlist.is_empty(),
            network_allowlist,
            sensitive_paths: None, // Uses defaults
            resource_limits: ResourceLimits::default(),
            seccomp_profile: SeccompProfile::strict().with_network(),
            exclude_tmpdir_env_var: true,
            exclude_slash_tmp: true,
        }
    }

    /// Create a workspace-write policy with custom sensitive path settings.
    pub fn workspace_write_with_sensitive_paths(
        writable_roots: Vec<PathBuf>,
        sensitive_paths: Vec<SensitivePath>,
    ) -> Self {
        Self::WorkspaceWrite {
            writable_roots: writable_roots.into_iter().map(WritableRoot::new).collect(),
            network_access: false,
            network_allowlist: Vec::new(),
            sensitive_paths: Some(sensitive_paths),
            resource_limits: ResourceLimits::default(),
            seccomp_profile: SeccompProfile::strict(),
            exclude_tmpdir_env_var: true,
            exclude_slash_tmp: true,
        }
    }

    /// Create a workspace-write policy without sensitive path blocking (dangerous).
    pub fn workspace_write_no_sensitive_blocking(writable_roots: Vec<PathBuf>) -> Self {
        Self::WorkspaceWrite {
            writable_roots: writable_roots.into_iter().map(WritableRoot::new).collect(),
            network_access: false,
            network_allowlist: Vec::new(),
            sensitive_paths: Some(Vec::new()), // Explicitly empty
            resource_limits: ResourceLimits::default(),
            seccomp_profile: SeccompProfile::strict(),
            exclude_tmpdir_env_var: true,
            exclude_slash_tmp: true,
        }
    }

    /// Create a workspace-write policy with resource limits.
    /// Useful for untrusted code that needs containment.
    pub fn workspace_write_with_limits(
        writable_roots: Vec<PathBuf>,
        resource_limits: ResourceLimits,
    ) -> Self {
        Self::WorkspaceWrite {
            writable_roots: writable_roots.into_iter().map(WritableRoot::new).collect(),
            network_access: false,
            network_allowlist: Vec::new(),
            sensitive_paths: None,
            resource_limits,
            seccomp_profile: SeccompProfile::strict(),
            exclude_tmpdir_env_var: true,
            exclude_slash_tmp: true,
        }
    }

    /// Create a fully-configured workspace-write policy.
    pub fn workspace_write_full(
        writable_roots: Vec<PathBuf>,
        network_allowlist: Vec<NetworkAllowlistEntry>,
        sensitive_paths: Option<Vec<SensitivePath>>,
        resource_limits: ResourceLimits,
        seccomp_profile: SeccompProfile,
    ) -> Self {
        Self::WorkspaceWrite {
            writable_roots: writable_roots.into_iter().map(WritableRoot::new).collect(),
            network_access: !network_allowlist.is_empty(),
            network_allowlist,
            sensitive_paths,
            resource_limits,
            seccomp_profile,
            exclude_tmpdir_env_var: true,
            exclude_slash_tmp: true,
        }
    }

    /// Create a full-access policy (dangerous).
    pub fn full_access() -> Self {
        Self::DangerFullAccess
    }

    /// Check if the policy allows full network access (unrestricted).
    pub fn has_full_network_access(&self) -> bool {
        match self {
            Self::ReadOnly => false,
            Self::WorkspaceWrite {
                network_access,
                network_allowlist,
                ..
            } => *network_access && network_allowlist.is_empty(),
            Self::DangerFullAccess | Self::ExternalSandbox { .. } => true,
        }
    }

    /// Check if the policy has a network allowlist (domain-restricted access).
    pub fn has_network_allowlist(&self) -> bool {
        match self {
            Self::WorkspaceWrite {
                network_allowlist, ..
            } => !network_allowlist.is_empty(),
            _ => false,
        }
    }

    /// Get the network allowlist entries, if any.
    pub fn network_allowlist(&self) -> &[NetworkAllowlistEntry] {
        match self {
            Self::WorkspaceWrite {
                network_allowlist, ..
            } => network_allowlist,
            _ => &[],
        }
    }

    /// Check if network access to a specific domain:port is allowed.
    pub fn is_network_allowed(&self, domain: &str, port: u16) -> bool {
        match self {
            Self::ReadOnly => false,
            Self::WorkspaceWrite {
                network_access,
                network_allowlist,
                ..
            } => {
                if network_allowlist.is_empty() {
                    // Legacy behavior: binary network_access flag
                    *network_access
                } else {
                    // Allowlist-based access control
                    network_allowlist
                        .iter()
                        .any(|entry| entry.matches(domain, port))
                }
            }
            Self::DangerFullAccess | Self::ExternalSandbox { .. } => true,
        }
    }

    /// Get the effective sensitive paths to block.
    /// Returns default paths if not explicitly configured.
    pub fn sensitive_paths(&self) -> Vec<SensitivePath> {
        match self {
            Self::ReadOnly => default_sensitive_paths(),
            Self::WorkspaceWrite {
                sensitive_paths, ..
            } => sensitive_paths
                .clone()
                .unwrap_or_else(default_sensitive_paths),
            Self::DangerFullAccess | Self::ExternalSandbox { .. } => Vec::new(),
        }
    }

    /// Check if a path is a sensitive location that should be blocked.
    pub fn is_sensitive_path(&self, path: &Path) -> bool {
        self.sensitive_paths()
            .iter()
            .any(|sp| sp.matches(path) && sp.block_read)
    }

    /// Check if read access to a path is allowed under this policy.
    /// Returns false if the path is in the sensitive paths list.
    pub fn is_path_readable(&self, path: &Path) -> bool {
        match self {
            Self::DangerFullAccess | Self::ExternalSandbox { .. } => true,
            _ => !self.is_sensitive_path(path),
        }
    }

    /// Get the resource limits for this policy.
    pub fn resource_limits(&self) -> ResourceLimits {
        match self {
            Self::ReadOnly => ResourceLimits::conservative(),
            Self::WorkspaceWrite {
                resource_limits, ..
            } => resource_limits.clone(),
            Self::DangerFullAccess | Self::ExternalSandbox { .. } => ResourceLimits::unlimited(),
        }
    }

    /// Get the seccomp profile for this policy (Linux only).
    pub fn seccomp_profile(&self) -> SeccompProfile {
        match self {
            Self::ReadOnly => SeccompProfile::strict(),
            Self::WorkspaceWrite {
                seccomp_profile, ..
            } => seccomp_profile.clone(),
            Self::DangerFullAccess | Self::ExternalSandbox { .. } => SeccompProfile::permissive(),
        }
    }

    /// Check if the policy allows full disk write access.
    pub fn has_full_disk_write_access(&self) -> bool {
        matches!(self, Self::DangerFullAccess | Self::ExternalSandbox { .. })
    }

    /// Check if the policy allows full disk read access.
    pub fn has_full_disk_read_access(&self) -> bool {
        // All policies allow read access
        true
    }

    /// Get the list of writable roots including the current working directory.
    pub fn get_writable_roots_with_cwd(&self, cwd: &Path) -> Vec<WritableRoot> {
        match self {
            Self::ReadOnly => vec![],
            Self::WorkspaceWrite { writable_roots, .. } => {
                let mut roots = writable_roots.clone();
                // Add cwd if not already included
                let cwd_root = WritableRoot::new(cwd);
                if !roots.contains(&cwd_root) {
                    roots.push(cwd_root);
                }
                roots
            }
            Self::DangerFullAccess | Self::ExternalSandbox { .. } => {
                // Full access - return cwd as a formality
                vec![WritableRoot::new(cwd)]
            }
        }
    }

    /// Check if a path is writable under this policy.
    pub fn is_path_writable(&self, path: &Path, cwd: &Path) -> bool {
        match self {
            Self::ReadOnly => false,
            Self::WorkspaceWrite { .. } => {
                let writable = self.get_writable_roots_with_cwd(cwd);
                writable.iter().any(|root| path.starts_with(&root.root))
            }
            Self::DangerFullAccess | Self::ExternalSandbox { .. } => true,
        }
    }

    /// Validate that another policy can be set from this one.
    /// Used to enforce policy escalation restrictions.
    pub fn can_set(&self, new_policy: &SandboxPolicy) -> anyhow::Result<()> {
        use SandboxPolicy::*;

        match (self, new_policy) {
            // Can always downgrade
            (DangerFullAccess, _) => Ok(()),
            // Cannot escalate from ReadOnly to write-capable
            (ReadOnly, WorkspaceWrite { .. } | DangerFullAccess) => Err(anyhow::anyhow!(
                "cannot escalate from read-only to write-capable policy"
            )),
            // Other transitions are allowed
            _ => Ok(()),
        }
    }

    /// Get a human-readable description of the policy.
    pub fn description(&self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only access",
            Self::WorkspaceWrite { .. } => "workspace write access",
            Self::DangerFullAccess => "full access (dangerous)",
            Self::ExternalSandbox { .. } => "external sandbox",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_only_policy() {
        let policy = SandboxPolicy::read_only();
        assert!(!policy.has_full_network_access());
        assert!(!policy.has_full_disk_write_access());
        assert!(policy.has_full_disk_read_access());
    }

    #[test]
    fn test_workspace_write_policy() {
        let policy = SandboxPolicy::workspace_write(vec![PathBuf::from("/tmp/workspace")]);
        assert!(!policy.has_full_network_access());
        assert!(!policy.has_full_disk_write_access());

        let cwd = PathBuf::from("/tmp/workspace");
        assert!(policy.is_path_writable(&cwd, &cwd));
        assert!(!policy.is_path_writable(&PathBuf::from("/etc"), &cwd));
    }

    #[test]
    fn test_full_access_policy() {
        let policy = SandboxPolicy::full_access();
        assert!(policy.has_full_network_access());
        assert!(policy.has_full_disk_write_access());
    }

    #[test]
    fn test_policy_escalation() {
        let read_only = SandboxPolicy::read_only();
        let full = SandboxPolicy::full_access();

        // Cannot escalate from read-only
        assert!(read_only.can_set(&full).is_err());

        // Can downgrade from full
        assert!(full.can_set(&read_only).is_ok());
    }

    #[test]
    fn test_network_allowlist_entry_matching() {
        let entry = NetworkAllowlistEntry::https("api.github.com");
        assert!(entry.matches("api.github.com", 443));
        assert!(!entry.matches("api.github.com", 80));
        assert!(!entry.matches("github.com", 443));
    }

    #[test]
    fn test_network_allowlist_wildcard() {
        let entry = NetworkAllowlistEntry::https("*.npmjs.org");
        assert!(entry.matches("registry.npmjs.org", 443));
        assert!(entry.matches("npmjs.org", 443));
        assert!(!entry.matches("npmjs.org.evil.com", 443));
    }

    #[test]
    fn test_workspace_with_network_allowlist() {
        let allowlist = vec![
            NetworkAllowlistEntry::https("api.github.com"),
            NetworkAllowlistEntry::https("*.npmjs.org"),
        ];
        let policy = SandboxPolicy::workspace_write_with_network(
            vec![PathBuf::from("/tmp/workspace")],
            allowlist,
        );

        // Has allowlist, not full access
        assert!(!policy.has_full_network_access());
        assert!(policy.has_network_allowlist());

        // Domain checks
        assert!(policy.is_network_allowed("api.github.com", 443));
        assert!(policy.is_network_allowed("registry.npmjs.org", 443));
        assert!(!policy.is_network_allowed("evil.com", 443));
        assert!(!policy.is_network_allowed("api.github.com", 80));
    }

    #[test]
    fn test_workspace_no_network() {
        let policy = SandboxPolicy::workspace_write(vec![PathBuf::from("/tmp/workspace")]);

        assert!(!policy.has_full_network_access());
        assert!(!policy.has_network_allowlist());
        assert!(!policy.is_network_allowed("api.github.com", 443));
    }

    #[test]
    fn test_sensitive_path_expansion() {
        let sp = SensitivePath::new("~/.ssh");
        let expanded = sp.expand_path();
        // Should expand to home directory
        assert!(expanded.to_string_lossy().contains(".ssh"));
        assert!(!expanded.to_string_lossy().starts_with('~'));
    }

    #[test]
    fn test_sensitive_path_matching() {
        let sp = SensitivePath::new("~/.ssh");
        let expanded = sp.expand_path();
        let ssh_key = expanded.join("id_rsa");
        assert!(sp.matches(&ssh_key));
        assert!(sp.matches(&expanded));
    }

    #[test]
    fn test_default_sensitive_paths() {
        let paths = default_sensitive_paths();
        assert!(!paths.is_empty());
        // Should include common credential locations
        let path_strings: Vec<&str> = paths.iter().map(|p| p.path.as_str()).collect();
        assert!(path_strings.contains(&"~/.ssh"));
        assert!(path_strings.contains(&"~/.aws"));
        assert!(path_strings.contains(&"~/.kube"));
    }

    #[test]
    fn test_workspace_blocks_sensitive_by_default() {
        let policy = SandboxPolicy::workspace_write(vec![PathBuf::from("/tmp/workspace")]);
        let sensitive = policy.sensitive_paths();
        assert!(!sensitive.is_empty());

        // Check that SSH keys are blocked
        if let Some(home) = dirs::home_dir() {
            let ssh_path = home.join(".ssh").join("id_rsa");
            assert!(policy.is_sensitive_path(&ssh_path));
            assert!(!policy.is_path_readable(&ssh_path));
        }
    }

    #[test]
    fn test_workspace_no_sensitive_blocking() {
        let policy =
            SandboxPolicy::workspace_write_no_sensitive_blocking(vec![PathBuf::from("/tmp")]);
        let sensitive = policy.sensitive_paths();
        assert!(sensitive.is_empty());

        // Nothing should be blocked
        if let Some(home) = dirs::home_dir() {
            let ssh_path = home.join(".ssh").join("id_rsa");
            assert!(!policy.is_sensitive_path(&ssh_path));
            assert!(policy.is_path_readable(&ssh_path));
        }
    }

    #[test]
    fn test_full_access_no_sensitive_blocking() {
        let policy = SandboxPolicy::full_access();
        let sensitive = policy.sensitive_paths();
        assert!(sensitive.is_empty());

        // Full access should allow everything
        if let Some(home) = dirs::home_dir() {
            let ssh_path = home.join(".ssh").join("id_rsa");
            assert!(policy.is_path_readable(&ssh_path));
        }
    }

    #[test]
    fn test_resource_limits_default() {
        let limits = ResourceLimits::default();
        assert_eq!(limits.max_memory_mb, 0);
        assert_eq!(limits.max_pids, 0);
        assert_eq!(limits.timeout_secs, 300);
        assert!(!limits.has_limits());
    }

    #[test]
    fn test_resource_limits_conservative() {
        let limits = ResourceLimits::conservative();
        assert_eq!(limits.max_memory_mb, 512);
        assert_eq!(limits.max_pids, 64);
        assert_eq!(limits.cpu_time_secs, 60);
        assert!(limits.has_limits());
    }

    #[test]
    fn test_resource_limits_builder() {
        let limits = ResourceLimits::default()
            .with_memory_mb(1024)
            .with_max_pids(128)
            .with_timeout_secs(60);
        assert_eq!(limits.max_memory_mb, 1024);
        assert_eq!(limits.max_pids, 128);
        assert_eq!(limits.effective_timeout_secs(), 60);
    }

    #[test]
    fn test_workspace_with_limits() {
        let limits = ResourceLimits::conservative();
        let policy = SandboxPolicy::workspace_write_with_limits(
            vec![PathBuf::from("/tmp/workspace")],
            limits.clone(),
        );

        let policy_limits = policy.resource_limits();
        assert_eq!(policy_limits.max_memory_mb, limits.max_memory_mb);
        assert_eq!(policy_limits.max_pids, limits.max_pids);
    }

    #[test]
    fn test_read_only_conservative_limits() {
        let policy = SandboxPolicy::read_only();
        let limits = policy.resource_limits();
        // ReadOnly should get conservative limits
        assert!(limits.has_limits());
        assert_eq!(limits.max_memory_mb, 512);
    }

    #[test]
    fn test_full_access_unlimited() {
        let policy = SandboxPolicy::full_access();
        let limits = policy.resource_limits();
        // Full access should have no limits
        assert!(!limits.has_limits());
    }

    #[test]
    fn test_seccomp_profile_strict() {
        let profile = SeccompProfile::strict();
        assert!(profile.is_blocked("ptrace"));
        assert!(profile.is_blocked("mount"));
        assert!(profile.is_blocked("kexec_load"));
        assert!(profile.is_blocked("bpf"));
        assert!(!profile.allow_network_sockets);
        assert!(!profile.allow_namespaces);
    }

    #[test]
    fn test_seccomp_profile_permissive() {
        let profile = SeccompProfile::permissive();
        // Still blocks the most dangerous syscalls
        assert!(profile.is_blocked("ptrace"));
        assert!(profile.is_blocked("kexec_load"));
        // But allows network
        assert!(profile.allow_network_sockets);
    }

    #[test]
    fn test_seccomp_profile_builder() {
        let profile = SeccompProfile::strict()
            .with_network()
            .block_syscall("custom_syscall");
        assert!(profile.allow_network_sockets);
        assert!(profile.is_blocked("custom_syscall"));
    }

    #[test]
    fn test_workspace_seccomp_profile() {
        let policy = SandboxPolicy::workspace_write(vec![PathBuf::from("/tmp")]);
        let profile = policy.seccomp_profile();
        // Should get strict profile by default
        assert!(profile.is_blocked("ptrace"));
        assert!(profile.is_blocked("mount"));
    }

    #[test]
    fn test_workspace_with_network_seccomp() {
        let policy = SandboxPolicy::workspace_write_with_network(
            vec![PathBuf::from("/tmp")],
            vec![NetworkAllowlistEntry::https("api.github.com")],
        );
        let profile = policy.seccomp_profile();
        // Should allow network sockets when network is enabled
        assert!(profile.allow_network_sockets);
    }

    #[test]
    fn test_seccomp_profile_json() {
        let profile = SeccompProfile::strict();
        let json = profile.to_json().unwrap();
        assert!(json.contains("ptrace"));
        assert!(json.contains("blocked_syscalls"));
    }

    #[test]
    fn test_blocked_syscalls_constant() {
        // Verify key dangerous syscalls are in the list
        assert!(BLOCKED_SYSCALLS.contains(&"ptrace"));
        assert!(BLOCKED_SYSCALLS.contains(&"mount"));
        assert!(BLOCKED_SYSCALLS.contains(&"kexec_load"));
        assert!(BLOCKED_SYSCALLS.contains(&"bpf"));
        assert!(BLOCKED_SYSCALLS.contains(&"perf_event_open"));
    }
}
