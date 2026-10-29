//! Sandbox policy definitions
//!
//! Defines the isolation levels for command execution, following the Codex model.
//! Implements the "three-question model" from the AI sandbox field guide:
//! - **Boundary**: What is shared between code and host (kernel-enforced via Seatbelt/Landlock)
//! - **Policy**: What can code touch (files, network, devices, syscalls)
//! - **Lifecycle**: What survives between runs (session-scoped approvals)

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use vtcode_commons::VtCodePaths;

mod seccomp;

pub use seccomp::{BLOCKED_SYSCALLS, FILTERED_SYSCALLS, SECCOMP_PROFILE_VERSION, SeccompProfile};

/// A root directory that may be written to under the sandbox policy.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]

pub struct WritableRoot {
    /// Absolute path to the writable directory.
    pub root: PathBuf,
}

impl WritableRoot {
    /// Create a new writable root from a path.
    #[must_use]
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
    pub(crate) domain: String,
    /// Optional port (defaults to 443 for HTTPS)
    #[serde(default = "default_https_port")]
    pub(crate) port: u16,
    /// Protocol (tcp or udp, defaults to tcp)
    #[serde(default = "default_protocol")]
    pub(crate) protocol: String,
}

fn default_https_port() -> u16 {
    443
}

fn default_protocol() -> String {
    "tcp".to_string()
}

impl NetworkAllowlistEntry {
    /// Create a new allowlist entry for HTTPS access to a domain.
    #[must_use]
    pub fn https(domain: impl Into<String>) -> Self {
        Self {
            domain: domain.into(),
            port: 443,
            protocol: "tcp".to_string(),
        }
    }

    /// Create a new allowlist entry with custom port.
    #[must_use]
    pub fn with_port(domain: impl Into<String>, port: u16) -> Self {
        Self {
            domain: domain.into(),
            port,
            protocol: "tcp".to_string(),
        }
    }

    /// Check if a domain matches this entry (supports wildcard prefix).
    #[inline]
    fn matches(&self, domain: &str, port: u16) -> bool {
        if self.port != port {
            return false;
        }
        if self.domain.starts_with("*.") {
            let suffix = self.domain.get(1..).unwrap_or_default();
            let exact = self.domain.get(2..).unwrap_or_default();
            domain.ends_with(suffix) || domain == exact
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

#[cfg(windows)]
const USERPROFILE_READ_ROOT_EXCLUSIONS: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".aws",
    ".azure",
    ".kube",
    ".docker",
    ".config",
    ".npm",
    ".pki",
    ".terraform.d",
];

/// Sensitive path entry for blocking access to credential locations.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SensitivePath {
    /// Path pattern (supports ~ for home directory)
    path: String,
    /// Whether to block read access (true by default)
    #[serde(default = "default_true")]
    pub(crate) block_read: bool,
    /// Whether to block write access (true by default)
    #[serde(default = "default_true")]
    pub(crate) block_write: bool,
}

fn default_true() -> bool {
    true
}

impl SensitivePath {
    /// Create a new sensitive path entry that blocks both read and write.
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            block_read: true,
            block_write: true,
        }
    }

    /// Create a sensitive path entry that only blocks write access.
    #[must_use]
    fn write_only(path: impl Into<String>) -> Self {
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
            return home.join(self.path.get(2..).unwrap_or_default());
        } else if self.path == "~"
            && let Some(home) = dirs::home_dir()
        {
            return home;
        }
        PathBuf::from(&self.path)
    }

    /// Check if a given path matches this sensitive path pattern.
    pub(crate) fn matches(&self, path: &Path) -> bool {
        let expanded = self.expand_path();
        #[cfg(windows)]
        {
            let path_norm = normalize_windows_path(path);
            let expanded_norm = normalize_windows_path(&expanded);
            let mut expanded_prefix = expanded_norm.clone();
            if !expanded_prefix.ends_with('/') {
                expanded_prefix.push('/');
            }
            path_norm == expanded_norm || path_norm.starts_with(&expanded_prefix)
        }
        #[cfg(not(windows))]
        path_starts_with_case_insensitive(path, &expanded)
    }
}

