//! Desktop-owned shell process and PTY I/O.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SendError, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context as _, Result};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

const OUTPUT_QUEUE_CAPACITY: usize = 32;
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug)]
pub(super) enum TerminalProcessEvent {
    Output(Vec<u8>),
    OutputClosed,
    Error(String),
    Exited(String),
}

pub(super) struct TerminalProcess {
    wake: SyncSender<()>,
    resize: Arc<Mutex<Option<PtySize>>>,
    closing: Arc<AtomicBool>,
    /// Unbounded: the UI thread is the single producer and must never block or
    /// drop keystrokes; the writer drains continuously.
    input: Sender<Vec<u8>>,
    output: Receiver<TerminalProcessEvent>,
    events: Receiver<TerminalProcessEvent>,
}

/// Interactive shell for production sessions, honoring the user's `$SHELL`.
pub(super) fn default_shell() -> PathBuf {
    std::env::var_os("SHELL")
        .filter(|shell| !shell.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/bin/sh"))
}

impl TerminalProcess {
    #[cfg(test)]
    pub(super) fn spawn(workspace: &Path, shell: &Path, size: PtySize) -> Result<Self> {
        Self::spawn_command(Self::base_command(workspace, shell), size)
    }

    /// Production sessions: a login shell so GUI-spawned terminals load the
    /// user's profile (PATH, prompt). Tests use [`Self::spawn`] to stay hermetic.
    pub(super) fn spawn_login(workspace: &Path, shell: &Path, size: PtySize) -> Result<Self> {
        let mut command = Self::base_command(workspace, shell);
        command.arg("-l");
        Self::spawn_command(command, size)
    }

    fn base_command(workspace: &Path, shell: &Path) -> CommandBuilder {
        let mut command = CommandBuilder::new(shell);
        command.cwd(workspace);
        command.env("TERM", "xterm-256color");
        command
    }

    fn spawn_command(command: CommandBuilder, size: PtySize) -> Result<Self> {
        let pair = native_pty_system().openpty(size).context("open terminal PTY")?;
        let mut reader = pair.master.try_clone_reader().context("open terminal reader")?;
        let mut writer = pair.master.take_writer().context("open terminal writer")?;
        let mut child = pair.slave.spawn_command(command).context("start terminal shell")?;
        drop(pair.slave);

        let (output_tx, output) = mpsc::sync_channel(OUTPUT_QUEUE_CAPACITY);
        let (events_tx, events) = mpsc::channel();
        let (input, input_rx) = mpsc::channel::<Vec<u8>>();
        let (wake, wake_rx) = mpsc::sync_channel(1);
        let resize = Arc::new(Mutex::new(None::<PtySize>));
        let closing = Arc::new(AtomicBool::new(false));
        let worker_resize = Arc::clone(&resize);
        let worker_closing = Arc::clone(&closing);
        let reader_events = events_tx.clone();
        let _reader_thread = thread::spawn(move || {
            let mut buffer = [0; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        let bytes = buffer.iter().copied().take(count).collect();
                        if output_tx.send(TerminalProcessEvent::Output(bytes)).is_err() {
                            break;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        let _sent = reader_events.send(TerminalProcessEvent::Error(format!("Terminal read: {error}")));
                        break;
                    }
                }
            }
            let _sent = output_tx.send(TerminalProcessEvent::OutputClosed);
        });
        let writer_events = events_tx.clone();
        let _writer_thread = thread::spawn(move || {
            while let Ok(bytes) = input_rx.recv() {
                // Pace large writes: instant multi-KB blasts overrun the kernel
                // TTY input queue and get silently discarded. Single-chunk
                // writes (typing, small pastes) never sleep.
                let mut chunks = bytes.chunks(1024).peekable();
                while let Some(chunk) = chunks.next() {
                    if let Err(error) = writer.write_all(chunk) {
                        let _sent = writer_events.send(TerminalProcessEvent::Error(format!("Terminal input: {error}")));
                        return;
                    }
                    if chunks.peek().is_some() {
                        thread::sleep(Duration::from_millis(2));
                    }
                }
            }
        });
        let _process_thread = thread::spawn(move || {
            let master = pair.master;
            let mut reaped = false;
            let mut wait_error_reported = false;
            loop {
                match if reaped { Ok(None) } else { child.try_wait() } {
                    Ok(Some(status)) => {
                        reaped = true;
                        let _sent = events_tx.send(TerminalProcessEvent::Exited(status.to_string()));
                    }
                    Err(error) => {
                        if !wait_error_reported {
                            let _sent = events_tx.send(TerminalProcessEvent::Error(format!("Terminal wait: {error}")));
                            wait_error_reported = true;
                        }
                    }
                    Ok(None) => {}
                }
                let _wake = wake_rx.recv_timeout(PROCESS_POLL_INTERVAL);
                if !worker_closing.load(Ordering::Acquire) {
                    let size = match worker_resize.lock() {
                        Ok(mut pending) => pending.take(),
                        Err(error) => error.into_inner().take(),
                    };
                    if let Some(size) = size {
                        match master.resize(size) {
                            Ok(()) => {}
                            Err(error) => {
                                let _sent =
                                    events_tx.send(TerminalProcessEvent::Error(format!("Terminal resize: {error}")));
                            }
                        }
                    }
                } else {
                    // The foreground job may be in a different group from
                    // the shell. Both are still anchored to this live PTY.
                    let pid = child.process_id();
                    if let Some(group) = master.process_group_leader().and_then(|id| u32::try_from(id).ok())
                        && (reaped || Some(group) != pid)
                        && let Err(error) = vtcode_bash_runner::kill_process_group(group)
                    {
                        let _sent =
                            events_tx.send(TerminalProcessEvent::Error(format!("Terminal foreground close: {error}")));
                    }
                    if reaped {
                        break;
                    }
                    let terminated = pid
                        .map(vtcode_bash_runner::kill_process_group_by_pid)
                        .is_some_and(|result| result.is_ok());
                    if !terminated && let Err(error) = child.kill() {
                        let _sent = events_tx.send(TerminalProcessEvent::Error(format!("Terminal close: {error}")));
                    }
                    loop {
                        match child.wait() {
                            Ok(status) => {
                                let _sent = events_tx.send(TerminalProcessEvent::Exited(status.to_string()));
                                break;
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                            Err(error) => {
                                if !wait_error_reported {
                                    let _sent =
                                        events_tx.send(TerminalProcessEvent::Error(format!("Terminal wait: {error}")));
                                }
                                break;
                            }
                        }
                    }
                    break;
                }
            }
        });
        Ok(Self { wake, resize, closing, input, output, events })
    }

    pub(super) fn input(&self, bytes: Vec<u8>) -> Result<(), SendError<Vec<u8>>> {
        self.input.send(bytes)
    }

    pub(super) fn resize(&self, size: PtySize) -> Result<(), SendError<()>> {
        if self.closing.load(Ordering::Acquire) {
            return Err(SendError(()));
        }
        match self.resize.lock() {
            Ok(mut pending) => *pending = Some(size),
            Err(error) => *error.into_inner() = Some(size),
        }
        match self.wake.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => Ok(()),
            Err(TrySendError::Disconnected(())) => Err(SendError(())),
        }
    }

    pub(super) fn try_recv(&self) -> Result<TerminalProcessEvent, mpsc::TryRecvError> {
        match self.events.try_recv() {
            Ok(event) => Ok(event),
            Err(mpsc::TryRecvError::Empty) => match self.output.try_recv() {
                Err(mpsc::TryRecvError::Disconnected) => Err(mpsc::TryRecvError::Empty),
                result => result,
            },
            Err(mpsc::TryRecvError::Disconnected) => self.output.try_recv(),
        }
    }

    /// Kill the shell with its foreground process group, then reap it. Jobs in
    /// detached background groups are not enumerated: callers must not present
    /// close as proof that every descendant died.
    pub(super) fn close(&self) {
        self.closing.store(true, Ordering::Release);
        let _sent = self.wake.try_send(());
    }
}

