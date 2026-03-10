use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RelaunchPreference {
    PreferPathCommand,
    PreferOriginalExecutable,
}

#[derive(Debug, Clone)]
struct PendingRelaunch {
    preference: RelaunchPreference,
}

#[derive(Debug, Clone, Default)]
struct RuntimeRelaunchContext {
    argv: Vec<OsString>,
    cwd: Option<PathBuf>,
    pending: Option<PendingRelaunch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RelaunchAttempt {
    program: OsString,
    args: Vec<OsString>,
}

static RUNTIME_RELAUNCH_CONTEXT: LazyLock<Mutex<RuntimeRelaunchContext>> =
    LazyLock::new(|| Mutex::new(RuntimeRelaunchContext::default()));

fn with_runtime_relaunch_context<R>(f: impl FnOnce(&mut RuntimeRelaunchContext) -> R) -> R {
    match RUNTIME_RELAUNCH_CONTEXT.lock() {
        Ok(mut context) => f(&mut context),
        Err(poisoned) => {
            let mut context = poisoned.into_inner();
            f(&mut context)
        }
    }
}

pub(crate) fn configure_runtime_relaunch_context(argv: Vec<OsString>, cwd: PathBuf) {
    with_runtime_relaunch_context(|context| {
        context.argv = argv;
        context.cwd = Some(cwd);
        context.pending = None;
    });
}

pub(crate) fn queue_runtime_relaunch(preference: RelaunchPreference) {
    with_runtime_relaunch_context(|context| {
        context.pending = Some(PendingRelaunch { preference });
    });
}

pub(crate) fn perform_queued_runtime_relaunch() {
    let Some((pending, argv, cwd)) = with_runtime_relaunch_context(|context| {
        let pending = context.pending.take()?;
        let cwd = context.cwd.clone()?;
        Some((pending, context.argv.clone(), cwd))
    }) else {
        return;
    };

    let current_exe = std::env::current_exe().ok();
    let attempts = relaunch_attempts(&argv, current_exe.as_deref(), pending.preference);
    let manual_command = attempts
        .first()
        .map(format_manual_restart_command)
        .unwrap_or_else(|| "vtcode".to_string());
    let mut last_error: Option<String> = None;

    for attempt in attempts {
        match spawn_relaunch_attempt(&attempt, &cwd) {
            Ok(_) => std::process::exit(0),
            Err(err) => {
                last_error = Some(format!("{}: {}", attempt.program.to_string_lossy(), err));
            }
        }
    }

    eprintln!("warning: VT Code updated but could not restart automatically.");
    eprintln!("warning: restart it manually with `{manual_command}`.");
    if let Some(err) = last_error {
        eprintln!("warning: last relaunch attempt failed: {err}");
    }
}

fn spawn_relaunch_attempt(attempt: &RelaunchAttempt, cwd: &Path) -> std::io::Result<()> {
    let mut command = Command::new(&attempt.program);
    command
        .args(&attempt.args)
        .current_dir(cwd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let _child = command.spawn()?;
    Ok(())
}

fn format_manual_restart_command(attempt: &RelaunchAttempt) -> String {
    let mut parts = Vec::with_capacity(attempt.args.len() + 1);
    parts.push(attempt.program.to_string_lossy().to_string());
    parts.extend(
        attempt
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().to_string()),
    );
    parts.join(" ")
}

fn relaunch_attempts(
    argv: &[OsString],
    current_exe: Option<&Path>,
    preference: RelaunchPreference,
) -> Vec<RelaunchAttempt> {
    let original_program = argv.first().cloned();
    let args = argv.get(1..).unwrap_or(&[]).to_vec();
    let current_exe = current_exe.map(|path| path.as_os_str().to_os_string());

    let candidates = match preference {
        RelaunchPreference::PreferPathCommand => vec![
            Some(OsString::from("vtcode")),
            original_program,
            current_exe,
        ],
        RelaunchPreference::PreferOriginalExecutable => vec![
            original_program,
            current_exe,
            Some(OsString::from("vtcode")),
        ],
    };

    let mut attempts = Vec::with_capacity(candidates.len());
    for candidate in candidates.into_iter().flatten() {
        if candidate.is_empty()
            || attempts
                .iter()
                .any(|attempt: &RelaunchAttempt| attempt.program == candidate)
        {
            continue;
        }
        attempts.push(RelaunchAttempt {
            program: candidate,
            args: args.clone(),
        });
    }
    attempts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relaunch_attempts_prefer_path_for_managed_installs() {
        let attempts = relaunch_attempts(
            &[
                OsString::from("/usr/local/bin/vtcode"),
                OsString::from("--resume"),
            ],
            Some(Path::new("/tmp/current-vtcode")),
            RelaunchPreference::PreferPathCommand,
        );

        assert_eq!(attempts[0].program, OsString::from("vtcode"));
        assert_eq!(attempts[0].args, vec![OsString::from("--resume")]);
        assert_eq!(attempts[1].program, OsString::from("/usr/local/bin/vtcode"));
    }

    #[test]
    fn relaunch_attempts_prefer_original_binary_for_standalone_installs() {
        let attempts = relaunch_attempts(
            &[OsString::from("/Users/dev/.local/bin/vtcode")],
            Some(Path::new("/tmp/current-vtcode")),
            RelaunchPreference::PreferOriginalExecutable,
        );

        assert_eq!(
            attempts[0].program,
            OsString::from("/Users/dev/.local/bin/vtcode")
        );
        assert_eq!(attempts[1].program, OsString::from("/tmp/current-vtcode"));
    }

    #[test]
    fn manual_restart_command_includes_original_args() {
        let command = format_manual_restart_command(&RelaunchAttempt {
            program: OsString::from("vtcode"),
            args: vec![OsString::from("--resume"), OsString::from("session-1")],
        });

        assert_eq!(command, "vtcode --resume session-1");
    }
}