#[cfg(not(windows))]
pub(crate) fn path_starts_with_case_insensitive(path: &Path, prefix: &Path) -> bool {
    let mut path_components = path.components();
    prefix.components().all(|prefix_component| {
        path_components.next().is_some_and(|path_component| {
            path_component
                .as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(prefix_component.as_os_str().to_string_lossy().as_ref())
        })
    })
}

#[cfg(windows)]
fn normalize_windows_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_ascii_lowercase()
}

/// Get the default sensitive paths as SensitivePath entries.
pub fn default_sensitive_paths() -> Vec<SensitivePath> {
    match vtcode_sensitive_paths(&[]) {
        Ok(paths) => paths,
        Err(error) => {
            tracing::warn!(%error, "VT Code path resolution failed; blocking absolute paths fail-closed");
            let mut paths: Vec<SensitivePath> =
                DEFAULT_SENSITIVE_PATHS.iter().map(|p| SensitivePath::new(*p)).collect();
            paths.push(SensitivePath::new("/"));
            paths
        }
    }
}

fn vtcode_sensitive_paths(environment: &[(&str, &str)]) -> anyhow::Result<Vec<SensitivePath>> {
    let resolved = if environment.is_empty() {
        VtCodePaths::resolve()?
    } else {
        VtCodePaths::from_environment(environment)?
    };
    let mut paths: Vec<SensitivePath> = DEFAULT_SENSITIVE_PATHS.iter().map(|p| SensitivePath::new(*p)).collect();
    let resolved_roots = [
        resolved.config_dir().to_path_buf(),
        resolved.auth_dir(),
        resolved.data_dir().to_path_buf(),
        resolved.state_dir().to_path_buf(),
        resolved.cache_dir().to_path_buf(),
        resolved.runtime_dir().to_path_buf(),
        resolved.executable_dir().to_path_buf(),
        resolved.legacy_dir().to_path_buf(),
    ];
    for root in resolved_roots {
        let path = root.display().to_string();
        if !paths.iter().any(|existing| existing.path == path) {
            paths.push(SensitivePath::new(path));
        }
    }

    #[cfg(windows)]
    {
        for entry in USERPROFILE_READ_ROOT_EXCLUSIONS {
            let path = format!("~/{}", entry);
            if !paths.iter().any(|existing| existing.path == path) {
                paths.push(SensitivePath::new(path));
            }
        }
        Ok(paths)
    }

    #[cfg(not(windows))]
    Ok(paths)
}

const PROTECTED_WRITABLE_ROOT_DIR_NAMES: &[&str] = &[".git", ".vtcode", ".codex", ".agents"];

