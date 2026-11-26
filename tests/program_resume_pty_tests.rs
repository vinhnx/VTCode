#![cfg(unix)]
//! Real terminal-stream coverage for OSC 7577 resume reporting.
//!
//! Spawns a live TUI session inside a pty, drives the resume adapter through
//! its public handle, and asserts the exact wire bytes on the terminal stream
//! (recomputed with the observed pid/time, so the MAC is verified too).

use anyhow::Context;
use std::process::Command;

const TOKEN: &str = "pty-resume-token-7577";
const FIXTURE_CWD: &str = "/tmp";
const FIXTURE_ID: &str = "pty-session-1";

fn fixture_argv() -> Vec<String> {
    vec![
        "vtcode".to_string(),
        "--workspace".to_string(),
        FIXTURE_CWD.to_string(),
        "--resume".to_string(),
        FIXTURE_ID.to_string(),
    ]
}

const CAPTURE: &str = r#"
import os, pty, fcntl, termios, struct, select, sys, time, signal
pid, fd = pty.fork()
if pid == 0:
    fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack('HHHH', 24, 100, 0, 0))
    env = dict(os.environ, TERM='xterm-256color', VTCODE_RESUME_PTY_CHILD='1', VTCODE_RESUME_PTY_MODE=sys.argv[2])
    env.pop('VTE_VERSION', None)
    if sys.argv[2] != 'no-token':
        env['OSC7577_TOKEN'] = 'pty-resume-token-7577'
    else:
        env.pop('OSC7577_TOKEN', None)
    os.execve(sys.argv[1], [sys.argv[1], '--exact', 'program_resume_pty_lifecycle', '--nocapture'], env)
data = bytearray()
deadline = time.monotonic() + 20
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
fn program_resume_pty_lifecycle() {
    if std::env::var_os("VTCODE_RESUME_PTY_CHILD").is_some() {
        run_child().expect("PTY child lifecycle");
        return;
    }
    let executable = std::env::current_exe().unwrap();
    for mode in ["terminal", "disabled", "no-token"] {
        let output = Command::new("python3")
            .args(["-c", CAPTURE])
            .arg(&executable)
            .arg(mode)
            .output()
            .unwrap();
        assert!(output.status.success(), "{mode} capture failed: {}", String::from_utf8_lossy(&output.stdout));
        let raw = String::from_utf8_lossy(&output.stdout);
        if mode != "terminal" {
            assert!(!raw.contains("\x1b]7577;"), "{mode} must suppress reports");
            continue;
        }
        // Exactly one report and one clear: nothing extra may emit, and the
        // worker shutdown must not clear (the record survives TUI teardown).
        assert_eq!(raw.matches("\x1b]7577;").count(), 2, "one report plus one clear: {raw:?}");
        let bodies: Vec<_> = raw
            .split("\x1b]7577;")
            .skip(1)
            .map(|part| part.split("\x1b\\").next().unwrap())
            .collect();
        let (pid, time) = parse_pid_time(bodies[0]);
        let expected_report =
            vtcode_commons::program_resume::encode_report(pid, time, &fixture_argv(), Some(FIXTURE_CWD), TOKEN)
                .expect("fixture must encode");
        assert!(raw.contains(&expected_report), "report bytes must verify against the token");
        let (clear_pid, clear_time) = parse_pid_time(bodies[1]);
        let expected_clear =
            vtcode_commons::program_resume::encode_clear(clear_pid, clear_time, TOKEN).expect("clear must encode");
        assert!(raw.contains(&expected_clear), "clear bytes must verify against the token");
        assert_eq!(pid, clear_pid, "report and clear share the owning pid");
        assert!(clear_time >= time, "clear must not predate the report");
        let last_sequence = raw.rfind("\x1b]7577;").unwrap();
        let restore = raw.rfind("\x1b[?1049l").unwrap();
        assert!(last_sequence < restore, "resume writes must precede terminal restoration");
    }
}

fn parse_pid_time(body: &str) -> (u32, u64) {
    let pid = body
        .split(":time=")
        .next()
        .unwrap()
        .strip_prefix("pid=")
        .unwrap()
        .parse()
        .unwrap();
    let time = body.split(":time=").nth(1).unwrap().split(':').next().unwrap().parse().unwrap();
    (pid, time)
}

fn run_child() -> anyhow::Result<()> {
    use vtcode_commons::program_resume::ProgramResumeUpdate;
    use vtcode_ui::tui::app::{SessionOptions, SessionSurface, spawn_session_with_options};
    let runtime = tokio::runtime::Runtime::new().context("building resume PTY test runtime")?;
    runtime.block_on(async {
        let mode = std::env::var("VTCODE_RESUME_PTY_MODE").context("reading resume PTY mode")?;
        let mut session = spawn_session_with_options(
            Default::default(),
            SessionOptions {
                surface_preference: SessionSurface::Alternate,
                ..Default::default()
            },
        )
        .context("starting resume PTY test TUI")?;
        let handle = session.clone_inline_handle();
        handle.program_resume(ProgramResumeUpdate::Configure { enabled: mode != "disabled" });
        handle.program_resume(ProgramResumeUpdate::SetTarget {
            argv: fixture_argv(),
            cwd: Some(FIXTURE_CWD.to_string()),
        });
        handle.program_resume(ProgramResumeUpdate::Clear);
        handle.shutdown();
        assert!(session.wait_for_exit(std::time::Duration::from_secs(8)).await);
        Ok(())
    })
}
