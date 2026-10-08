#![cfg(unix)]
//! Real terminal-stream coverage without a provider or user credentials.

use anyhow::Context;
use std::process::Command;

const CAPTURE: &str = r#"
import os, pty, fcntl, termios, struct, select, sys, time, signal
pid, fd = pty.fork()
if pid == 0:
    fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack('HHHH', 24, 100, 0, 0))
    env = dict(os.environ, TERM='xterm-256color', VTCODE_STATUS_PTY_CHILD='1', VTCODE_STATUS_PTY_MODE=sys.argv[2])
    redirected = {'redirect-stdin': 0, 'redirect-stdout': 1, 'redirect-stderr': 2}.get(sys.argv[2])
    if redirected is not None:
        replacement = os.open(os.devnull, os.O_RDONLY if redirected == 0 else os.O_WRONLY)
        os.dup2(replacement, redirected)
        os.close(replacement)
    argv = [sys.argv[1], '--exact', 'program_status_pty_lifecycle', '--nocapture']
    if sys.argv[2] == 'recorder':
        os.execve('/usr/bin/script', ['script', '-q', sys.argv[3], *argv], env)
    os.execve(sys.argv[1], argv, env)
data = bytearray()
deadline = time.monotonic() + 15
while time.monotonic() < deadline:
    if select.select([fd], [], [], 0.1)[0]:
        try:
            chunk = os.read(fd, 65536)
        except OSError:
            break
        if not chunk:
            break
        data.extend(chunk)
        if b'\x1b[6n' in chunk:
            os.write(fd, b'\x1b[1;1R')
else:
    os.kill(pid, signal.SIGKILL)
_, status = os.waitpid(pid, 0)
os.close(fd)
sys.stdout.buffer.write(data)
sys.exit(os.waitstatus_to_exitcode(status))
"#;

#[test]
fn program_status_pty_lifecycle() {
    if std::env::var_os("VTCODE_STATUS_PTY_CHILD").is_some() {
        run_child().expect("PTY child lifecycle");
        return;
    }
    let executable = std::env::current_exe().unwrap();
    let capture_dir = tempfile::tempdir().unwrap();
    let recording_path = capture_dir.path().join("terminal.typescript");
    for mode in [
        "terminal",
        "error",
        "disabled",
        "redirect-stdin",
        "redirect-stdout",
        "redirect-stderr",
        "cancel",
        "reload",
        "recorder",
    ] {
        // macOS script accepts an argv command; other hosts use different flags.
        if mode == "recorder" && !cfg!(target_os = "macos") {
            continue;
        }
        let output = Command::new("python3")
            .args(["-c", CAPTURE])
            .arg(&executable)
            .arg(mode)
            .arg(&recording_path)
            .output()
            .unwrap();
        assert!(output.status.success(), "{mode} capture failed: {}", String::from_utf8_lossy(&output.stdout));
        let raw = String::from_utf8_lossy(&output.stdout);
        if mode == "disabled" || mode.starts_with("redirect-") {
            assert!(!raw.contains("\x1b]7501;"), "{mode} must suppress reports");
            continue;
        }
        let reports: Vec<_> = raw
            .split("\x1b]7501;")
            .skip(1)
            .map(|part| part.split("\x1b\\").next().unwrap())
            .collect();
        assert!(!reports.contains(&"?"), "reporting must not query or require a supporting terminal");
        assert!(reports.iter().any(|report| report.starts_with("state=working:")));
        let blocked = reports
            .iter()
            .position(|report| report.contains("state=blocked:") && report.contains(":kind=question:"))
            .unwrap();
        let resumed = reports
            .iter()
            .enumerate()
            .skip(blocked + 1)
            .find(|(_, report)| report.starts_with("state=working:"))
            .unwrap()
            .0;
        if mode == "cancel" {
            assert!(reports.iter().skip(resumed + 1).any(|report| report.starts_with("state=idle:")));
            assert!(
                !reports
                    .iter()
                    .any(|report| report.starts_with("state=done:") || report.starts_with("state=error:"))
            );
            assert!(reports.last().unwrap().starts_with("state=clear:"));
        } else {
            let finished_prefix = if mode == "error" { "state=error:" } else { "state=done:" };
            let done = reports
                .iter()
                .enumerate()
                .skip(resumed + 1)
                .find(|(_, report)| report.starts_with(finished_prefix))
                .unwrap()
                .0;
            assert!(blocked < resumed && resumed < done);
            let parent_id = reports[done].split(":id=").nth(1).unwrap().split(':').next().unwrap();
            let last_done = reports.iter().rposition(|report| report.starts_with(finished_prefix)).unwrap();
            assert!(
                !reports
                    .iter()
                    .skip(last_done + 1)
                    .any(|report| report.starts_with("state=clear:") && report.contains(&format!(":id={parent_id}:")))
            );
            if mode == "reload" {
                let cleared = reports.iter().position(|report| report.starts_with("state=clear:")).unwrap();
                assert!(done < cleared && cleared < last_done);
                assert!(reports[last_done].contains(&format!(":id={parent_id}:")), "reload must retain owned identity");
            }
        }
        let last_report = raw.rfind("\x1b]7501;").unwrap();
        let restore = raw.rfind("\x1b[?1049l").unwrap();
        assert!(last_report < restore, "status writes must precede terminal restoration");
        assert_eq!(reports.iter().filter(|report| report.starts_with("state=blocked:")).count(), 1);
        if mode == "recorder" {
            let recording = std::fs::read_to_string(&recording_path).unwrap();
            assert_eq!(recording.matches("\x1b]7501;").count(), reports.len());
            assert!(recording.rfind("\x1b]7501;").unwrap() < recording.rfind("\x1b[?1049l").unwrap());
        }
    }
}

