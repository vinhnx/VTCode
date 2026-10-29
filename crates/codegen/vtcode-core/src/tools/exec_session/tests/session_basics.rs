use super::*;

#[test]
fn completion_command_is_single_line_redacted_and_bounded() {
    let secret = concat!("password=", "supersecretvalue");
    let command = format!("cargo\ncheck\u{1b} {secret} {}", "x".repeat(1_024));

    let bounded = bounded_completion_command(&command);

    assert!(bounded.starts_with("cargo check "));
    assert!(!bounded.chars().any(char::is_control));
    assert!(!bounded.contains("supersecretvalue"));
    assert!(bounded.contains("[REDACTED_SECRET]"));
    assert!(bounded.len() <= EXEC_SESSION_COMPLETION_COMMAND_MAX_BYTES);
    assert!(bounded.ends_with("..."));
}
#[test]
fn completion_command_preserves_short_safe_labels() {
    assert_eq!(bounded_completion_command("cargo check --locked"), "cargo check --locked");
}
#[tokio::test]
#[cfg(all(unix, feature = "tui"))]
async fn pty_session_limit_holds_until_exec_session_close() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions =
        PtySessionManager::new(workspace_root.clone(), PtyConfig { max_sessions: 1, ..Default::default() });
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);
    let size = PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };

    manager
        .create_pty_session(
            "run-1".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 1".to_string()],
            workspace_root.clone(),
            size,
            HashMap::new(),
            None,
        )
        .await?;

    let second = manager
        .create_pty_session(
            "run-2".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 1".to_string()],
            workspace_root.clone(),
            size,
            HashMap::new(),
            None,
        )
        .await;
    assert!(second.is_err());
    assert!(second.unwrap_err().to_string().contains("Maximum PTY sessions"));

    manager.close_session("run-1").await?;
    manager
        .create_pty_session(
            "run-3".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 1".to_string()],
            workspace_root,
            size,
            HashMap::new(),
            None,
        )
        .await?;
    manager.close_session("run-3").await?;

    Ok(())
}
#[tokio::test]
#[cfg(unix)]
async fn pipe_session_activity_receiver_notifies_on_output() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);

    manager
        .create_pipe_session(
            "run-1".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "printf hello".to_string()],
            workspace_root,
            HashMap::new(),
        )
        .await?;

    let mut activity_rx = manager
        .activity_receiver("run-1")
        .await?
        .expect("pipe sessions should expose activity receiver");

    let output = timeout(Duration::from_secs(2), async {
        loop {
            if let Some(output) = manager.read_session_output("run-1", true).await? {
                return Ok::<String, anyhow::Error>(output);
            }
            activity_rx.changed().await?;
        }
    })
    .await??;
    assert!(output.contains("hello"));

    manager.close_session("run-1").await?;
    Ok(())
}
/// A sandbox-active pipe session rebuilds the child environment from the central
/// allowlist: a planted credential and an arbitrary caller variable cannot reach
/// the process, while allowlisted names and the sandbox markers do.
#[tokio::test]
#[cfg(unix)]
async fn sandbox_active_pipe_session_hands_the_child_only_the_allowlisted_environment() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);

    let mut env = HashMap::new();
    drop(env.insert("PATH".to_string(), "/usr/bin:/bin".to_string()));
    drop(env.insert("OPENAI_API_KEY".to_string(), "sk-planted-never-leaks".to_string()));
    drop(env.insert("VTCODE_PROBE_NEVER_LEAKS".to_string(), "probe".to_string()));
    drop(env.insert("PYTHONUNBUFFERED".to_string(), "1".to_string()));
    drop(env.insert("GIT_TERMINAL_PROMPT".to_string(), "0".to_string()));

    manager
        .create_pipe_session_with_sandbox_and_background(
            "env-1".to_string().into(),
            vec!["/usr/bin/env".to_string()],
            workspace_root,
            env,
            true,
            false,
        )
        .await?;

    let mut activity_rx = manager
        .activity_receiver("env-1")
        .await?
        .expect("pipe sessions should expose activity receiver");
    let output = timeout(Duration::from_secs(5), async {
        loop {
            if let Some(output) = manager.read_session_output("env-1", true).await? {
                return Ok::<String, anyhow::Error>(output);
            }
            activity_rx.changed().await?;
        }
    })
    .await??;
    manager.close_session("env-1").await?;

    // An allowlisted inherited name survives the rebuild ...
    assert!(output.contains("PATH=/usr/bin:/bin"), "allowlisted PATH must survive: {output}");
    // ... while credentials and arbitrary caller variables do not.
    assert!(!output.contains("OPENAI_API_KEY"), "credential names must not reach the child: {output}");
    assert!(!output.contains("sk-planted-never-leaks"), "credential values must not reach the child: {output}");
    assert!(
        !output.contains("VTCODE_PROBE_NEVER_LEAKS"),
        "non-allowlisted caller variables must not reach the child: {output}"
    );
    // The markers prove the sanitized launch ran instead of the child inheriting
    // whatever the caller happened to hold.
    assert!(output.contains("VTCODE_SANDBOX_ACTIVE=1"), "sandbox marker missing: {output}");
    assert!(output.contains("VTCODE_SANDBOX_TYPE=exec-session"), "sandbox type marker missing: {output}");
    // Pipeline hygiene the pipe launcher sets for streaming builds has to survive
    // the same rebuild: unbuffered output keeps live previews streaming, and the
    // git prompt guard keeps builds from stalling on a credential prompt.
    assert!(output.contains("PYTHONUNBUFFERED=1"), "streaming hygiene lost in rebuild: {output}");
    assert!(output.contains("GIT_TERMINAL_PROMPT=0"), "prompt-guard hygiene lost in rebuild: {output}");
    Ok(())
}
#[tokio::test]
#[cfg(unix)]
async fn concurrent_pipe_session_create_with_same_id_creates_exactly_one() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);

    let (a, b) = tokio::join!(
        manager.create_pipe_session(
            "same-id".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
            workspace_root.clone(),
            HashMap::new(),
        ),
        manager.create_pipe_session(
            "same-id".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
            workspace_root.clone(),
            HashMap::new(),
        ),
    );

    assert_eq!(a.is_ok() as u8 + b.is_ok() as u8, 1, "exactly one concurrent create must win: {a:?} {b:?}");
    let loser = a.err().or_else(|| b.err()).expect("the loser should error");
    assert!(loser.to_string().contains("already exists"), "loser error should report duplicate: {loser}");

    manager.close_session("same-id").await?;
    Ok(())
}