impl Drop for TerminalProcess {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn dimensions(rows: u16, cols: u16) -> PtySize {
        PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }
    }

    fn collect_until(process: &mut TerminalProcess, expected: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = String::new();
        while Instant::now() < deadline {
            match process.try_recv() {
                Ok(TerminalProcessEvent::Output(bytes)) => output.push_str(&String::from_utf8_lossy(&bytes)),
                Ok(TerminalProcessEvent::OutputClosed) => panic!("terminal output closed before {expected}"),
                Ok(TerminalProcessEvent::Error(error)) => panic!("unexpected terminal error: {error}"),
                Ok(TerminalProcessEvent::Exited(status)) => panic!("terminal exited before {expected}: {status}"),
                Err(mpsc::TryRecvError::Empty) => thread::sleep(Duration::from_millis(10)),
                Err(error) => panic!("terminal events unavailable: {error}"),
            }
            if output.contains(expected) {
                return output;
            }
        }
        panic!("timed out waiting for {expected}: {output}");
    }

    #[test]
    fn terminal_process_reports_exit_and_drains_later_output() {
        let (wake, _wake_rx) = mpsc::sync_channel(1);
        let (input, _input_rx) = mpsc::channel();
        let (output_tx, output) = mpsc::sync_channel(1);
        let (events_tx, events) = mpsc::channel();
        let process = TerminalProcess {
            wake,
            resize: Arc::new(Mutex::new(None)),
            closing: Arc::new(AtomicBool::new(false)),
            input,
            output,
            events,
        };
        events_tx.send(TerminalProcessEvent::Exited("done".into())).unwrap();
        assert!(matches!(process.try_recv(), Ok(TerminalProcessEvent::Exited(status)) if status == "done"));
        output_tx.send(TerminalProcessEvent::Output(b"last".to_vec())).unwrap();
        assert!(matches!(process.try_recv(), Ok(TerminalProcessEvent::Output(bytes)) if bytes == b"last"));
        output_tx.send(TerminalProcessEvent::OutputClosed).unwrap();
        drop(output_tx);
        assert!(matches!(process.try_recv(), Ok(TerminalProcessEvent::OutputClosed)));
    }

    #[test]
    fn terminal_process_real_exit_preserves_output_and_status() {
        let dir = tempfile::tempdir().unwrap();
        let mut command = CommandBuilder::new("/bin/sh");
        command.cwd(dir.path());
        command.arg("-c");
        command.arg("printf 'final-output'; exit 7");
        let process = TerminalProcess::spawn_command(command, dimensions(10, 40)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = Vec::new();
        let mut status = None;
        let mut output_closed = false;
        while status.is_none() || !output_closed {
            assert!(Instant::now() < deadline, "terminal exit or output closure not observed");
            match process.try_recv() {
                Ok(TerminalProcessEvent::Output(bytes)) => output.extend(bytes),
                Ok(TerminalProcessEvent::OutputClosed) => output_closed = true,
                Ok(TerminalProcessEvent::Exited(value)) => status = Some(value),
                Ok(TerminalProcessEvent::Error(error)) => panic!("terminal failed: {error}"),
                Err(mpsc::TryRecvError::Empty) => thread::sleep(Duration::from_millis(10)),
                Err(error) => panic!("terminal events unavailable: {error}"),
            }
        }
        assert_eq!(output, b"final-output");
        assert!(status.unwrap().contains('7'));
    }

    #[test]
    fn terminal_process_sessions_use_workspace_input_and_resize_independently() {
        let first_dir = tempfile::tempdir().unwrap();
        let second_dir = tempfile::tempdir().unwrap();
        let mut first = TerminalProcess::spawn(first_dir.path(), Path::new("/bin/sh"), dimensions(10, 40)).unwrap();
        let mut second = TerminalProcess::spawn(second_dir.path(), Path::new("/bin/sh"), dimensions(11, 50)).unwrap();
        first.resize(dimensions(17, 71)).unwrap();
        first
            .input(
                b"while [ \"$(stty size)\" != '17 71' ]; do sleep 0.01; done; pwd; stty size; printf 'FIRST_DONE\\n'\n"
                    .to_vec(),
            )
            .unwrap();
        second.input(b"pwd; stty size; printf 'SECOND_DONE\\n'\n".to_vec()).unwrap();
        let first_output = collect_until(&mut first, "FIRST_DONE\r\n");
        let second_output = collect_until(&mut second, "SECOND_DONE\r\n");
        assert!(first_output.contains(first_dir.path().to_str().unwrap()));
        assert!(first_output.contains("17 71"));
        assert!(!first_output.contains(second_dir.path().to_str().unwrap()));
        assert!(second_output.contains(second_dir.path().to_str().unwrap()));
        assert!(second_output.contains("11 50"));
        assert!(!second_output.contains(first_dir.path().to_str().unwrap()));
    }

    #[test]
    fn terminal_process_close_reports_reaped_shell() {
        let dir = tempfile::tempdir().unwrap();
        let mut process = TerminalProcess::spawn(dir.path(), Path::new("/bin/sh"), dimensions(10, 40)).unwrap();
        let mut other = TerminalProcess::spawn(dir.path(), Path::new("/bin/sh"), dimensions(12, 48)).unwrap();
        process.input(b"printf 'READY\\n'\n".to_vec()).unwrap();
        let _output = collect_until(&mut process, "READY\r\n");
        for columns in 40..1040 {
            process.resize(dimensions(10, columns)).unwrap();
        }
        process.close();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(Instant::now() < deadline, "terminal shell was not reaped");
            match process.try_recv() {
                Ok(TerminalProcessEvent::Exited(_)) => break,
                Ok(TerminalProcessEvent::Error(error)) => panic!("terminal close failed: {error}"),
                Ok(TerminalProcessEvent::Output(_) | TerminalProcessEvent::OutputClosed)
                | Err(mpsc::TryRecvError::Empty) => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("terminal events unavailable: {error}"),
            }
        }
        other.input(b"printf 'OTHER_ALIVE\\n'\n".to_vec()).unwrap();
        let output = collect_until(&mut other, "OTHER_ALIVE\r\n");
        assert!(output.contains("OTHER_ALIVE\r\n"));
    }

    #[cfg(unix)]
    fn sh_success(script: &str) -> bool {
        std::process::Command::new("sh")
            .arg("-c")
            .arg(script)
            .status()
            .is_ok_and(|status| status.success())
    }

    #[cfg(unix)]
    fn sh_output(script: &str) -> String {
        let output = std::process::Command::new("sh").arg("-c").arg(script).output().unwrap();
        assert!(output.status.success(), "probe failed: {script}");
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    #[cfg(unix)]
    fn process_group_id(pid: u32) -> u32 {
        sh_output(&format!("ps -o pgid= -p {pid}"))
            .trim()
            .parse()
            .expect("ps did not print a numeric pgid")
    }

    #[cfg(unix)]
    fn is_foreground(pid: u32) -> bool {
        sh_output(&format!("ps -o stat= -p {pid}")).contains('+')
    }

    #[cfg(unix)]
    fn is_alive(pid: u32) -> bool {
        sh_success(&format!("kill -0 {pid}"))
    }

    /// Best-effort reaper so a failed assertion never leaks the background sleep.
    #[cfg(unix)]
    struct KillOnDrop(Option<u32>);

    #[cfg(unix)]
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            if let Some(pid) = self.0 {
                let _status = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(format!("kill -9 {pid}"))
                    .status();
            }
        }
    }

    /// Parse the `MARKER:digits` value after the last marker. PTY echo shows the
    /// literal command (`BG_PID:$!`) before the real output, so only the last
    /// occurrence with real digits counts.
    #[cfg(unix)]
    fn parse_marker(output: &str, marker: &str) -> Option<u32> {
        let (_, tail) = output.rsplit_once(marker)?;
        let mut chars = tail.chars();
        let digits: String = chars.by_ref().take_while(char::is_ascii_digit).collect();
        // Accept only terminated runs: more digits may still be in flight.
        if digits.is_empty() || chars.next().is_none() {
            return None;
        }
        digits.parse::<u32>().ok()
    }

    #[cfg(unix)]
    fn collect_pids(process: &mut TerminalProcess) -> (u32, u32) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = String::new();
        loop {
            assert!(Instant::now() < deadline, "shell pids not observed: {output}");
            match process.try_recv() {
                Ok(TerminalProcessEvent::Output(bytes)) => output.push_str(&String::from_utf8_lossy(&bytes)),
                Ok(TerminalProcessEvent::OutputClosed | TerminalProcessEvent::Exited(_)) => {
                    panic!("terminal ended before pids observed: {output}")
                }
                Ok(TerminalProcessEvent::Error(error)) => panic!("unexpected terminal error: {error}"),
                Err(mpsc::TryRecvError::Empty) => thread::sleep(Duration::from_millis(10)),
                Err(error) => panic!("terminal events unavailable: {error}"),
            }
            if let (Some(shell), Some(child)) = (parse_marker(&output, "SHELL_PID:"), parse_marker(&output, "BG_PID:"))
            {
                return (shell, child);
            }
        }
    }

    /// Closing the terminal kills a foreground job in its own process group,
    /// not just the shell. The sleep ignores SIGHUP, so only the close path's
    /// SIGKILL explains its death; a sibling session must survive to prove the
    /// kill was targeted.
    #[cfg(unix)]
    #[test]
    fn terminal_process_close_kills_foreground_group_and_reaps_shell() {
        let dir = tempfile::tempdir().unwrap();
        let mut process = TerminalProcess::spawn(dir.path(), Path::new("/bin/sh"), dimensions(10, 40)).unwrap();
        let mut sibling = TerminalProcess::spawn(dir.path(), Path::new("/bin/sh"), dimensions(10, 40)).unwrap();
        process
            .input(b"echo SHELL_PID:$$; set -m; trap '' HUP; sleep 60 & echo BG_PID:$!\n".to_vec())
            .unwrap();
        let (shell_pid, bg_pid) = collect_pids(&mut process);
        let _reaper = KillOnDrop(Some(bg_pid));
        assert!(is_alive(bg_pid), "background sleep never started");
        let bg_group = process_group_id(bg_pid);
        assert_ne!(bg_group, shell_pid, "sleep shares the shell group; job control is off");
        process.input(b"fg\n".to_vec()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !is_foreground(bg_pid) {
            assert!(Instant::now() < deadline, "sleep never reached the foreground group");
            thread::sleep(Duration::from_millis(25));
        }
        process.close();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(Instant::now() < deadline, "terminal shell was not reaped");
            match process.try_recv() {
                Ok(TerminalProcessEvent::Exited(_)) => break,
                Ok(TerminalProcessEvent::Error(error)) => panic!("terminal close failed: {error}"),
                Ok(TerminalProcessEvent::Output(_) | TerminalProcessEvent::OutputClosed)
                | Err(mpsc::TryRecvError::Empty) => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("terminal events unavailable: {error}"),
            }
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while is_alive(bg_pid) {
            assert!(Instant::now() < deadline, "foreground sleep survived close");
            thread::sleep(Duration::from_millis(25));
        }
        sibling.input(b"printf 'SIBLING_ALIVE\\n'\n".to_vec()).unwrap();
        let output = collect_until(&mut sibling, "SIBLING_ALIVE\r\n");
        assert!(output.contains("SIBLING_ALIVE\r\n"));
    }

    #[cfg(unix)]
    #[test]
    fn parse_marker_ignores_echo_and_partial_chunks() {
        assert_eq!(parse_marker("echo BG_PID:$!\n", "BG_PID:"), None);
        assert_eq!(parse_marker("BG_PID:$!\nBG_PID:12345\r\n", "BG_PID:"), Some(12345));
        assert_eq!(parse_marker("noise BG_PID:123", "BG_PID:"), None);
        assert_eq!(parse_marker("BG_PID:", "BG_PID:"), None);
    }

    #[test]
    fn default_shell_never_returns_empty() {
        assert!(!default_shell().as_os_str().is_empty());
    }

    /// Accumulate output until one line shows the shell's own argv (`sh` plus
    /// `-l`). The input echo contains neither, so only real `ps` output
    /// completes; chunk splits resolve by accumulation, not timing.
    #[cfg(unix)]
    fn collect_shell_argv(process: &mut TerminalProcess) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = String::new();
        loop {
            assert!(Instant::now() < deadline, "shell argv not observed: {output}");
            match process.try_recv() {
                Ok(TerminalProcessEvent::Output(bytes)) => output.push_str(&String::from_utf8_lossy(&bytes)),
                Ok(TerminalProcessEvent::OutputClosed | TerminalProcessEvent::Exited(_)) => {
                    panic!("terminal ended before argv observed: {output}")
                }
                Ok(TerminalProcessEvent::Error(error)) => panic!("unexpected terminal error: {error}"),
                Err(mpsc::TryRecvError::Empty) => thread::sleep(Duration::from_millis(10)),
                Err(error) => panic!("terminal events unavailable: {error}"),
            }
            if output.split('\n').any(|line| line.contains("sh") && line.contains("-l")) {
                return output;
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn terminal_process_login_shell_passes_profile_flag() {
        let dir = tempfile::tempdir().unwrap();
        let mut process = TerminalProcess::spawn_login(dir.path(), Path::new("/bin/sh"), dimensions(10, 40)).unwrap();
        process.input(b"ps -o args= -p $$; echo ARGS_DONE\n".to_vec()).unwrap();
        let output = collect_shell_argv(&mut process);
        assert!(output.contains("-l"), "login flag missing from shell argv: {output}");
    }

    /// Paced volume delivery: batched input with progress sync arrives complete
    /// and ordered, mirroring how paced pastes stay under kernel TTY limits.
    #[test]
    fn terminal_process_paced_volume_arrives_complete_and_ordered() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("flood.log");
        let process = TerminalProcess::spawn(dir.path(), Path::new("/bin/sh"), dimensions(10, 40)).unwrap();
        const LINES: usize = 1000;
        const BATCH: usize = 20;
        let expected: String = (0..LINES).map(|index| format!("LINE{index:04}\n")).collect();
        let deadline = Instant::now() + Duration::from_secs(30);
        for batch in 0..LINES / BATCH {
            for index in batch * BATCH..(batch + 1) * BATCH {
                process
                    .input(format!("echo LINE{index:04} >> flood.log\n").into_bytes())
                    .unwrap();
            }
            let want = expected.len() * (batch + 1) * BATCH / LINES;
            loop {
                // Drain output like the UI pump does; an undrained reader would
                // backpressure the PTY echo path and stall the flood.
                while process.try_recv().is_ok() {}
                let content = std::fs::read_to_string(&log).unwrap_or_default();
                if content.len() >= want {
                    break;
                }
                assert!(Instant::now() < deadline, "shell stalled at batch {batch}");
                thread::sleep(Duration::from_millis(10));
            }
        }
        assert_eq!(std::fs::read_to_string(&log).unwrap(), expected);
    }
}