fn protected_writable_root_sensitive_paths(writable_roots: &[WritableRoot]) -> Vec<SensitivePath> {
    let mut paths = Vec::new();

    for root in writable_roots {
        for dir_name in PROTECTED_WRITABLE_ROOT_DIR_NAMES {
            let protected_path = root.root.join(dir_name).display().to_string();
            if !paths.iter().any(|existing: &SensitivePath| {
                existing.path == protected_path && !existing.block_read && existing.block_write
            }) {
                paths.push(SensitivePath::write_only(protected_path));
            }
        }
    }

    paths
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
    #[must_use]
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
    #[must_use]
    pub fn conservative() -> Self {
        Self {
            max_memory_mb: 512,
            max_pids: 64,
            max_disk_mb: 1024,
            cpu_time_secs: 60,
            timeout_secs: 120,
        }
    }

    /// Create moderate limits for semi-trusted code.
    #[must_use]
    pub fn moderate() -> Self {
        Self {
            max_memory_mb: 2048,
            max_pids: 256,
            max_disk_mb: 4096,
            cpu_time_secs: 300,
            timeout_secs: 600,
        }
    }

    /// Create generous limits for trusted internal code.
    #[must_use]
    pub fn generous() -> Self {
        Self {
            max_memory_mb: 8192,
            max_pids: 1024,
            max_disk_mb: 16384,
            cpu_time_secs: 0,
            timeout_secs: 3600,
        }
    }

    /// Builder: set memory limit.
    #[must_use]
    fn with_memory_mb(mut self, mb: u64) -> Self {
        self.max_memory_mb = mb;
        self
    }

    /// Builder: set PID limit.
    #[must_use]
    fn with_max_pids(mut self, pids: u32) -> Self {
        self.max_pids = pids;
        self
    }

    /// Builder: set disk limit.
    #[must_use]
    pub fn with_disk_mb(mut self, mb: u64) -> Self {
        self.max_disk_mb = mb;
        self
    }

    /// Builder: set CPU time limit.
    #[must_use]
    pub fn with_cpu_time_secs(mut self, secs: u64) -> Self {
        self.cpu_time_secs = secs;
        self
    }

    /// Builder: set timeout.
    #[must_use]
    fn with_timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Check if any limits are set.
    #[inline]
    #[must_use]
    fn has_limits(&self) -> bool {
        self.max_memory_mb > 0
            || self.max_pids > 0
            || self.max_disk_mb > 0
            || self.cpu_time_secs > 0
            || self.timeout_secs > 0
    }

    /// Get the effective timeout in seconds.
    #[inline]
    #[must_use]
    fn effective_timeout_secs(&self) -> u64 {
        if self.timeout_secs > 0 { self.timeout_secs } else { 300 }
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
pub enum SandboxPolicy {
    /// No write access to the filesystem; network access may be restricted or allowlisted.
    ReadOnly {
        /// Whether network access is enabled when no allowlist is set.
        #[serde(default)]
        network_access: bool,

        /// Domain-based network egress allowlist.
        #[serde(default)]
        network_allowlist: Vec<NetworkAllowlistEntry>,
    },

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
    #[must_use]
    pub fn read_only() -> Self {
        Self::ReadOnly {
            network_access: false,
            network_allowlist: Vec::new(),
        }
    }

    /// Create a new read-only policy (alias for backwards compatibility).
    #[must_use]
    pub fn new_read_only_policy() -> Self {
        Self::read_only()
    }

    /// Create a read-only policy with a network allowlist.
    #[must_use]
    pub fn read_only_with_network(network_allowlist: Vec<NetworkAllowlistEntry>) -> Self {
        Self::ReadOnly {
            network_access: !network_allowlist.is_empty(),
            network_allowlist,
        }
    }

    /// Create a read-only policy with full network access.
    #[must_use]
    pub fn read_only_with_full_network() -> Self {
        Self::ReadOnly {
            network_access: true,
            network_allowlist: Vec::new(),
        }
    }

    /// Create a workspace-write policy with specified roots.
    /// Uses default sensitive path blocking and strict seccomp profile.
    #[must_use]
    pub fn workspace_write(writable_roots: Vec<PathBuf>) -> Self {
        Self::WorkspaceWrite {
            writable_roots: writable_roots.into_iter().map(WritableRoot::new).collect(),
            network_access: false,
            network_allowlist: Vec::new(),
            sensitive_paths: None,
            resource_limits: ResourceLimits::default(),
            seccomp_profile: SeccompProfile::strict(),
            exclude_tmpdir_env_var: true,
            exclude_slash_tmp: true,
        }
    }

    /// Create a workspace-write policy with network allowlist.
    #[must_use]
    fn workspace_write_with_network(
        writable_roots: Vec<PathBuf>,
        network_allowlist: Vec<NetworkAllowlistEntry>,
    ) -> Self {
        Self::WorkspaceWrite {
            writable_roots: writable_roots.into_iter().map(WritableRoot::new).collect(),
            network_access: !network_allowlist.is_empty(),
            network_allowlist,
            sensitive_paths: None,
            resource_limits: ResourceLimits::default(),
            seccomp_profile: SeccompProfile::strict().with_network(),
            exclude_tmpdir_env_var: true,
            exclude_slash_tmp: true,
        }
    }

    /// Create a workspace-write policy with custom sensitive path settings.
    #[must_use]
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
    #[must_use]
    fn workspace_write_no_sensitive_blocking(writable_roots: Vec<PathBuf>) -> Self {
        Self::WorkspaceWrite {
            writable_roots: writable_roots.into_iter().map(WritableRoot::new).collect(),
            network_access: false,
            network_allowlist: Vec::new(),
            sensitive_paths: Some(Vec::new()),
            resource_limits: ResourceLimits::default(),
            seccomp_profile: SeccompProfile::strict(),
            exclude_tmpdir_env_var: true,
            exclude_slash_tmp: true,
        }
    }

    /// Create a workspace-write policy with resource limits.
    /// Useful for untrusted code that needs containment.
    #[must_use]
    fn workspace_write_with_limits(writable_roots: Vec<PathBuf>, resource_limits: ResourceLimits) -> Self {
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
    #[must_use]
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
    #[must_use]
    pub fn full_access() -> Self {
        Self::DangerFullAccess
    }

    /// Check if the policy allows full network access (unrestricted).
    #[inline]
    #[must_use]
    pub fn has_full_network_access(&self) -> bool {
        match self {
            Self::ReadOnly { network_access, network_allowlist }
            | Self::WorkspaceWrite { network_access, network_allowlist, .. } => {
                *network_access && network_allowlist.is_empty()
            }
            Self::DangerFullAccess | Self::ExternalSandbox { .. } => true,
        }
    }

    /// Check if the policy has a network allowlist (domain-restricted access).
    #[inline]
    #[must_use]
    pub fn has_network_allowlist(&self) -> bool {
        match self {
            Self::ReadOnly { network_allowlist, .. } | Self::WorkspaceWrite { network_allowlist, .. } => {
                !network_allowlist.is_empty()
            }
            _ => false,
        }
    }

    /// Get the network allowlist entries, if any.
    #[inline]
    #[must_use]
    pub fn network_allowlist(&self) -> &[NetworkAllowlistEntry] {
        match self {
            Self::ReadOnly { network_allowlist, .. } | Self::WorkspaceWrite { network_allowlist, .. } => {
                network_allowlist
            }
            _ => &[],
        }
    }

    /// Check if network access to a specific domain:port is allowed.
    #[inline]
    #[must_use]
    pub fn is_network_allowed(&self, domain: &str, port: u16) -> bool {
        match self {
            Self::ReadOnly { network_access, network_allowlist }
            | Self::WorkspaceWrite { network_access, network_allowlist, .. } => {
                if network_allowlist.is_empty() {
                    *network_access
                } else {
                    network_allowlist.iter().any(|entry| entry.matches(domain, port))
                }
            }
            Self::DangerFullAccess | Self::ExternalSandbox { .. } => true,
        }
    }

    /// Get the effective sensitive paths to block.
    /// Returns default paths if not explicitly configured.
    #[must_use]
    fn sensitive_paths(&self) -> Vec<SensitivePath> {
        match self {
            Self::ReadOnly { .. } => default_sensitive_paths(),
            Self::WorkspaceWrite { sensitive_paths, .. } => {
                sensitive_paths.clone().unwrap_or_else(default_sensitive_paths)
            }
            Self::DangerFullAccess | Self::ExternalSandbox { .. } => Vec::new(),
        }
    }

    /// Get sensitive paths including write-only protected directories for writable roots.
    #[must_use]
    pub(crate) fn sensitive_paths_for_execution(&self, cwd: &Path) -> Vec<SensitivePath> {
        match self {
            Self::WorkspaceWrite { .. } => {
                let mut sensitive_paths = self.sensitive_paths();
                sensitive_paths.extend(protected_writable_root_sensitive_paths(&self.get_writable_roots_with_cwd(cwd)));
                sensitive_paths
            }
            _ => self.sensitive_paths(),
        }
    }

    /// Check if a path is a sensitive location that should be blocked.
    #[inline]
    #[must_use]
    fn is_sensitive_path(&self, path: &Path) -> bool {
        self.sensitive_paths().iter().any(|sp| sp.matches(path) && sp.block_read)
    }

    /// Check if write access to a path is blocked under this policy.
    #[inline]
    #[must_use]
    fn is_path_write_blocked(&self, path: &Path, cwd: &Path) -> bool {
        match self {
            Self::DangerFullAccess | Self::ExternalSandbox { .. } => false,
            _ => self
                .sensitive_paths_for_execution(cwd)
                .iter()
                .any(|sp| sp.matches(path) && sp.block_write),
        }
    }

    /// Check if read access to a path is allowed under this policy.
    #[inline]
    #[must_use]
    pub fn is_path_readable(&self, path: &Path) -> bool {
        match self {
            Self::DangerFullAccess | Self::ExternalSandbox { .. } => true,
            _ => !self.is_sensitive_path(path),
        }
    }

    /// Get the resource limits for this policy.
    #[must_use]
    pub fn resource_limits(&self) -> ResourceLimits {
        match self {
            Self::ReadOnly { .. } => ResourceLimits::conservative(),
            Self::WorkspaceWrite { resource_limits, .. } => resource_limits.clone(),
            Self::DangerFullAccess | Self::ExternalSandbox { .. } => ResourceLimits::unlimited(),
        }
    }

    /// Get the seccomp profile for this policy (Linux only).
    #[must_use]
    pub fn seccomp_profile(&self) -> SeccompProfile {
        match self {
            Self::ReadOnly { network_access, network_allowlist } => {
                let mut profile = SeccompProfile::strict();
                if *network_access || !network_allowlist.is_empty() {
                    profile = profile.with_network();
                }
                profile
            }
            Self::WorkspaceWrite { seccomp_profile, .. } => seccomp_profile.clone(),
            Self::DangerFullAccess | Self::ExternalSandbox { .. } => SeccompProfile::permissive(),
        }
    }

    /// Check if the policy allows full disk write access.
    #[inline]
    #[must_use]
    fn has_full_disk_write_access(&self) -> bool {
        matches!(self, Self::DangerFullAccess | Self::ExternalSandbox { .. })
    }

    /// Check if the policy allows full disk read access.
    #[inline]
    #[must_use]
    fn has_full_disk_read_access(&self) -> bool {
        true
    }

    /// Get the list of writable roots including the current working directory.
    #[must_use]
    pub(crate) fn get_writable_roots_with_cwd(&self, cwd: &Path) -> Vec<WritableRoot> {
        match self {
            Self::ReadOnly { .. } => vec![],
            Self::WorkspaceWrite { writable_roots, .. } => {
                let mut roots = writable_roots.clone();
                let cwd_root = WritableRoot::new(cwd);
                if !roots.contains(&cwd_root) {
                    roots.push(cwd_root);
                }
                roots
            }
            Self::DangerFullAccess | Self::ExternalSandbox { .. } => {
                vec![WritableRoot::new(cwd)]
            }
        }
    }

    /// Check if a path is writable under this policy.
    #[inline]
    #[must_use]
    pub fn is_path_writable(&self, path: &Path, cwd: &Path) -> bool {
        match self {
            Self::ReadOnly { .. } => false,
            Self::WorkspaceWrite { .. } => {
                let writable = self.get_writable_roots_with_cwd(cwd);
                writable.iter().any(|root| path.starts_with(&root.root)) && !self.is_path_write_blocked(path, cwd)
            }
            Self::DangerFullAccess | Self::ExternalSandbox { .. } => true,
        }
    }

    /// Validate that another policy can be set from this one.
    /// Used to enforce policy escalation restrictions.
    fn can_set(&self, new_policy: &SandboxPolicy) -> anyhow::Result<()> {
        use SandboxPolicy::*;

        match (self, new_policy) {
            // Can always downgrade
            (DangerFullAccess, _) => Ok(()),
            // Cannot escalate from ReadOnly to write-capable
            (ReadOnly { .. }, WorkspaceWrite { .. } | DangerFullAccess) => {
                Err(anyhow::anyhow!("cannot escalate from read-only to write-capable policy"))
            }
            // Other transitions are allowed
            _ => Ok(()),
        }
    }

    /// Get a human-readable description of the policy.
    pub fn description(&self) -> &'static str {
        match self {
            Self::ReadOnly { .. } => "read-only access",
            Self::WorkspaceWrite { .. } => "workspace write access",
            Self::DangerFullAccess => "full access (dangerous)",
            Self::ExternalSandbox { .. } => "external sandbox",
        }
    }
}

impl Default for SandboxPolicy {
    fn default() -> Self {
        Self::read_only()
    }
}

#[cfg(test)]
mod tests;
