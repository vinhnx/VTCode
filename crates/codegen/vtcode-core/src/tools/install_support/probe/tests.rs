use super::*;

#[cfg(unix)]
fn shell(script: &str) -> Command {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", script, "version-probe-test"]);
    command
}

#[cfg(unix)]
#[test]
fn probe_preserves_asymmetric_streams_and_nonzero_exit() {
    let output = run_probe_with_limits(
        shell("printf 'tool 1.2'; printf 'unsupported flag' >&2; exit 7"),
        Duration::from_secs(2),
        64,
    )
    .unwrap();
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(output.stdout, b"tool 1.2");
    assert_eq!(output.stderr, b"unsupported flag");
}

#[cfg(unix)]
#[test]
fn probe_allows_exact_cap_and_rejects_each_overflow() {
    let output = run_probe_with_limits(shell("printf abc; printf XYZ >&2"), Duration::from_secs(2), 3).unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"abc");
    assert_eq!(output.stderr, b"XYZ");
    for (script, stream) in [("printf abcd", "stdout"), ("printf x; printf WXYZ >&2", "stderr")] {
        let error = run_probe_with_limits(shell(script), Duration::from_secs(2), 3).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains(stream));
    }
    let empty = run_probe_with_limits(shell(":"), Duration::from_secs(2), 0).unwrap();
    assert!(empty.stdout.is_empty() && empty.stderr.is_empty());
}

#[test]
fn probe_preserves_missing_executable_error_kind() {
    let directory = tempfile::tempdir().unwrap();
    let error = run_version_probe(Command::new(directory.path().join("absent-tool"))).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn synchronous_probe_can_run_inside_tokio() {
    let output = run_version_probe(shell("printf 'version 2'; printf 'diagnostic' >&2")).unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"version 2");
    assert_eq!(output.stderr, b"diagnostic");
}

#[cfg(unix)]
#[test]
fn probe_reaps_child_on_timeout_and_output_overflow() {
    use std::time::Instant;
    for (script, expected_kind) in [
        ("printf '%s\\n' \"$$\" > \"$1\"; exec /bin/sleep 30", io::ErrorKind::TimedOut),
        (
            "printf '%s\\n' \"$$\" > \"$1\"; while :; do printf '0123456789' >&2; done",
            io::ErrorKind::InvalidData,
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let pid_path = directory.path().join("pid");
        let mut command = shell(script);
        command.arg(&pid_path);
        let start = Instant::now();
        let error = run_probe_with_limits(command, Duration::from_secs(1), 16).unwrap_err();
        assert_eq!(error.kind(), expected_kind);
        assert!(start.elapsed() < Duration::from_secs(3));
        let pid = std::fs::read_to_string(pid_path).unwrap();
        let alive = Command::new("kill")
            .args(["-0", pid.trim()])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success();
        if alive {
            let _ = Command::new("kill").args(["-KILL", pid.trim()]).status();
        }
        assert!(!alive, "the direct probe child must be reaped before returning");
    }
}

#[cfg(unix)]
#[test]
fn probe_deadline_includes_pipes_after_leader_exit() {
    use std::time::Instant;
    let directory = tempfile::tempdir().unwrap();
    let pid_path = directory.path().join("descendant-pid");
    let mut command = shell("/bin/sleep 30 & printf '%s\\n' \"$!\" > \"$1\"; printf version");
    command.arg(&pid_path);
    let start = Instant::now();
    let result = run_probe_with_limits(command, Duration::from_secs(1), 64);
    // The fixture's descendant deliberately owns the inherited pipe; it is not
    // the direct child managed by the probe, so the fixture cleans it up.
    let pid = std::fs::read_to_string(pid_path).unwrap();
    let _ = Command::new("kill").args(["-KILL", pid.trim()]).status();
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
    assert!(start.elapsed() < Duration::from_secs(3));
}
