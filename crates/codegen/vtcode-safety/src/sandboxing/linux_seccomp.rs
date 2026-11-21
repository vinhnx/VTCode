//! seccomp-BPF syscall filtering for the Linux sandbox launcher.
//!
//! Translates the policy's [`SeccompProfile`](super::policy::SeccompProfile)
//! into BPF programs installed on the launcher process before it execs the
//! wrapped command. `PR_SET_NO_NEW_PRIVS` is set first so an unprivileged
//! process may install filters; filters survive `execve`.
//!
//! Enforcement:
//! - `blocked_syscalls` → `EPERM` outright (plus x32-aliased numbers on x86_64,
//!   so `__X32_SYSCALL_BIT` cannot bypass the blocklist).
//! - `clone`/`clone3` → namespace creation blocked unless `allow_namespaces`.
//!   `clone3` passes its flags behind a pointer that BPF cannot dereference,
//!   so it is blocked outright with `ENOSYS` — the errno glibc needs to fall
//!   back to plain `clone`, whose flags *are* filterable.
//! - `socket` → `AF_INET`/`AF_INET6` denied unless `allow_network_sockets`.
//!   Unix sockets stay available (Seatbelt parity: `(allow network* (local
//!   unix))`); the managed network proxy (future work) closes that gap.
//! - `ioctl` → `TIOCSTI`/`TIOCSCTTY` denied always. File descriptors are
//!   capabilities except for `ioctl`s: terminal injection via `TIOCSTI` is a
//!   classic sandbox escape, while general TTY `ioctl`s stay allowed so PTY
//!   sessions keep working (mirrors the Landlock `IoctlDev`-unhandled choice).
//!
//! Domain allowlists are not enforceable here (BPF cannot inspect
//! `connect()` destinations); they fail closed upstream, same as Seatbelt.
//!
//! Design note (sandboxing-basics): this is a blocklist for OS hardening, not
//! a Capsicum-style whitelist for discretionary privilege dropping. New kernel
//! syscalls, multiarch numbers, and x32 aliasing are the known fragile edges;
//! seccompiler kills mismatched `AUDIT_ARCH` values outright, and we duplicate
//! blocked numbers with `__X32_SYSCALL_BIT` on x86_64. Landlock remains the
//! filesystem boundary; seccomp remains defense-in-depth.

use std::collections::BTreeMap;

use anyhow::{Context, Result, anyhow};
use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter, SeccompRule, TargetArch,
};

use super::policy::{SECCOMP_PROFILE_VERSION, SeccompProfile};

// Syscall numbers absent from some supported architectures: legacy `umount`
// was replaced by `umount2` (`SYS_umount` exists only on 32-bit/legacy
// targets — none of x86_64, aarch64, riscv64 expose it in libc 0.2.189),
// and port-I/O (`iopl`, `ioperm`) is x86-only. Alias legacy `umount`
// profiles to `umount2` so they still block the real syscall.
const SYS_UMOUNT: Option<i64> = Some(libc::SYS_umount2);
#[cfg(target_arch = "x86_64")]
const SYS_IOPL: Option<i64> = Some(libc::SYS_iopl);
#[cfg(not(target_arch = "x86_64"))]
const SYS_IOPL: Option<i64> = None;
#[cfg(target_arch = "x86_64")]
const SYS_IOPERM: Option<i64> = Some(libc::SYS_ioperm);
#[cfg(not(target_arch = "x86_64"))]
const SYS_IOPERM: Option<i64> = None;

/// Namespace-creation clone flags blocked when `allow_namespaces` is false.
const NAMESPACE_CLONE_FLAGS: &[libc::c_int] = &[
    libc::CLONE_NEWNS,
    libc::CLONE_NEWCGROUP,
    libc::CLONE_NEWUTS,
    libc::CLONE_NEWIPC,
    libc::CLONE_NEWUSER,
    libc::CLONE_NEWPID,
    libc::CLONE_NEWNET,
];

/// x32 ABI alias bit: x86_64 and x32 share `AUDIT_ARCH_X86_64`, distinguished
/// only by this bit on the syscall number. A blocklist that ignores it can be
/// bypassed by issuing the x32-aliased number.
#[cfg(target_arch = "x86_64")]
const X32_SYSCALL_BIT: i64 = 0x4000_0000;

/// Terminal injection ioctls denied unconditionally (argument-filtered).
/// `TIOCSTI` pushes input into a TTY queue — a classic container/sandbox
/// escape. `TIOCSCTTY` steals the controlling terminal.
const TIOCSTI_REQUEST: u64 = 0x5412;
const TIOCSCTTY_REQUEST: u64 = 0x540E;

