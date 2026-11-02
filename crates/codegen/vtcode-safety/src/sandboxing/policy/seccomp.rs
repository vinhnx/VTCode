use serde::{Deserialize, Serialize};

/// Version of the seccomp blocklist schema compiled into this binary.
///
/// Bump when `BLOCKED_SYSCALLS` gains or loses an entry so launcher logs
/// (see `linux_seccomp::apply_seccomp_filter`) can distinguish which policy
/// a trace came from.
pub const SECCOMP_PROFILE_VERSION: u32 = 1;

/// Syscalls that should be blocked in seccomp-bpf profiles.
///
/// Following the field guide: "A tight seccomp profile blocks syscalls that expand
/// kernel attack surface or enable escalation."
///
/// This is a blocklist (deny-list), not a whitelist. Per the sandboxing-basics
/// analysis (Emilua 2025), blocklists are inherently fragile: new kernel syscalls,
/// multiarch numberings, and the x32 ABI (`__X32_SYSCALL_BIT`) can bypass naive
/// filters. `linux_seccomp::primary_rules` therefore installs both the native
/// number and its x32-aliased variant on x86_64, and the launcher kills
/// mismatched architectures outright (`SECCOMP_RET_KILL_PROCESS` via
/// seccompiler arch validation). Prefer Landlock for filesystem policy and keep
/// this list focused on escalation/escape primitives.
///
/// Group labels mirror the Kafel-inspired families from that analysis
/// (`Debug`, `FilesystemHandle`, `IoUring`, `ProcessVm`, ...) so future
/// whitelist work can promote one family at a time.
pub const BLOCKED_SYSCALLS: &[&str] = &[
    // Debug/inspection - can be used to escape sandboxes or leak process state
    "ptrace",
    "kcmp",
    "pidfd_getfd",
    "process_madvise",
    "process_mrelease",
    // Mounting - can change filesystem namespace
    "mount",
    "umount",
    "umount2",
    // FilesystemHandle - file-handle escapes around path-based policy
    "open_by_handle_at",
    "name_to_handle_at",
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
    // IoUring - widely distrusted for untrusted code; see
    // https://security.googleblog.com/2023/06/learnings-from-kctf-vrps-42-linux.html
    "io_uring_setup",
    "io_uring_enter",
    "io_uring_register",
    // ProcessVm - cross-process memory inspection/writes
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
    blocked_syscalls: Vec<String>,

    /// Whether to allow new namespace creation (usually false for sandboxes).
    #[serde(default)]
    pub(super) allow_namespaces: bool,

    /// Whether to allow network socket creation (controlled separately).
    #[serde(default)]
    pub(super) allow_network_sockets: bool,

    /// Whether to log blocked syscalls instead of killing the process.
    #[serde(default)]
    log_only: bool,
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
    /// Syscalls this profile blocks outright.
    #[must_use]
    pub(crate) fn blocked_syscalls(&self) -> &[String] {
        &self.blocked_syscalls
    }

    /// Whether new namespace creation is permitted.
    #[must_use]
    pub(crate) fn allow_namespaces(&self) -> bool {
        self.allow_namespaces
    }

    /// Whether network socket creation is permitted.
    #[must_use]
    pub(crate) fn allow_network_sockets(&self) -> bool {
        self.allow_network_sockets
    }

    /// Whether the profile is log-only (no filter installed).
    #[must_use]
    pub(crate) fn log_only(&self) -> bool {
        self.log_only
    }

    /// Create a strict profile blocking all dangerous syscalls.
    #[must_use]
    pub fn strict() -> Self {
        Self {
            blocked_syscalls: default_blocked_syscalls(),
            allow_namespaces: false,
            allow_network_sockets: false,
            log_only: false,
        }
    }

    /// Create a permissive profile for semi-trusted code.
    #[must_use]
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
    #[must_use]
    pub fn logging() -> Self {
        Self {
            blocked_syscalls: default_blocked_syscalls(),
            allow_namespaces: false,
            allow_network_sockets: false,
            log_only: true,
        }
    }

    /// Builder: add a syscall to block.
    #[must_use]
    pub fn block_syscall(mut self, syscall: impl Into<String>) -> Self {
        let syscall = syscall.into();
        if !self.blocked_syscalls.contains(&syscall) {
            self.blocked_syscalls.push(syscall);
        }
        self
    }

    /// Builder: allow network sockets.
    #[must_use]
    pub fn with_network(mut self) -> Self {
        self.allow_network_sockets = true;
        self
    }

    /// Builder: enable log-only mode.
    #[must_use]
    pub fn with_logging(mut self) -> Self {
        self.log_only = true;
        self
    }

    /// Check if a syscall is blocked by this profile.
    #[inline]
    #[must_use]
    pub(super) fn is_blocked(&self, syscall: &str) -> bool {
        self.blocked_syscalls.iter().any(|s| s == syscall)
    }

    /// Generate a JSON representation for the sandbox helper.
    pub(crate) fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}
