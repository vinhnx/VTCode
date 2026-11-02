//! Linux kernel sandbox enforcement: Landlock filesystem restriction.
//!
//! This module is the enforcement half of [`SandboxType::LinuxLandlock`](super::SandboxType).
//! The main binary acts as the sandbox helper (busybox pattern):
//! [`super::SandboxManager`] wraps commands as
//! `vtcode sandbox-exec --sandbox-policy … -- <command>` (or an external helper
//! configured via `VTCODE_LINUX_SANDBOX_EXECUTABLE`), and this module applies
//! the restrictions to the launcher process before it execs the wrapped command.
//!
//! Enforcement model (mirrors the macOS Seatbelt profile in
//! [`super::SandboxManager`]):
//!
//! - **Reads**: allowed everywhere except sensitive credential paths
//!   (`~/.ssh`, cloud configs, …) and virtual filesystems (`/proc`, `/sys`).
//!   Landlock rules are additive grants, so the exclusion is implemented by
//!   enumerating the filesystem and granting every subtree that does not
//!   intersect a sensitive path. Directory *listings* of ungranted ancestors
//!   (`ls ~`, `ls /`) fail; files beneath granted subtrees stay readable.
//!   `/proc`/`/sys` stay ungranted so `/proc/self/fd` reopen-with-different-mode
//!   escapes stay denied (FDs-as-capabilities). `/dev` is never granted
//!   wholesale (`/dev/fd` → `/proc/self/fd`); only `/dev/null`, `/dev/zero`,
//!   and entropy sources are granted explicitly.
//! - **Writes**: denied everywhere except writable roots (workspace-write) or
//!   `/dev/null` (read-only). Unlike Seatbelt, Landlock has no deny rules, so
//!   `.git`/`.vtcode` inside writable roots cannot be subtracted at the kernel
//!   layer; that protection stays at the preflight/approval layer on Linux.
//! - **Execute**: unrestricted (not handled by the ruleset), matching the
//!   Seatbelt profile's broad `(allow process-exec)`.
//! - **Network**: enforced by the seccomp filter ([`super::linux_seccomp`]),
//!   not by Landlock, so block-all works on any Landlock-capable kernel.
//!
//! The ruleset is built for the exact ABI the kernel reports (probed via
//! `landlock_create_ruleset`), so the crate's best-effort compatibility layer
//! never silently downgrades a right we believed we were enforcing. Kernels
//! older than 5.13 (no Landlock) fail closed via [`landlock_supported`].

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};

use super::policy::{ResourceLimits, SandboxPolicy};

/// Sanity cap on generated Landlock rules; a well-formed policy needs at most
/// a few hundred. Exceeding it means the sensitive-path descent degenerated.
const MAX_LANDLOCK_RULES: usize = 4096;

/// Probe whether the running kernel enforces Landlock (ABI >= 1, Linux 5.13+).
///
/// Cached per process: `SandboxType::is_available` consults this on every
/// transform, and the launcher re-checks before applying restrictions.
pub fn landlock_supported() -> bool {
    static SUPPORTED: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| probe_landlock_abi().is_some());
    *SUPPORTED
}

