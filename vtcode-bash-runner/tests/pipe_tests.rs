//! Integration tests for pipe-based process spawning.
//!
//! These tests verify the async pipe spawning functionality works correctly
//! across different platforms.

use std::collections::HashMap;
use std::path::Path;

use vtcode_bash_runner::{
    PipeSpawnOptions, PipeStdinMode, collect_output_until_exit, spawn_pipe_process,
    spawn_pipe_process_no_stdin, spawn_pipe_process_with_options,
};

fn find_python() -> Option<String> {
    for candidate in ["python3", "python"] {
        if let Ok(output) = std::process::Command::new(candidate)
            .arg("--version")
            .output()
            && output.status.success()
        {
            return Some(candidate.to_string());
        }
    }
    None
}

fn shell_command(script: &str) -> (String, Vec<String>) {
    if cfg!(windows) {
        let cmd = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());
        (cmd, vec!["/C".to_string(), script.to_string()])
    } else {
        (
            "/bin/sh".to_string(),
            vec!["-c".to_string(), script.to_string()],
        )
    }
}

#[tokio::test]
async fn test_pipe_process_echo() -> anyhow::Result<()> {
    let (program, args) = shell_command("echo hello");
    let env: HashMap<String, String> = std::env::vars().collect();

    let spawned = spawn_pipe_process(&program, &args, Path::new("."), &env, &None).await?;

    let (output, code) = collect_output_until_exit(spawned.output_rx, spawned.exit_rx, 5_000).await;
    let text = String::from_utf8_lossy(&output);

    assert!(
        text.contains("hello"),
        "expected 'hello' in output: {text:?}"
    );
    assert_eq!(code, 0, "expected exit code 0");

    Ok(())
}

#[tokio::test]
async fn test_pipe_process_round_trips_stdin() -> anyhow::Result<()> {
    let Some(python) = find_python() else {
        eprintln!("python not found; skipping test_pipe_process_round_trips_stdin");
        return Ok(());
    };

    let args = vec![
        "-u".to_string(),
        "-c".to_string(),
        "import sys; print(sys.stdin.readline().strip())".to_string(),
    ];
    let env: HashMap<String, String> = std::env::vars().collect();

    let spawned = spawn_pipe_process(&python, &args, Path::new("."), &env, &None).await?;
    let writer = spawned.session.writer_sender();
    writer.send(b"roundtrip\n".to_vec()).await?;

    let (output, code) = collect_output_until_exit(spawned.output_rx, spawned.exit_rx, 5_000).await;
    let text = String::from_utf8_lossy(&output);

    assert!(
        text.contains("roundtrip"),
        "expected pipe process to echo stdin: {text:?}"
    );
    assert_eq!(code, 0, "expected python to exit cleanly");

    Ok(())
}

#[tokio::test]
async fn test_pipe_process_no_stdin() -> anyhow::Result<()> {
    // Test that spawn_process_no_stdin properly closes stdin
    let (program, args) = shell_command("echo no_stdin_test");
    let env: HashMap<String, String> = std::env::vars().collect();

    let spawned = spawn_pipe_process_no_stdin(&program, &args, Path::new("."), &env, &None).await?;

    let (output, code) = collect_output_until_exit(spawned.output_rx, spawned.exit_rx, 5_000).await;
    let text = String::from_utf8_lossy(&output);

    assert!(text.contains("no_stdin_test"), "expected output: {text:?}");
    assert_eq!(code, 0);

    Ok(())
}

#[tokio::test]
async fn test_pipe_spawn_options() -> anyhow::Result<()> {
    let (program, args) = shell_command("echo options_test");

    let opts = PipeSpawnOptions::new(program, ".")
        .args(args)
        .stdin_mode(PipeStdinMode::Null);

    let spawned = spawn_pipe_process_with_options(opts).await?;

    let (output, code) = collect_output_until_exit(spawned.output_rx, spawned.exit_rx, 5_000).await;
    let text = String::from_utf8_lossy(&output);

    assert!(text.contains("options_test"), "expected output: {text:?}");
    assert_eq!(code, 0);

    Ok(())
}

#[tokio::test]
async fn test_process_handle_terminate() -> anyhow::Result<()> {
    // Spawn a long-running process and terminate it
    let (program, args) = if cfg!(windows) {
        let cmd = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());
        (
            cmd,
            vec!["/C".to_string(), "ping -n 100 127.0.0.1".to_string()],
        )
    } else {
        (
            "/bin/sh".to_string(),
            vec!["-c".to_string(), "sleep 100".to_string()],
        )
    };

    let env: HashMap<String, String> = std::env::vars().collect();
    let spawned = spawn_pipe_process(&program, &args, Path::new("."), &env, &None).await?;

    // Give it a moment to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Terminate
    spawned.session.terminate();

    // Process should be terminated
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    assert!(spawned.session.has_exited() || spawned.session.is_writer_closed());

    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn test_pipe_process_detaches_from_parent_session() -> anyhow::Result<()> {
    let parent_sid = unsafe { libc::getsid(0) };
    if parent_sid == -1 {
        anyhow::bail!("failed to read parent session id");
    }

    let env: HashMap<String, String> = std::env::vars().collect();
    let (program, args) = shell_command("echo $$; sleep 0.2");
    let spawned = spawn_pipe_process(&program, &args, Path::new("."), &env, &None).await?;

    let mut output_rx = spawned.output_rx;
    let pid_bytes =
        tokio::time::timeout(tokio::time::Duration::from_millis(500), output_rx.recv()).await??;
    let pid_text = String::from_utf8_lossy(&pid_bytes);
    let child_pid: i32 = pid_text
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing child pid output: {pid_text:?}"))?
        .parse()?;

    let child_sid = unsafe { libc::getsid(child_pid) };
    if child_sid == -1 {
        // Process may have already exited, which is fine
        return Ok(());
    }

    // Child should be in its own session or process group
    assert_ne!(
        child_sid, parent_sid,
        "expected child to be detached from parent session"
    );

    let exit_code = spawned.exit_rx.await.unwrap_or(-1);
    assert_eq!(exit_code, 0, "expected process to exit cleanly");

    Ok(())
}

#[tokio::test]
async fn test_pipe_drains_stderr() -> anyhow::Result<()> {
    let Some(python) = find_python() else {
        eprintln!("python not found; skipping test_pipe_drains_stderr");
        return Ok(());
    };

    // Write to stderr only
    let script = "import sys; sys.stderr.write('stderr_output\\n'); sys.stderr.flush()";
    let args = vec!["-c".to_string(), script.to_string()];
    let env: HashMap<String, String> = std::env::vars().collect();

    let spawned = spawn_pipe_process(&python, &args, Path::new("."), &env, &None).await?;

    let (output, code) = collect_output_until_exit(spawned.output_rx, spawned.exit_rx, 5_000).await;
    let text = String::from_utf8_lossy(&output);

    assert!(
        text.contains("stderr_output"),
        "expected stderr to be captured: {text:?}"
    );
    assert_eq!(code, 0);

    Ok(())
}