/// Install the seccomp filters described by `profile` on this process.
pub fn apply_seccomp_filter(profile: &SeccompProfile) -> Result<()> {
    if profile.log_only() {
        tracing::debug!("seccomp profile is log_only; installing no filter");
        return Ok(());
    }
    tracing::debug!(
        version = SECCOMP_PROFILE_VERSION,
        blocked = profile.blocked_syscalls().len(),
        "installing seccomp blocklist"
    );
    set_no_new_privs()?;

    let arch = target_arch().ok_or_else(|| anyhow!("seccomp filtering is unsupported on this architecture"))?;

    // Primary filter: blocklist + argument-filtered rules → EPERM.
    let primary = SeccompFilter::new(
        primary_rules(profile)?,
        SeccompAction::Allow,
        SeccompAction::Errno(u32::try_from(libc::EPERM).context("EPERM conversion")?),
        arch,
    )
    .map_err(|error| anyhow!("seccomp filter construction failed: {error}"))?;
    install(primary)?;

    // Separate filter so clone3 can return ENOSYS (glibc's fallback trigger)
    // while everything else returns EPERM.
    if !profile.allow_namespaces() {
        let mut clone3 = BTreeMap::new();
        let _ = clone3.insert(libc::SYS_clone3, vec![unconditional_rule()?]);
        #[cfg(target_arch = "x86_64")]
        {
            // clone3's flags hide behind a pointer BPF cannot dereference, so
            // the x32-aliased number needs its own entry here too.
            let _ = clone3.insert(libc::SYS_clone3 | X32_SYSCALL_BIT, vec![unconditional_rule()?]);
        }
        let filter = SeccompFilter::new(
            clone3,
            SeccompAction::Allow,
            SeccompAction::Errno(u32::try_from(libc::ENOSYS).context("ENOSYS conversion")?),
            arch,
        )
        .map_err(|error| anyhow!("seccomp clone3 filter construction failed: {error}"))?;
        install(filter)?;
    }
    Ok(())
}

fn install(filter: SeccompFilter) -> Result<()> {
    let program = BpfProgram::try_from(filter).map_err(|error| anyhow!("seccomp BPF compilation failed: {error}"))?;
    seccompiler::apply_filter(&program).map_err(|error| anyhow!("failed to install seccomp filter: {error}"))
}

/// Insert `rule` for syscall `nr`, plus its x32-aliased number on x86_64.
///
/// x86_64 and x32 share `AUDIT_ARCH_X86_64`, so without the alias entry the
/// same call issued via the x32 ABI would miss the filter entirely. Applies
/// to outright blocks and argument-filtered rules alike (`clone` flags,
/// `socket` domains, `ioctl` requests).
fn insert_rule(rules: &mut BTreeMap<i64, Vec<SeccompRule>>, nr: i64, rule: SeccompRule) {
    #[cfg(target_arch = "x86_64")]
    {
        rules.entry(nr).or_default().push(rule.clone());
        rules.entry(nr | X32_SYSCALL_BIT).or_default().push(rule);
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        rules.entry(nr).or_default().push(rule);
    }
}

/// seccompiler 0.5 rejects rules with zero conditions, so outright blocks use
/// a condition that always holds: `MaskedEq` with mask 0 compares nothing and
/// matches every argument set. Without this, every unconditional block fails
/// filter construction and the launcher fails closed on Linux.
fn unconditional_rule() -> Result<SeccompRule> {
    SeccompRule::new(vec![SeccompCondition::new(
        0,
        SeccompCmpArgLen::Dword,
        SeccompCmpOp::MaskedEq(0),
        0,
    )?])
    .map_err(|error| anyhow!("unconditional seccomp rule construction failed: {error}"))
}