/// Return the kernel's Landlock ABI version, or `None` when unsupported.
///
/// # Contract
///
/// - Ensures: `Some(version)` with `version >= 1` when the kernel enforces
///   Landlock (Linux 5.13+), and `None` for `ENOSYS`/`EOPNOTSUPP` or any other
///   `-errno` result. Absence of overflow: the raw `c_long` is narrowed with
///   `u32::try_from`, so a negative or oversized value maps to `None` instead
///   of truncating. Total and pure (one syscall, no allocation).
#[allow(
    unsafe_code,
    reason = "The Landlock ABI probe is a bare `landlock_create_ruleset` syscall; libc exposes no safe wrapper for it."
)]
fn probe_landlock_abi() -> Option<u32> {
    // landlock_create_ruleset(NULL, 0, LANDLOCK_CREATE_RULESET_VERSION)
    // returns the highest supported ABI version on success.
    const LANDLOCK_CREATE_RULESET_VERSION: libc::c_ulong = 1 << 0;
    // SAFETY: `libc::syscall` is invoked with the documented
    // `landlock_create_ruleset` version-probe arguments — a null ruleset
    // pointer, a zero size, and the `LANDLOCK_CREATE_RULESET_VERSION` flag.
    // With a null ruleset the kernel dereferences no user memory and writes
    // nothing back; it only returns a `c_long` (a non-negative ABI version, or
    // `-errno`), so no pointer-validity, aliasing, or initialization
    // precondition can be violated.
    let version = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            std::ptr::null::<libc::c_void>(),
            0usize,
            LANDLOCK_CREATE_RULESET_VERSION,
        )
    };
    if version < 0 { None } else { u32::try_from(version).ok() }
}

/// Apply every kernel-level restriction of the sandbox policy to the current
/// process: resource limits, then Landlock filesystem rules, then seccomp.
///
/// Called by the `sandbox-exec` launcher. Fails closed: any error means the
/// caller must not exec the wrapped command.
pub fn apply_sandbox_restrictions(
    policy: &SandboxPolicy,
    seccomp: &super::policy::SeccompProfile,
    limits: &ResourceLimits,
    policy_cwd: &Path,
) -> Result<()> {
    // Launcher-side defense in depth: a hostname allowlist is unenforceable
    // with Landlock/seccomp alone (BPF cannot inspect connect() destinations),
    // so a caller that somehow reached the launcher with one must not get
    // unrestricted network. Mirrors the transform-layer check.
    if policy.has_network_allowlist() {
        bail!(
            "hostname network allowlists cannot be enforced exactly by the Linux sandbox; refusing unrestricted network"
        );
    }
    apply_resource_limits(limits)?;
    apply_landlock(policy, policy_cwd)?;
    super::linux_seccomp::apply_seccomp_filter(seccomp)?;
    Ok(())
}

/// Apply explicitly configured resource limits. Zero values mean unlimited and
/// are skipped, so default policies impose no rlimits (Seatbelt parity).
fn apply_resource_limits(limits: &ResourceLimits) -> Result<()> {
    use nix::sys::resource::{Resource, setrlimit};

    let mib = |mb: u64| mb.saturating_mul(1024 * 1024);
    if limits.max_memory_mb > 0 {
        setrlimit(Resource::RLIMIT_AS, mib(limits.max_memory_mb), mib(limits.max_memory_mb))
            .map_err(|error| anyhow!("RLIMIT_AS failed: {error}"))?;
    }
    if limits.max_pids > 0 {
        let pids = u64::from(limits.max_pids);
        setrlimit(Resource::RLIMIT_NPROC, pids, pids).map_err(|error| anyhow!("RLIMIT_NPROC failed: {error}"))?;
    }
    if limits.max_disk_mb > 0 {
        setrlimit(Resource::RLIMIT_FSIZE, mib(limits.max_disk_mb), mib(limits.max_disk_mb))
            .map_err(|error| anyhow!("RLIMIT_FSIZE failed: {error}"))?;
    }
    if limits.cpu_time_secs > 0 {
        let secs = limits.cpu_time_secs;
        setrlimit(Resource::RLIMIT_CPU, secs, secs).map_err(|error| anyhow!("RLIMIT_CPU failed: {error}"))?;
    }
    Ok(())
}

