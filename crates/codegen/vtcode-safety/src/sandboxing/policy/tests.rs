use super::*;

#[test]
fn test_read_only_policy() {
    let policy = SandboxPolicy::read_only();
    assert!(!policy.has_full_network_access());
    assert!(!policy.has_network_allowlist());
    assert!(!policy.has_full_disk_write_access());
    assert!(policy.has_full_disk_read_access());
}

#[test]
fn test_read_only_with_network_allowlist() {
    let policy = SandboxPolicy::read_only_with_network(vec![
        NetworkAllowlistEntry::https("api.github.com"),
        NetworkAllowlistEntry::with_port("registry.npmjs.org", 443),
    ]);

    assert!(!policy.has_full_network_access());
    assert!(policy.has_network_allowlist());
    assert!(policy.is_network_allowed("api.github.com", 443));
    assert!(policy.is_network_allowed("registry.npmjs.org", 443));
    assert!(!policy.is_network_allowed("example.com", 443));
}

#[test]
fn test_read_only_with_full_network_access() {
    let policy = SandboxPolicy::read_only_with_full_network();

    assert!(policy.has_full_network_access());
    assert!(policy.is_network_allowed("example.com", 443));
    assert!(policy.seccomp_profile().allow_network_sockets);
}

#[test]
fn test_read_only_deserializes_legacy_shape() {
    let policy: SandboxPolicy = serde_json::from_str(r#"{"type":"read_only"}"#).expect("legacy read-only policy");

    assert_eq!(policy, SandboxPolicy::read_only());
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
fn test_workspace_write_protects_internal_metadata_dirs() {
    let cwd = PathBuf::from("/tmp/workspace");
    let policy = SandboxPolicy::workspace_write(vec![cwd.clone()]);

    assert!(!policy.is_path_writable(&cwd.join(".git/config"), &cwd));
    assert!(!policy.is_path_writable(&cwd.join(".vtcode/cache"), &cwd));
    assert!(!policy.is_path_writable(&cwd.join(".codex/state"), &cwd));
    assert!(!policy.is_path_writable(&cwd.join(".agents/skills"), &cwd));
    assert!(policy.is_path_writable(&cwd.join("src/main.rs"), &cwd));
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
    full.can_set(&read_only).unwrap();
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
    let policy = SandboxPolicy::workspace_write_with_network(vec![PathBuf::from("/tmp/workspace")], allowlist);

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

#[cfg(not(windows))]
#[test]
fn test_sensitive_path_matching_is_case_insensitive_with_component_boundaries() {
    let sp = SensitivePath::new("/tmp/Workspace/.env");

    assert!(sp.matches(Path::new("/tmp/workspace/.ENV")));
    assert!(sp.matches(Path::new("/tmp/workspace/.ENV/child")));
    assert!(!sp.matches(Path::new("/tmp/workspace/.environment")));
    assert!(!sp.matches(Path::new("/tmp/workspaces/.ENV")));
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
fn resolved_vtcode_roots_are_sensitive_without_duplicate_entries() {
    let environment = [
        ("HOME", "/home/tester"),
        ("VTCODE_CONFIG", "/vtcode/shared"),
        ("VTCODE_DATA", "/vtcode/shared"),
        ("XDG_STATE_HOME", "/xdg/state"),
        ("XDG_CACHE_HOME", "/xdg/cache"),
        ("XDG_RUNTIME_DIR", "/xdg/runtime"),
        ("XDG_BIN_HOME", "/xdg/bin"),
        ("VTCODE_HOME", "/legacy/vtcode"),
    ];
    let resolved = VtCodePaths::from_environment(&environment).expect("explicit absolute VT Code paths should resolve");
    let paths = vtcode_sensitive_paths(&environment).expect("explicit absolute VT Code paths should resolve");
    let path_strings: Vec<&str> = paths.iter().map(|path| path.path.as_str()).collect();

    for expected in [
        resolved.config_dir().to_path_buf(),
        resolved.auth_dir(),
        resolved.data_dir().to_path_buf(),
        resolved.state_dir().to_path_buf(),
        resolved.cache_dir().to_path_buf(),
        resolved.runtime_dir().to_path_buf(),
        resolved.executable_dir().to_path_buf(),
        resolved.legacy_dir().to_path_buf(),
    ] {
        let expected = expected.display().to_string();
        assert!(path_strings.contains(&expected.as_str()), "missing sensitive root: {expected}");
    }
    assert_eq!(path_strings.iter().filter(|path| **path == "/vtcode/shared").count(), 1);
}

#[test]
fn invalid_vtcode_path_resolution_is_rejected_before_policy_construction() {
    let error = vtcode_sensitive_paths(&[("HOME", "/home/tester"), ("VTCODE_CONFIG", "relative/config")])
        .expect_err("relative VT Code config paths must fail closed");
    assert!(error.to_string().contains("VTCODE_CONFIG"));
}

#[cfg(windows)]
#[test]
fn test_windows_userprofile_root_exclusions_are_in_defaults() {
    let paths = default_sensitive_paths();
    let path_strings: Vec<&str> = paths.iter().map(|p| p.path.as_str()).collect();

    for entry in USERPROFILE_READ_ROOT_EXCLUSIONS {
        let expected = format!("~/{}", entry);
        assert!(path_strings.contains(&expected.as_str()), "missing expected default sensitive path: {expected}");
    }
}

#[cfg(windows)]
#[test]
fn test_sensitive_path_matching_is_case_insensitive_on_windows() {
    let sp = SensitivePath::new("~/.aws");
    let home = dirs::home_dir().expect("home dir");
    let mixed_case_candidate = home.join(".AWS").join("credentials");

    assert!(sp.matches(&mixed_case_candidate));
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
    let policy = SandboxPolicy::workspace_write_no_sensitive_blocking(vec![PathBuf::from("/tmp")]);
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
    assert!(limits.has_limits());
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
    let policy = SandboxPolicy::workspace_write_with_limits(vec![PathBuf::from("/tmp/workspace")], limits.clone());

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
    let profile = SeccompProfile::strict().with_network().block_syscall("custom_syscall");
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
    // Verify key dangerous syscalls are in the list, including the
    // sandboxing-basics additions (ptrace-adjacent inspection, io_uring,
    // file-handle escapes). Asymmetric: both a classic entry and each new
    // family must be present.
    for must_block in [
        "ptrace",
        "kcmp",
        "pidfd_getfd",
        "mount",
        "open_by_handle_at",
        "name_to_handle_at",
        "kexec_load",
        "bpf",
        "perf_event_open",
        "userfaultfd",
        "io_uring_setup",
        "io_uring_enter",
        "io_uring_register",
        "process_vm_readv",
        "process_madvise",
        "unshare",
        "setns",
    ] {
        assert!(BLOCKED_SYSCALLS.contains(&must_block), "missing {must_block}");
    }
}
