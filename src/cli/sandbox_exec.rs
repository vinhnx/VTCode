//! Hidden `vtcode sandbox-exec` subcommand: the Linux sandbox launcher.
//!
//! The main binary doubles as the sandbox helper (busybox pattern). Command
//! transforms wrap sandboxed commands as
//! `vtcode sandbox-exec --sandbox-policy-cwd … --sandbox-policy … --seccomp-profile …
//! --resource-limits … -- <command>`, and this module applies the kernel
//! restrictions (Landlock filesystem rules, seccomp syscall filtering, rlimits)
//! to the launcher process before exec-ing the wrapped command. See
//! `vtcode_safety::sandboxing::linux` for the enforcement details.
//!
//! This runs before any CLI/startup machinery so sandboxed command spawns stay
//! fast and never depend on config loading.

use std::ffi::OsString;
use std::os::unix::process::CommandExt as _;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};

use vtcode_safety::sandboxing::{ResourceLimits, SandboxPolicy, apply_sandbox_restrictions};

/// Launcher-internal error exit code (distinct from the wrapped command's
/// status and from exec failures).
const EXIT_LAUNCHER_ERROR: i32 = 125;
/// `exec` failed because the target was not found.
const EXIT_COMMAND_NOT_FOUND: i32 = 127;
/// `exec` failed for another reason.
const EXIT_COMMAND_NOT_EXECUTABLE: i32 = 126;

/// Handle `vtcode sandbox-exec …` before any other startup work.
///
/// Returns `false` when this invocation is not the launcher (normal startup
/// proceeds). On launcher error paths this never returns: it exits with a
/// launcher-specific status so failures are never mistaken for the wrapped
/// command's exit code.
pub fn try_run_sandbox_exec_mode() -> bool {
    let mut argv = std::env::args_os();
    let _binary = argv.next();
    match argv.next() {
        Some(arg) if arg == OsString::from("sandbox-exec") => {}
        _ => return false,
    }

    let args: Vec<OsString> = argv.collect();
    if let Err(error) = launch(args) {
        vtcode_commons::diagnostics::stderr(format_args!("vtcode sandbox-exec: {error:#}"));
        std::process::exit(EXIT_LAUNCHER_ERROR);
    }
    unreachable!("sandbox launcher exec replaced the process");
}

/// The parsed helper-protocol invocation.
struct SandboxProtocolArgs {
    policy_cwd: PathBuf,
    policy_json: String,
    seccomp_json: Option<String>,
    limits_json: String,
    command: Vec<OsString>,
}

fn parse_protocol_args(args: Vec<OsString>) -> Result<SandboxProtocolArgs> {
    let mut policy_cwd: Option<PathBuf> = None;
    let mut policy_json: Option<String> = None;
    let mut seccomp_json: Option<String> = None;
    let mut limits_json: Option<String> = None;
    let mut command: Vec<OsString> = Vec::new();

    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        if arg == OsString::from("--") {
            command = iter.collect();
            break;
        }
        let text = arg.to_string_lossy().into_owned();
        if !text.starts_with("--") {
            bail!("unexpected token {text:?} before `--`; usage: vtcode sandbox-exec <flags> -- <command> [args...]");
        }
        let (flag, inline_value) = match text.split_once('=') {
            Some((flag, value)) => (flag.to_string(), Some(OsString::from(value))),
            None => (text.clone(), None),
        };
        let take_value = |flag: &str, missing: Option<OsString>| -> Result<OsString> {
            missing.ok_or_else(|| anyhow!("missing value for {flag}"))
        };
        match flag.as_str() {
            "--sandbox-policy-cwd" => {
                policy_cwd = Some(PathBuf::from(take_value(&flag, inline_value.or_else(|| iter.next()))?));
            }
            "--sandbox-policy" => {
                policy_json = Some(
                    take_value(&flag, inline_value.or_else(|| iter.next()))?
                        .to_string_lossy()
                        .into_owned(),
                );
            }
            "--seccomp-profile" => {
                seccomp_json = Some(
                    take_value(&flag, inline_value.or_else(|| iter.next()))?
                        .to_string_lossy()
                        .into_owned(),
                );
            }
            "--resource-limits" => {
                limits_json = Some(
                    take_value(&flag, inline_value.or_else(|| iter.next()))?
                        .to_string_lossy()
                        .into_owned(),
                );
            }
            other => bail!("unknown sandbox launcher flag {other:?}"),
        }
    }

    if command.is_empty() {
        bail!("no command after `--`; usage: vtcode sandbox-exec <flags> -- <command> [args...]");
    }
    Ok(SandboxProtocolArgs {
        policy_cwd: policy_cwd.ok_or_else(|| anyhow!("missing --sandbox-policy-cwd"))?,
        policy_json: policy_json.ok_or_else(|| anyhow!("missing --sandbox-policy"))?,
        seccomp_json,
        limits_json: limits_json.ok_or_else(|| anyhow!("missing --resource-limits"))?,
        command,
    })
}