/// Apply the Landlock filesystem restrictions of `policy` to this process.
pub fn apply_landlock(policy: &SandboxPolicy, policy_cwd: &Path) -> Result<()> {
    use landlock::{ABI, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreatedAttr, RulesetStatus};

    let Some(version) = probe_landlock_abi() else {
        bail!("Landlock is not supported by this kernel (Linux 5.13+ required); refusing to run unsandboxed");
    };
    // Map the probed kernel ABI to exactly the rights the kernel supports, so
    // the crate's BestEffort compat layer never silently downgrades anything.
    let abi = ABI::from(i32::try_from(version).unwrap_or(0));
    if abi == ABI::Unsupported {
        bail!("Landlock ABI version {version} is not usable");
    }

    let handled = handled_fs_access(abi);
    let rules = compute_rules(policy, policy_cwd, abi, handled)?;

    let mut created = Ruleset::default()
        .handle_access(handled)
        .map_err(|error| anyhow!("Landlock ruleset setup failed: {error}"))?
        .create()
        .map_err(|error| anyhow!("Landlock ruleset creation failed: {error}"))?;
    for rule in &rules {
        let fd = PathFd::new(&rule.path)
            .map_err(|error| anyhow!("Landlock cannot open rule path {}: {error}", rule.path.display()))?;
        created = created
            .add_rule(PathBeneath::new(fd, rule.access))
            .map_err(|error| anyhow!("Landlock rule for {} failed: {error}", rule.path.display()))?;
    }
    let status = created
        .restrict_self()
        .map_err(|error| anyhow!("Landlock self-restriction failed: {error}"))?;
    if status.ruleset != RulesetStatus::FullyEnforced {
        bail!("Landlock restrictions were only partially enforced ({:?}); refusing to exec", status.ruleset);
    }
    Ok(())
}

/// One Landlock `path_beneath` rule: grant `access` beneath `path`.
struct LandlockRule {
    path: PathBuf,
    access: landlock::BitFlags<landlock::AccessFs>,
}

/// Filesystem access rights this sandbox handles: everything the probed ABI
/// supports for read and write, minus two deliberate exclusions.
///
/// - `Execute` stays unhandled so exec paths remain unrestricted, matching the
///   Seatbelt profile's broad `(allow process-exec)` (`from_read` includes it).
/// - `IoctlDev` stays unhandled so PTY terminals keep working (`from_write`
///   includes it from ABI v5 on); writable-root grants never cover `/dev/pts`,
///   so handling it would deny TTY ioctls to every sandboxed command.
///
/// Rule grants must stay within this set (`add_rule` rejects rights the
/// ruleset does not handle), so callers intersect grant rights with the value
/// this function returns.
fn handled_fs_access(abi: landlock::ABI) -> landlock::BitFlags<landlock::AccessFs> {
    use landlock::{AccessFs, BitFlags};

    let abi_access: BitFlags<AccessFs> = AccessFs::from_read(abi) | AccessFs::from_write(abi);
    abi_access & !(AccessFs::Execute | AccessFs::IoctlDev)
}

/// Compute the full grant set for `policy`: read grants everywhere except
/// sensitive paths, plus write grants for writable roots (or `/dev/null`).
fn compute_rules(
    policy: &SandboxPolicy,
    policy_cwd: &Path,
    abi: landlock::ABI,
    handled: landlock::BitFlags<landlock::AccessFs>,
) -> Result<Vec<LandlockRule>> {
    let mut rules = Vec::new();
    for path in compute_read_rule_paths(policy, policy_cwd)? {
        rules.push(LandlockRule {
            path,
            access: landlock::AccessFs::from_read(abi) & handled,
        });
    }
    for path in compute_write_rule_paths(policy, policy_cwd) {
        rules.push(LandlockRule {
            path,
            access: landlock::AccessFs::from_write(abi) & handled,
        });
    }
    Ok(rules)
}

/// Virtual filesystems never granted read access.
///
/// Per the sandboxing-basics analysis, file descriptors cannot be modeled as
/// capabilities when a sandboxed process can reopen `/proc/self/fd/N` with a
/// different mode. Landlock grants are additive, so granting `/` or `/proc`
/// wholesale would re-admit that escape. `/proc` and `/sys` are excluded from
/// read grants entirely (fail closed for those subtrees); explicit device
/// nodes in [`EXPLICIT_DEV_READ_GRANTS`] cover the compatibility cases.
const VIRTUAL_FS_ROOTS: &[&str] = &["/proc", "/sys"];

