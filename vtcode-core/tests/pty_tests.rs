use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use portable_pty::PtySize;
use tempfile::tempdir;

use vtcode_core::config::PtyConfig;
use vtcode_core::tools::{PtyCommandRequest, PtyManager};

fn shell_command(script: &str) -> Vec<String> {
    if cfg!(windows) {
        let cmd = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());
        vec![cmd, "/C".to_string(), script.to_string()]
    } else {
        vec!["sh".to_string(), "-c".to_string(), script.to_string()]
    }
}

#[tokio::test]
async fn run_pty_command_captures_output() -> Result<()> {
    let temp_dir = tempdir()?;
    let manager = PtyManager::new(temp_dir.path().to_path_buf(), PtyConfig::default());

    let working_dir = manager.resolve_working_dir(Some(".")).await?;
    let request = PtyCommandRequest {
        command: vec![
            "sh".to_string(),
            "-c".to_string(),
            "printf 'hello from pty'".to_string(),
        ],
        working_dir,
        timeout: Duration::from_secs(5),
        size: PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        },
        max_tokens: None,
        output_callback: None,
    };

    let result = manager.run_command(request).await?;
    assert_eq!(result.exit_code, 0);
    assert!(result.output.contains("hello from pty"));

    Ok(())
}

#[tokio::test]
async fn create_list_and_close_session_preserves_screen_contents() -> Result<()> {
    let temp_dir = tempdir()?;
    let manager = PtyManager::new(temp_dir.path().to_path_buf(), PtyConfig::default());

    let working_dir = manager.resolve_working_dir(Some(".")).await?;
    let size = PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };

    let session_id = "session-test".to_string();
    manager.create_session(
        session_id.clone(),
        vec![
            "sh".to_string(),
            "-c".to_string(),
            "printf ready && sleep 0.1".to_string(),
        ],
        working_dir,
        size,
        HashMap::new(),
        None,
    )?;

    std::thread::sleep(Duration::from_millis(150));

    let sessions = manager.list_sessions();
    assert_eq!(sessions.len(), 1);
    let snapshot = &sessions[0];
    assert_eq!(snapshot.id, session_id);
    assert!(
        snapshot
            .screen_contents
            .as_deref()
            .map(|contents| contents.contains("ready"))
            .unwrap_or(false)
    );
    assert!(
        snapshot
            .scrollback
            .as_deref()
            .map(|contents| contents.contains("ready"))
            .unwrap_or(false)
    );

    let closed = manager.close_session(&session_id)?;
    assert!(
        closed
            .screen_contents
            .as_deref()
            .map(|contents| contents.contains("ready"))
            .unwrap_or(false)
    );
    assert!(
        closed
            .scrollback
            .as_deref()
            .map(|contents| contents.contains("ready"))
            .unwrap_or(false)
    );

    Ok(())
}

#[tokio::test]
async fn resolve_working_dir_rejects_missing_directory() {
    let temp_dir = tempdir().unwrap();
    let manager = PtyManager::new(temp_dir.path().to_path_buf(), PtyConfig::default());

    let error = manager.resolve_working_dir(Some("missing")).await;
    assert!(error.unwrap_err().to_string().contains("does not exist"));
}

#[tokio::test]
async fn session_input_roundtrip_and_resize() -> Result<()> {
    let temp_dir = tempdir()?;
    let mut config = PtyConfig::default();
    config.scrollback_lines = 200;
    let manager = PtyManager::new(temp_dir.path().to_path_buf(), config);

    let working_dir = manager.resolve_working_dir(Some(".")).await?;
    let size = PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };

    let session_id = "roundtrip".to_string();
    manager.create_session(
        session_id.clone(),
        vec![
            "sh".to_string(),
            "-c".to_string(),
            "while read line; do if [ \"$line\" = \"exit\" ]; then break; fi; printf 'got:%s\\n' \"$line\"; done".to_string(),
        ],
        working_dir,
        size,
        HashMap::new(),
        None,
    )?;

    std::thread::sleep(Duration::from_millis(150));

    manager.send_input_to_session(&session_id, b"hello", true)?;
    std::thread::sleep(Duration::from_millis(150));

    let drained = manager.read_session_output(&session_id, true)?;
    let drained_text = drained.as_deref().expect("expected drained output");
    assert!(drained_text.contains("got:hello"));

    assert!(manager.read_session_output(&session_id, false)?.is_none());

    manager.send_input_to_session(&session_id, b"world", true)?;
    std::thread::sleep(Duration::from_millis(150));

    let peek = manager.read_session_output(&session_id, false)?;
    let peek_text = peek
        .as_deref()
        .expect("expected pending output")
        .to_string();
    assert!(peek_text.contains("got:world"));

    let drained_again = manager.read_session_output(&session_id, true)?;
    let drained_again_text = drained_again
        .as_deref()
        .expect("expected drained output after peek");
    assert!(drained_again_text.contains("got:world"));

    let updated = manager.resize_session(
        &session_id,
        PtySize {
            rows: 48,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        },
    )?;
    assert_eq!(updated.rows, 48);
    assert_eq!(updated.cols, 120);

    let snapshot = manager.snapshot_session(&session_id)?;
    let scrollback = snapshot
        .scrollback
        .as_deref()
        .expect("scrollback should be present");
    assert!(scrollback.contains("got:hello"));
    assert!(scrollback.contains("got:world"));

    manager.close_session(&session_id)?;

    Ok(())
}