fn launch(args: Vec<OsString>) -> Result<()> {
    let protocol = parse_protocol_args(args)?;
    let policy: SandboxPolicy = serde_json::from_str(&protocol.policy_json)
        .map_err(|error| anyhow!("invalid --sandbox-policy JSON: {error}"))
        .context("parsing sandbox policy")?;
    let seccomp = match &protocol.seccomp_json {
        Some(json) => serde_json::from_str(json)
            .map_err(|error| anyhow!("invalid --seccomp-profile JSON: {error}"))
            .context("parsing seccomp profile")?,
        None => policy.seccomp_profile(),
    };
    let limits: ResourceLimits = serde_json::from_str(&protocol.limits_json)
        .map_err(|error| anyhow!("invalid --resource-limits JSON: {error}"))
        .context("parsing resource limits")?;

    // Fail closed: if any restriction cannot be applied, this process exits
    // with a launcher error instead of exec-ing the command unsandboxed.
    apply_sandbox_restrictions(&policy, &seccomp, &limits, &protocol.policy_cwd)
        .context("applying Linux sandbox restrictions")?;

    exec_wrapped_command(protocol.command)
}

/// Replace this process with the wrapped command. Never returns: on `exec`
/// failure the launcher exits with a conventional exec status.
fn exec_wrapped_command(command: Vec<OsString>) -> ! {
    let mut parts = command.into_iter();
    let program = parts.next().unwrap_or_else(|| unreachable!("parser rejects empty commands"));
    let mut cmd = std::process::Command::new(&program);
    cmd.args(parts);
    // `CommandExt::exec` replaces the process on success and returns the
    // `io::Error` directly on failure (it never returns `Ok`).
    let error = cmd.exec();
    let exit_code = match error.kind() {
        std::io::ErrorKind::NotFound => EXIT_COMMAND_NOT_FOUND,
        _ => EXIT_COMMAND_NOT_EXECUTABLE,
    };
    vtcode_commons::diagnostics::stderr(format_args!(
        "vtcode sandbox-exec: failed to exec {}: {error}",
        program.to_string_lossy()
    ));
    std::process::exit(exit_code);
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    #[test]
    fn parse_protocol_args_accepts_full_flag_set() {
        let parsed = parse_protocol_args(args(&[
            "--sandbox-policy-cwd",
            "/tmp/ws",
            "--sandbox-policy",
            r#"{"type":"read_only"}"#,
            "--seccomp-profile",
            r#"{"blocked_syscalls":[],"allow_namespaces":false,"allow_network_sockets":false,"log_only":true}"#,
            "--resource-limits",
            r#"{"max_memory_mb":0,"max_pids":0,"max_disk_mb":0,"cpu_time_secs":0,"timeout_secs":0}"#,
            "--",
            "echo",
            "hi",
        ]))
        .expect("protocol args parse");

        assert_eq!(parsed.policy_cwd, PathBuf::from("/tmp/ws"));
        assert_eq!(parsed.command, vec![OsString::from("echo"), OsString::from("hi")]);
        assert!(parsed.seccomp_json.is_some());
    }

    #[test]
    fn parse_protocol_args_accepts_inline_values() {
        let parsed = parse_protocol_args(args(&[
            "--sandbox-policy-cwd=/tmp/ws",
            "--sandbox-policy={\"type\":\"read_only\"}",
            "--resource-limits={}",
            "--",
            "true",
        ]))
        .expect("protocol args parse");

        assert_eq!(parsed.policy_cwd, PathBuf::from("/tmp/ws"));
        assert_eq!(parsed.policy_json, r#"{"type":"read_only"}"#);
    }

    #[test]
    fn parse_protocol_args_rejects_json_value_tokens_as_flags() {
        // A JSON value containing `=` must not be mistaken for a flag.
        let parsed = parse_protocol_args(args(&[
            "--sandbox-policy-cwd",
            "/tmp/ws",
            "--sandbox-policy",
            r#"{"type":"read_only","note":"a=b"}"#,
            "--resource-limits",
            "{}",
            "--",
            "true",
        ]));
        assert!(parsed.is_ok());
    }

    #[test]
    fn parse_protocol_args_requires_command() {
        let parsed = parse_protocol_args(args(&[
            "--sandbox-policy-cwd",
            "/tmp/ws",
            "--sandbox-policy",
            r#"{"type":"read_only"}"#,
            "--resource-limits",
            "{}",
        ]));
        assert!(parsed.is_err());
    }
}