/// Build the primary rule set: outright blocks, plus argument-filtered rules
/// for `clone` (namespace flags), `socket` (INET domains), and `ioctl`
/// (terminal injection requests).
fn primary_rules(profile: &SeccompProfile) -> Result<BTreeMap<i64, Vec<SeccompRule>>> {
    let mut rules: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();

    for name in profile.blocked_syscalls() {
        match syscall_number(name) {
            Some(nr) => insert_rule(&mut rules, nr, unconditional_rule()?),
            None => tracing::debug!(syscall = %name, "syscall absent on this architecture; not blocking"),
        }
    }

    if !profile.allow_namespaces() {
        // clone's flags argument is arg 0 on x86_64, aarch64, and riscv64.
        // MaskedEq(mask) with value == mask matches when all mask bits are
        // set, i.e. "this namespace flag is present"; rules are OR-bound.
        // The x32-aliased number shares the layout, so `insert_rule` covers
        // both.
        for flag in NAMESPACE_CLONE_FLAGS {
            let mask =
                u64::from(u32::try_from(*flag).map_err(|error| anyhow!("CLONE flag conversion failed: {error}"))?);
            insert_rule(
                &mut rules,
                libc::SYS_clone,
                SeccompRule::new(vec![SeccompCondition::new(
                    0,
                    SeccompCmpArgLen::Dword,
                    SeccompCmpOp::MaskedEq(mask),
                    mask,
                )?])?,
            );
        }
    }

    if !profile.allow_network_sockets() {
        for domain in [libc::AF_INET, libc::AF_INET6] {
            insert_rule(
                &mut rules,
                libc::SYS_socket,
                SeccompRule::new(vec![SeccompCondition::new(
                    0,
                    SeccompCmpArgLen::Dword,
                    SeccompCmpOp::Eq,
                    u64::try_from(domain).map_err(|error| anyhow!("socket domain conversion failed: {error}"))?,
                )?])?,
            );
        }
    }

    // Deny terminal injection ioctls while leaving other TTY ioctls alone so
    // PTY sessions keep working. `ioctl(fd, request, ...)` carries the request
    // in arg 1 on all supported arches.
    for request in [TIOCSTI_REQUEST, TIOCSCTTY_REQUEST] {
        insert_rule(
            &mut rules,
            libc::SYS_ioctl,
            SeccompRule::new(vec![SeccompCondition::new(
                1,
                SeccompCmpArgLen::Dword,
                SeccompCmpOp::Eq,
                request,
            )?])?,
        );
    }

    Ok(rules)
}

fn set_no_new_privs() -> Result<()> {
    nix::sys::prctl::set_no_new_privs().map_err(|error| anyhow!("PR_SET_NO_NEW_PRIVS failed: {error}"))
}