/// Device nodes explicitly granted read access.
///
/// `/dev` is never granted wholesale: a wholesale grant would re-admit
/// `/dev/fd` (a symlink to `/proc/self/fd`) and sensitive device nodes.
/// These four are the compatibility set legacy code expects
/// (`/dev/null`, `/dev/zero`, entropy sources).
const EXPLICIT_DEV_READ_GRANTS: &[&str] = &["/dev/null", "/dev/zero", "/dev/urandom", "/dev/random"];

/// Effective sensitive paths with read blocking, expanded to absolute paths.
fn read_blocked_paths(policy: &SandboxPolicy, policy_cwd: &Path) -> Vec<PathBuf> {
    policy
        .sensitive_paths_for_execution(policy_cwd)
        .into_iter()
        .filter(|sp| sp.block_read)
        .map(|sp| sp.expand_path())
        .collect()
}

/// Compute the read-grant rule paths: enumerate the filesystem from `/` and
/// `$HOME`, granting every subtree that does not intersect a sensitive path.
///
/// Virtual filesystems (`/proc`, `/sys`) are always treated as exclusions so
/// `/proc/self/fd` reopen escapes stay denied, and `/dev` wholesale grants
/// are replaced with [`EXPLICIT_DEV_READ_GRANTS`].
fn compute_read_rule_paths(policy: &SandboxPolicy, policy_cwd: &Path) -> Result<Vec<PathBuf>> {
    let mut exclusions = read_blocked_paths(policy, policy_cwd);
    exclusions.extend(VIRTUAL_FS_ROOTS.iter().map(PathBuf::from));

    let mut roots = vec![PathBuf::from("/")];
    if let Some(home) = dirs::home_dir()
        && home != Path::new("/")
    {
        roots.push(home);
    }
    let mut grants = enumerate_read_grants(&roots, &exclusions)?;
    restrict_dev_grants(&mut grants);
    if grants.len() > MAX_LANDLOCK_RULES {
        bail!(
            "Landlock read enumeration produced {} rules (cap {MAX_LANDLOCK_RULES}); refusing to continue",
            grants.len()
        );
    }
    Ok(grants)
}

/// Replace wholesale `/dev` grants with explicit device-node grants.
///
/// A grant on `/dev` (or any path beneath it other than the explicit set)
/// would re-admit `/dev/fd` → `/proc/self/fd` and unrelated device nodes, so
/// those grants are dropped and only existing explicit nodes are kept.
fn restrict_dev_grants(grants: &mut Vec<PathBuf>) {
    let dev_root = Path::new("/dev");
    grants.retain(|path| {
        !path_within(path, dev_root) || EXPLICIT_DEV_READ_GRANTS.iter().any(|grant| path == Path::new(grant))
    });
    for candidate in EXPLICIT_DEV_READ_GRANTS {
        let path = PathBuf::from(candidate);
        if path.exists() && !grants.contains(&path) {
            grants.push(path);
        }
    }
}

/// Case-insensitive component-boundary containment: does `path` lie within
/// (or equal) `ancestor`?
fn path_within(path: &Path, ancestor: &Path) -> bool {
    super::policy::path_starts_with_case_insensitive(path, ancestor)
}