#[tokio::test]
async fn run_pty_command_applies_max_tokens_truncation() -> Result<()> {
    let temp_dir = tempdir()?;
    let manager = PtyManager::new(temp_dir.path().to_path_buf(), PtyConfig::default());

    let working_dir = manager.resolve_working_dir(Some(".")).await?;
    let script = if cfg!(windows) {
        "echo 012345678901234567890123456789012345678901234567890123456789"
    } else {
        "printf '012345678901234567890123456789012345678901234567890123456789'"
    };
    let request = PtyCommandRequest {
        command: shell_command(script),
        working_dir,
        timeout: Duration::from_secs(5),
        size: PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        },
        max_tokens: Some(8),
        output_callback: None,
    };

    let result = manager.run_command(request).await?;
    assert_eq!(result.exit_code, 0);
    assert!(result.output.contains("[... truncated by max_tokens ...]"));

    Ok(())
}

#[tokio::test]
async fn run_pty_command_returns_timeout_error() -> Result<()> {
    let temp_dir = tempdir()?;
    let manager = PtyManager::new(temp_dir.path().to_path_buf(), PtyConfig::default());

    let working_dir = manager.resolve_working_dir(Some(".")).await?;
    let script = if cfg!(windows) {
        "ping -n 5 127.0.0.1 > nul"
    } else {
        "sleep 2"
    };
    let request = PtyCommandRequest {
        command: shell_command(script),
        working_dir,
        timeout: Duration::from_millis(200),
        size: PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        },
        max_tokens: None,
        output_callback: None,
    };

    let error = match manager.run_command(request).await {
        Ok(_) => anyhow::bail!("expected timeout error"),
        Err(error) => error,
    };
    let message = error.to_string();
    assert!(
        message.contains("timed out"),
        "unexpected timeout error: {message}"
    );

    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn pty_terminate_kills_background_children_in_same_process_group() -> Result<()> {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;

    let temp_dir = tempdir()?;
    let manager = PtyManager::new(temp_dir.path().to_path_buf(), PtyConfig::default());

    let working_dir = manager.resolve_working_dir(Some(".")).await?;
    let size = PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };

    let session_id = "background-test".to_string();
    manager.create_session(
        session_id.clone(),
        vec![
            "sh".to_string(),
            "-c".to_string(),
            "sleep 1000 & echo \"bg_pid:$!\"; wait".to_string(),
        ],
        working_dir,
        size,
        HashMap::new(),
        None,
    )?;

    // Wait for the background process to be spawned and its PID to be printed
    let mut bg_pid: Option<i32> = None;
    for _ in 0..20 {
        if let Ok(Some(output)) = manager.read_session_output(&session_id, false) {
            if let Some(line) = output.lines().find(|l| l.contains("bg_pid:")) {
                if let Some(pid_str) = line.split(':').last() {
                    if let Ok(pid) = pid_str.trim().parse::<i32>() {
                        bg_pid = Some(pid);
                        break;
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let bg_pid = bg_pid.expect("Failed to capture background PID");
    let pid = Pid::from_raw(bg_pid);

    // Verify background process is running
    assert!(
        kill(pid, None).is_ok(),
        "Background process should be running"
    );

    // Close session, which should kill the process group
    manager.close_session(&session_id)?;

    // Verify background process is killed (may need a short wait for signal to propagate)
    let mut killed = false;
    for _ in 0..10 {
        if kill(pid, None).is_err() {
            killed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    assert!(killed, "Background process should have been killed");

    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn run_pty_command_timeout_kills_background_children() -> Result<()> {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;
    use std::fs;

    let temp_dir = tempdir()?;
    let manager = PtyManager::new(temp_dir.path().to_path_buf(), PtyConfig::default());

    let working_dir = manager.resolve_working_dir(Some(".")).await?;

    // Create a temporary file to store the background PID
    let pid_file = temp_dir.path().join("bg_pid.txt");

    // Script that spawns a background process and writes its PID to a file, then sleeps
    let script = format!("sleep 1000 & echo $! > {}; sleep 5", pid_file.display());

    let request = PtyCommandRequest {
        command: vec!["sh".to_string(), "-c".to_string(), script],
        working_dir,
        timeout: Duration::from_millis(500), // Short timeout to trigger kill
        size: PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        },
        max_tokens: None,
        output_callback: None,
    };

    // run_command should timeout and kill the process group
    let result = manager.run_command(request).await;
    let err = match result {
        Ok(_) => anyhow::bail!("Expected timeout error"),
        Err(e) => e,
    };
    assert!(err.to_string().contains("timed out"));

    // Give a small amount of time for the background process to write the file if it hasn't yet
    // though the timeout is 500ms, which should be enough for 'echo $! > file'
    std::thread::sleep(Duration::from_millis(100));

    // Read the background PID
    let bg_pid_str = fs::read_to_string(&pid_file).expect("Failed to read PID file");
    let bg_pid = bg_pid_str
        .trim()
        .parse::<i32>()
        .expect("Failed to parse PID");
    let pid = Pid::from_raw(bg_pid);

    // Verify background process is killed
    let mut killed = false;
    for _ in 0..10 {
        if kill(pid, None).is_err() {
            killed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    assert!(
        killed,
        "Background process should have been killed by run_command timeout"
    );

    Ok(())
}