fn target_arch() -> Option<TargetArch> {
    #[cfg(target_arch = "x86_64")]
    {
        Some(TargetArch::x86_64)
    }
    #[cfg(target_arch = "aarch64")]
    {
        Some(TargetArch::aarch64)
    }
    #[cfg(target_arch = "riscv64")]
    {
        Some(TargetArch::riscv64)
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64", target_arch = "riscv64")))]
    {
        None
    }
}

/// Map a syscall name to its number, or `None` when absent on this
/// architecture (e.g. `iopl`/`ioperm` do not exist on aarch64).
fn syscall_number(name: &str) -> Option<i64> {
    match name {
        "ptrace" => Some(libc::SYS_ptrace),
        "kcmp" => Some(libc::SYS_kcmp),
        "pidfd_getfd" => Some(libc::SYS_pidfd_getfd),
        "process_madvise" => Some(libc::SYS_process_madvise),
        "process_mrelease" => Some(libc::SYS_process_mrelease),
        "mount" => Some(libc::SYS_mount),
        "umount" => SYS_UMOUNT,
        "umount2" => Some(libc::SYS_umount2),
        "open_by_handle_at" => Some(libc::SYS_open_by_handle_at),
        "name_to_handle_at" => Some(libc::SYS_name_to_handle_at),
        "init_module" => Some(libc::SYS_init_module),
        "finit_module" => Some(libc::SYS_finit_module),
        "delete_module" => Some(libc::SYS_delete_module),
        "kexec_load" => Some(libc::SYS_kexec_load),
        "kexec_file_load" => Some(libc::SYS_kexec_file_load),
        "bpf" => Some(libc::SYS_bpf),
        "perf_event_open" => Some(libc::SYS_perf_event_open),
        "userfaultfd" => Some(libc::SYS_userfaultfd),
        "io_uring_setup" => Some(libc::SYS_io_uring_setup),
        "io_uring_enter" => Some(libc::SYS_io_uring_enter),
        "io_uring_register" => Some(libc::SYS_io_uring_register),
        "process_vm_readv" => Some(libc::SYS_process_vm_readv),
        "process_vm_writev" => Some(libc::SYS_process_vm_writev),
        "reboot" => Some(libc::SYS_reboot),
        "swapon" => Some(libc::SYS_swapon),
        "swapoff" => Some(libc::SYS_swapoff),
        "settimeofday" => Some(libc::SYS_settimeofday),
        "clock_settime" => Some(libc::SYS_clock_settime),
        "adjtimex" => Some(libc::SYS_adjtimex),
        "add_key" => Some(libc::SYS_add_key),
        "request_key" => Some(libc::SYS_request_key),
        "keyctl" => Some(libc::SYS_keyctl),
        "ioperm" => SYS_IOPERM,
        "iopl" => SYS_IOPL,
        "acct" => Some(libc::SYS_acct),
        "quotactl" => Some(libc::SYS_quotactl),
        "unshare" => Some(libc::SYS_unshare),
        "setns" => Some(libc::SYS_setns),
        "personality" => Some(libc::SYS_personality),
        "clone" => Some(libc::SYS_clone),
        "clone3" => Some(libc::SYS_clone3),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_blocked_syscall_maps_to_a_number() {
        for name in super::super::policy::BLOCKED_SYSCALLS {
            // iopl/ioperm legitimately do not exist on non-x86_64.
            let x86_only = matches!(*name, "iopl" | "ioperm");
            if x86_only && !cfg!(target_arch = "x86_64") {
                continue;
            }
            assert!(syscall_number(name).is_some(), "BLOCKED_SYSCALLS entry {name:?} has no number mapping");
        }
    }

    #[test]
    fn blocked_syscalls_have_no_duplicates() {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        for name in super::super::policy::BLOCKED_SYSCALLS {
            assert!(seen.insert(*name), "duplicate BLOCKED_SYSCALLS entry {name:?}");
        }
    }

    #[test]
    fn strict_profile_blocks_escalation_primitives() {
        // Asymmetric oracle: must-block vs must-allow, not a snapshot of the
        // current list. Catches regressions that silently drop a family.
        let profile = SeccompProfile::strict();
        for must_block in [
            "ptrace",
            "kcmp",
            "pidfd_getfd",
            "bpf",
            "perf_event_open",
            "userfaultfd",
            "io_uring_setup",
            "io_uring_enter",
            "io_uring_register",
            "process_vm_readv",
            "process_vm_writev",
            "process_madvise",
            "mount",
            "open_by_handle_at",
            "name_to_handle_at",
            "unshare",
            "setns",
        ] {
            assert!(profile.blocked_syscalls().iter().any(|s| s == must_block), "strict profile lost {must_block}");
            assert!(syscall_number(must_block).is_some(), "no number mapping for {must_block}");
        }
    }

    #[test]
    fn primary_rules_block_everything_requested() {
        let profile = SeccompProfile::strict();
        let rules = primary_rules(&profile).unwrap();
        for name in super::super::policy::BLOCKED_SYSCALLS {
            let Some(nr) = syscall_number(name) else { continue };
            assert!(rules.contains_key(&nr), "syscall {name} missing from primary rules");
        }
        assert!(rules.contains_key(&libc::SYS_clone), "namespace-flag clone rules required");
        assert!(rules.contains_key(&libc::SYS_socket), "socket domain rules required");
        assert!(rules.contains_key(&libc::SYS_ioctl), "TIOCSTI/TIOCSCTTY ioctl rules required");
        // clone3 lives in the separate ENOSYS filter.
        assert!(!rules.contains_key(&libc::SYS_clone3));
    }

    #[test]
    fn ioctl_rules_target_terminal_injection_only() {
        let profile = SeccompProfile::strict();
        let rules = primary_rules(&profile).unwrap();
        let ioctl_rules = rules.get(&libc::SYS_ioctl).expect("ioctl rules required");
        // Two argument-filtered rules (TIOCSTI, TIOCSCTTY), not a blanket ioctl block:
        // PTY sessions need general TTY ioctls to keep working.
        assert_eq!(ioctl_rules.len(), 2, "expected exactly TIOCSTI + TIOCSCTTY rules, got {ioctl_rules:?}");
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn primary_rules_cover_x32_aliased_numbers() {
        let profile = SeccompProfile::strict();
        let rules = primary_rules(&profile).unwrap();
        for name in super::super::policy::BLOCKED_SYSCALLS {
            let Some(nr) = syscall_number(name) else { continue };
            assert!(
                rules.contains_key(&(nr | X32_SYSCALL_BIT)),
                "x32 alias for {name} (nr {nr}) missing; __X32_SYSCALL_BIT bypass possible"
            );
        }
        // Argument-filtered rules need the alias too: the x32-aliased number
        // is a different key, so an x32 clone/socket/ioctl would otherwise
        // miss the namespace, network, and terminal-injection filters.
        for (label, nr) in [
            ("clone", libc::SYS_clone),
            ("socket", libc::SYS_socket),
            ("ioctl", libc::SYS_ioctl),
        ] {
            assert!(
                rules.contains_key(&(nr | X32_SYSCALL_BIT)),
                "x32 alias for filtered {label} (nr {nr}) missing; bypass possible"
            );
        }
    }

    #[test]
    fn permissive_profile_keeps_network_sockets() {
        let profile = SeccompProfile::permissive();
        let rules = primary_rules(&profile).unwrap();
        assert!(!rules.contains_key(&libc::SYS_socket), "allow_network_sockets must not block socket()");
        // clone namespace filtering is still applied (allow_namespaces=false).
        assert!(rules.contains_key(&libc::SYS_clone));
        // Terminal injection is denied even in permissive mode.
        assert!(rules.contains_key(&libc::SYS_ioctl));
    }
}