/// Enumerate read grants beneath `roots`, excluding `sensitive` subtrees.
///
/// Descends only along chains that lead to a sensitive path, so the grant set
/// stays small; everything else is granted wholesale as one directory rule.
/// Symlink entries are granted only when their canonical target neither is a
/// sensitive path nor lies *above* one (Landlock rule paths are opened with
/// `O_PATH`, which follows symlinks, so a grant on a link is a grant on its
/// target — and an ancestor grant would re-admit the excluded subtree beneath
/// it, since Landlock allows access granted by any ancestor rule).
fn enumerate_read_grants(roots: &[PathBuf], sensitive: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut grants = Vec::new();
    let mut queued: HashSet<PathBuf> = HashSet::new();
    let mut queue: VecDeque<PathBuf> = VecDeque::new();
    for root in roots {
        if queued.insert(root.clone()) {
            queue.push_back(root.clone());
        }
    }
    while let Some(dir) = queue.pop_front() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            // Unreadable directory: its children simply stay ungranted
            // (fail closed for that subtree).
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if sensitive.iter().any(|sp| path_within(&path, sp)) {
                continue;
            }
            let Ok(file_type) = entry.file_type() else { continue };
            if file_type.is_dir() {
                if sensitive.iter().any(|sp| path_within(sp, &path)) {
                    if queued.insert(path.clone()) {
                        queue.push_back(path);
                    }
                } else {
                    grants.push(path);
                }
            } else if file_type.is_symlink() {
                // Exclude targets that are sensitive OR ancestors of a
                // sensitive path: an ancestor grant would re-admit the
                // excluded subtree beneath it (e.g. a link to `$HOME` or `/`
                // would re-admit `~/.ssh`).
                if let Ok(target) = std::fs::canonicalize(&path)
                    && !sensitive.iter().any(|sp| path_within(&target, sp) || path_within(sp, &target))
                {
                    grants.push(path);
                }
            } else {
                grants.push(path);
            }
        }
    }
    Ok(grants)
}

