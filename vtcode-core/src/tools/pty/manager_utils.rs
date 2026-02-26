use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use portable_pty::{CommandBuilder, PtySize};

use super::command_utils::is_shell_program;
use crate::tools::path_env;
use crate::tools::shell_snapshot::ShellSnapshot;

pub(super) fn clamp_timeout(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

pub(super) fn exit_status_code(status: portable_pty::ExitStatus) -> i32 {
    if status.signal().is_some() {
        -1
    } else {
        status.exit_code() as i32
    }
}

pub(super) fn set_command_environment(
    builder: &mut CommandBuilder,
    program: &str,
    size: PtySize,
    workspace_root: &Path,
    extra_paths: &[PathBuf],
) {
    // Inherit environment from parent process to preserve PATH and other important variables
    let mut env_map: HashMap<OsString, OsString> = std::env::vars_os().collect();

    // Ensure HOME is set - this is crucial for proper path expansion in cargo and other tools
    let home_key = OsString::from("HOME");
    if !env_map.contains_key(&home_key)
        && let Some(home_dir) = dirs::home_dir()
    {
        env_map.insert(home_key.clone(), OsString::from(home_dir.as_os_str()));
    }

    let path_key = OsString::from("PATH");
    let current_path = env_map.get(&path_key).map(|value| value.as_os_str());
    if let Some(merged) = path_env::merge_path_env(current_path, extra_paths) {
        env_map.insert(path_key, merged);
    }

    for (key, value) in env_map {
        builder.env(key, value);
    }

    // Override or set specific environment variables for TTY
    builder.env("TERM", "xterm-256color");
    builder.env("PAGER", "cat");
    builder.env("GIT_PAGER", "cat");
    builder.env("LESS", "R");
    builder.env("COLUMNS", size.cols.to_string());
    builder.env("LINES", size.rows.to_string());
    builder.env("WORKSPACE_DIR", workspace_root.as_os_str());

    // Disable automatic color output from ls and other commands
    builder.env("CLICOLOR", "0");
    builder.env("CLICOLOR_FORCE", "0");
    builder.env("LS_COLORS", "");
    builder.env("NO_COLOR", "1");

    // For Rust/Cargo, disable colors at the source
    builder.env("CARGO_TERM_COLOR", "never");

    // Suppress macOS malloc debugging junk that can pollute PTY output
    // This is especially common when running in login shells (-l)
    builder.env_remove("MallocStackLogging");
    builder.env_remove("MallocStackLoggingNoCompact");
    builder.env_remove("MallocStackLoggingDirectory");
    builder.env_remove("MallocErrorAbort");
    builder.env_remove("MallocCheckHeapStart");
    builder.env_remove("MallocCheckHeapEach");
    builder.env_remove("MallocCheckHeapSleep");
    builder.env_remove("MallocCheckHeapAbort");
    builder.env_remove("MallocGuardEdges");
    builder.env_remove("MallocScribble");
    builder.env_remove("MallocDoNotProtectSentinel");
    builder.env_remove("MallocQuiet");

    if is_shell_program(program) {
        builder.env("SHELL", program);
    }
}

/// Set command environment from a shell snapshot for faster startup.
///
/// This uses a pre-captured shell environment instead of inheriting from the
/// parent process, which can speed up command execution by avoiding the need
/// to run login scripts via `-l` flag.
#[allow(dead_code)] // Infrastructure for future snapshot-based PTY execution
pub(super) fn set_command_environment_from_snapshot(
    builder: &mut CommandBuilder,
    snapshot: &ShellSnapshot,
    program: &str,
    size: PtySize,
    workspace_root: &Path,
    extra_paths: &[PathBuf],
) {
    // Start with the snapshot environment
    for (key, value) in &snapshot.env {
        builder.env(key, value);
    }

    // Merge extra paths into PATH
    let path_key = OsString::from("PATH");
    let current_path = snapshot.env.get("PATH").map(OsString::from);
    let current_path_ref = current_path.as_deref();
    if let Some(merged) = path_env::merge_path_env(current_path_ref, extra_paths) {
        builder.env(path_key, merged);
    }

    // Override or set specific environment variables for TTY
    builder.env("TERM", "xterm-256color");
    builder.env("PAGER", "cat");
    builder.env("GIT_PAGER", "cat");
    builder.env("LESS", "R");
    builder.env("COLUMNS", size.cols.to_string());
    builder.env("LINES", size.rows.to_string());
    builder.env("WORKSPACE_DIR", workspace_root.as_os_str());

    // Disable automatic color output
    builder.env("CLICOLOR", "0");
    builder.env("CLICOLOR_FORCE", "0");
    builder.env("LS_COLORS", "");
    builder.env("NO_COLOR", "1");
    builder.env("CARGO_TERM_COLOR", "never");

    // Suppress macOS malloc debugging
    builder.env_remove("MallocStackLogging");
    builder.env_remove("MallocStackLoggingNoCompact");
    builder.env_remove("MallocStackLoggingDirectory");
    builder.env_remove("MallocErrorAbort");
    builder.env_remove("MallocCheckHeapStart");
    builder.env_remove("MallocCheckHeapEach");
    builder.env_remove("MallocCheckHeapSleep");
    builder.env_remove("MallocCheckHeapAbort");
    builder.env_remove("MallocGuardEdges");
    builder.env_remove("MallocScribble");
    builder.env_remove("MallocDoNotProtectSentinel");
    builder.env_remove("MallocQuiet");

    if is_shell_program(program) {
        builder.env("SHELL", program);
    }
}