fn run_child() -> anyhow::Result<()> {
    use vtcode_commons::program_status::{InteractionKind, ProgramState, ProgramStatusUpdate};
    use vtcode_commons::ui_protocol::ProgressPhase;
    use vtcode_ui::tui::app::{SessionOptions, SessionSurface, spawn_session_with_options};
    let runtime = tokio::runtime::Runtime::new().context("building status PTY test runtime")?;
    runtime.block_on(async {
        let mode = std::env::var("VTCODE_STATUS_PTY_MODE").context("reading status PTY mode")?;
        let result = spawn_session_with_options(
            Default::default(),
            SessionOptions {
                surface_preference: SessionSurface::Alternate,
                ..Default::default()
            },
        );
        if mode == "redirect-stdin" {
            let error = result.err().context("redirected stdin must reject interactive TUI startup")?;
            assert!(error.to_string().contains("stdin is not a terminal"));
            return Ok(());
        }
        let mut session = result.context("starting status PTY test TUI")?;
        let handle = session.clone_inline_handle();
        if mode != "disabled" && mode != "reload" {
            handle.program_status(ProgramStatusUpdate::Configure { enabled: true });
        }
        let progress = handle.begin_progress(ProgressPhase::PreparingContext);
        let wait = handle.program_status_wait(InteractionKind::Question);
        if mode == "reload" {
            handle.program_status(ProgramStatusUpdate::Configure { enabled: true });
        }
        // Help/navigation has no interaction ownership and cannot add a blocked report.
        handle.show_modal("Help".into(), vec!["Navigation".into()], None);
        handle.close_modal();
        drop(wait);
        let outcome = match mode.as_str() {
            "cancel" => ProgramState::Idle,
            "error" => ProgramState::Error,
            _ => ProgramState::Done,
        };
        handle.program_status(ProgramStatusUpdate::Outcome(outcome));
        drop(progress);
        if mode == "reload" {
            handle.program_status(ProgramStatusUpdate::Configure { enabled: false });
            handle.program_status(ProgramStatusUpdate::Configure { enabled: true });
        }
        handle.shutdown();
        assert!(session.wait_for_exit(std::time::Duration::from_secs(8)).await);
        handle.program_status(ProgramStatusUpdate::Outcome(ProgramState::Error));
        Ok(())
    })
}