/// Compute the write-grant rule paths for `policy`.
fn compute_write_rule_paths(policy: &SandboxPolicy, policy_cwd: &Path) -> Vec<PathBuf> {
    match policy {
        // Seatbelt parity: read-only policies may write only to /dev/null.
        SandboxPolicy::ReadOnly { .. } => vec![PathBuf::from("/dev/null")],
        SandboxPolicy::WorkspaceWrite { .. } => policy
            .get_writable_roots_with_cwd(policy_cwd)
            .into_iter()
            .map(|root| root.root)
            .collect(),
        // Only restrictive policies reach the Linux transform.
        SandboxPolicy::DangerFullAccess | SandboxPolicy::ExternalSandbox { .. } => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn sorted(mut paths: Vec<PathBuf>) -> Vec<String> {
        paths.sort();
        paths.into_iter().map(|p| p.display().to_string()).collect()
    }

    #[test]
    fn read_grants_exclude_sensitive_subtrees_and_files() {
        let root = TempDir::new().unwrap();
        let root = root.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join(".ssh")).unwrap();
        fs::create_dir_all(root.join("deep/with/.config/gcloud")).unwrap();
        fs::create_dir_all(root.join("deep/with/.config/git")).unwrap();
        fs::write(root.join("readme.md"), "x").unwrap();
        fs::write(root.join(".npmrc"), "token").unwrap();

        let sensitive = vec![
            root.join(".ssh"),
            root.join(".npmrc"),
            root.join("deep/with/.config/gcloud"),
        ];
        let grants = sorted(enumerate_read_grants(&[root.to_path_buf()], &sensitive).unwrap());

        assert!(grants.iter().any(|g| g.ends_with("src")), "wholesale dir grant: {grants:?}");
        assert!(grants.iter().any(|g| g.ends_with("readme.md")));
        // The .config level is descended into (gcloud beneath), so its other
        // children get wholesale grants.
        assert!(grants.iter().any(|g| g.ends_with(".config/git")));
        // Excluded: sensitive dirs/files and every ancestor that was descended.
        assert!(!grants.iter().any(|g| g.contains(".ssh")));
        assert!(!grants.iter().any(|g| g.contains(".npmrc")));
        assert!(!grants.iter().any(|g| g.contains("gcloud")));
        assert!(!grants.iter().any(|g| g.as_str() == root.display().to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn read_grants_exclude_symlinks_into_sensitive_paths() {
        let root = TempDir::new().unwrap();
        let root = root.path();
        fs::create_dir_all(root.join(".ssh")).unwrap();
        fs::create_dir_all(root.join("work")).unwrap();
        std::os::unix::fs::symlink(root.join(".ssh"), root.join("ssh-link")).unwrap();
        std::os::unix::fs::symlink(root.join("work"), root.join("work-link")).unwrap();

        let sensitive = vec![root.join(".ssh")];
        let grants = sorted(enumerate_read_grants(&[root.to_path_buf()], &sensitive).unwrap());

        assert!(
            !grants.iter().any(|g| g.ends_with("ssh-link")),
            "symlink into sensitive must be excluded: {grants:?}"
        );
        assert!(grants.iter().any(|g| g.ends_with("work")));
        assert!(grants.iter().any(|g| g.ends_with("work-link")));
    }

    #[cfg(unix)]
    #[test]
    fn read_grants_exclude_symlinks_to_sensitive_ancestors() {
        let root = TempDir::new().unwrap();
        let root = root.path();
        fs::create_dir_all(root.join(".ssh")).unwrap();
        fs::create_dir_all(root.join("work")).unwrap();
        // `root-link` resolves to the sensitive path's parent directory itself;
        // `parent-link` resolves to an even higher ancestor. Granting either
        // would re-admit `.ssh` beneath the target.
        std::os::unix::fs::symlink(root, root.join("root-link")).unwrap();
        std::os::unix::fs::symlink(root.parent().unwrap(), root.join("parent-link")).unwrap();
        // Positive control: a sibling subtree and a link into it stay granted.
        std::os::unix::fs::symlink(root.join("work"), root.join("work-link")).unwrap();

        let sensitive = vec![root.join(".ssh")];
        let grants = sorted(enumerate_read_grants(&[root.to_path_buf()], &sensitive).unwrap());

        assert!(
            !grants.iter().any(|g| g.ends_with("root-link") || g.ends_with("parent-link")),
            "symlink to a sensitive ancestor must be excluded: {grants:?}"
        );
        assert!(grants.iter().any(|g| g.ends_with("work")));
        assert!(grants.iter().any(|g| g.ends_with("work-link")));
    }

    #[test]
    fn handled_fs_access_excludes_execute_and_ioctl_dev() {
        use landlock::{ABI, AccessFs};

        let abis = [
            ABI::V1,
            ABI::V2,
            ABI::V3,
            ABI::V4,
            ABI::V5,
            ABI::V6,
            ABI::V7,
            ABI::V8,
            ABI::V9,
        ];
        for abi in abis {
            let handled = handled_fs_access(abi);
            assert!(!handled.contains(AccessFs::Execute), "Execute must stay unhandled at {abi:?}");
            assert!(!handled.contains(AccessFs::IoctlDev), "IoctlDev must stay unhandled at {abi:?}");
            assert!(handled.contains(AccessFs::ReadFile), "read handling lost at {abi:?}");
            assert!(handled.contains(AccessFs::WriteFile), "write handling lost at {abi:?}");
            // Only the two exclusions may be dropped: later-ABI rights such as
            // Truncate (v3+) must stay handled.
            if matches!(abi, ABI::V3 | ABI::V4 | ABI::V5 | ABI::V6 | ABI::V7 | ABI::V8 | ABI::V9) {
                assert!(handled.contains(AccessFs::Truncate), "Truncate must stay handled at {abi:?}");
            }
            // Rule grants are intersected with the handled set, so they must
            // never carry a right the ruleset does not handle (add_rule fails
            // on such rules).
            assert!(!(AccessFs::from_read(abi) & handled).contains(AccessFs::Execute));
            assert!(!(AccessFs::from_write(abi) & handled).contains(AccessFs::IoctlDev));
        }
    }

    #[test]
    fn write_grants_read_only_is_dev_null_only() {
        let paths = compute_write_rule_paths(&SandboxPolicy::read_only(), Path::new("/tmp"));
        assert_eq!(paths, vec![PathBuf::from("/dev/null")]);
    }

    #[test]
    fn restrict_dev_grants_replaces_wholesale_dev_with_explicit_nodes() {
        // Asymmetric oracle: wholesale /dev (and /dev/fd → /proc/self/fd)
        // must go; explicit compatibility nodes must stay.
        let mut grants = vec![
            PathBuf::from("/usr/bin"),
            PathBuf::from("/dev"),
            PathBuf::from("/dev/sda"),
            PathBuf::from("/dev/fd"),
            PathBuf::from("/dev/null"),
        ];
        restrict_dev_grants(&mut grants);

        assert!(!grants.iter().any(|g| g.as_os_str() == "/dev"), "wholesale /dev grant survives: {grants:?}");
        assert!(!grants.iter().any(|g| g.as_os_str() == "/dev/sda"), "device node grant survives: {grants:?}");
        assert!(!grants.iter().any(|g| g.as_os_str() == "/dev/fd"), "/dev/fd grant survives: {grants:?}");
        assert!(grants.iter().any(|g| g.as_os_str() == "/usr/bin"), "unrelated grant lost: {grants:?}");
        assert!(grants.iter().any(|g| g.as_os_str() == "/dev/null"), "explicit /dev/null grant lost: {grants:?}");
        for candidate in EXPLICIT_DEV_READ_GRANTS {
            let path = PathBuf::from(candidate);
            if path.exists() {
                assert!(grants.contains(&path), "existing explicit node {candidate} missing: {grants:?}");
            }
        }
    }

    #[test]
    fn virtual_fs_roots_cover_fd_reopen_escape() {
        assert!(VIRTUAL_FS_ROOTS.contains(&"/proc"), "proc exclusion lost: {VIRTUAL_FS_ROOTS:?}");
        assert!(VIRTUAL_FS_ROOTS.contains(&"/sys"), "sys exclusion lost: {VIRTUAL_FS_ROOTS:?}");
        assert!(
            EXPLICIT_DEV_READ_GRANTS.contains(&"/dev/null"),
            "dev/null compatibility grant lost: {EXPLICIT_DEV_READ_GRANTS:?}"
        );
        assert!(
            !EXPLICIT_DEV_READ_GRANTS.iter().any(|g| *g == "/dev/fd" || *g == "/dev"),
            "explicit grants must not re-admit /dev/fd: {EXPLICIT_DEV_READ_GRANTS:?}"
        );
    }

    #[test]
    fn write_grants_workspace_roots() {
        let workspace = TempDir::new().unwrap();
        let cwd = workspace.path().to_path_buf();
        let policy = SandboxPolicy::workspace_write(vec![cwd.clone()]);
        let paths = compute_write_rule_paths(&policy, &cwd);
        assert_eq!(paths, vec![cwd]);
    }

    #[test]
    fn probe_landlock_abi_is_none_or_positive() {
        // On Linux this exercises the real syscall; on other platforms this
        // test only guards the type contract.
        if let Some(version) = probe_landlock_abi() {
            assert!(version >= 1);
        }
    }

    #[test]
    fn apply_sandbox_restrictions_rejects_hostname_allowlists() {
        // The launcher-side fail-closed check fires before any kernel probe,
        // so it is exercised whenever the Linux module is compiled and run.
        let policy = SandboxPolicy::read_only_with_network(vec![super::super::policy::NetworkAllowlistEntry::https(
            "api.example.com",
        )]);
        let error = apply_sandbox_restrictions(
            &policy,
            &super::super::policy::SeccompProfile::strict(),
            &ResourceLimits::unlimited(),
            Path::new("/tmp"),
        )
        .expect_err("allowlist must fail closed at the launcher");
        assert!(error.to_string().contains("allowlist"), "got {error}");
    }
}
