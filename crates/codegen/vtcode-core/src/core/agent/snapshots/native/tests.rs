use super::*;
use crate::llm::provider::MessageRole;
use tempfile::TempDir;

#[test]
#[cfg(unix)]
fn rewind_lock_acquire_verifies_and_blocks_concurrent_holders() {
    let temp = TempDir::new().expect("tempdir");
    let lock_path = temp.path().join("rewind.lock");

    let first = acquire_verified_rewind_lock(&lock_path).expect("first acquire");
    assert!(
        locked_file_matches_path(&first, &lock_path),
        "a fresh acquisition must cover the current path inode"
    );

    // A live holder blocks other acquirers (WouldBlock), preserving the
    // pre-existing mutual-exclusion behavior.
    let second = acquire_verified_rewind_lock(&lock_path);
    let error = second.expect_err("held lock must block");
    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);

    drop(first);
    let third = acquire_verified_rewind_lock(&lock_path).expect("re-acquire after release");
    assert!(locked_file_matches_path(&third, &lock_path));
}

#[test]
#[cfg(unix)]
fn rewind_lock_detects_replaced_path_inode() {
    let temp = TempDir::new().expect("tempdir");
    let lock_path = temp.path().join("rewind.lock");
    fs::write(&lock_path, b"").expect("create lock file");
    let ghost = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .expect("open old inode");

    // The completion path's unlink-while-held replaced the file: the old
    // fd is now a ghost and must not be treated as covering the path.
    fs::remove_file(&lock_path).expect("unlink");
    fs::write(&lock_path, b"").expect("recreate lock file");
    assert!(
        !locked_file_matches_path(&ghost, &lock_path),
        "a ghost fd must not verify against the replaced path"
    );

    // The acquire helper never accepts a ghost: it retries onto the
    // current file even while the ghost is still open.
    let verified = acquire_verified_rewind_lock(&lock_path).expect("acquire onto current inode");
    assert!(locked_file_matches_path(&verified, &lock_path));
}

#[test]
#[cfg(unix)]
fn shell_redirects_only_capture_literal_targets_with_a_known_cwd() {
    let paths = |script: &str| shell_redirect_paths(&serde_json::json!({"cmd": script}));
    assert_eq!(paths("printf 'hello' > hello.py && cat hello.py"), BTreeSet::from([PathBuf::from("hello.py")]));
    assert_eq!(
        paths("cat > 'hello world.py' <<'PY'\nprint('> not-a-path')\nPY"),
        BTreeSet::from([PathBuf::from("hello world.py")])
    );
    assert_eq!(paths("printf hi >> log.txt 2>&1"), BTreeSet::from([PathBuf::from("log.txt")]));
    for script in [
        "cat hello.py",
        "cat < input.txt",
        "printf '> innocent.txt'",
        "echo hi 2>&1",
        "echo hi > $OUTPUT",
        "echo hi > $(pwd)/hello.py",
        "echo hi > ~/hello.py",
        "cd nested && echo hi > hello.py",
        "command cd nested; echo hi > hello.py",
        "eval 'cd nested'; echo hi > hello.py",
        "f() { echo hi > hello.py; }; f",
        "echo hi >",
    ] {
        assert!(paths(script).is_empty(), "must not guess paths for {script}");
    }
}

#[tokio::test]
async fn cancelled_prompt_preparation_never_admits_a_lease_and_a_fresh_prompt_can_start() -> Result<()> {
    let dir = TempDir::new()?;
    let manager = SnapshotManager::new(SnapshotConfig::new(dir.path().into()))?;
    let session = uuid::Uuid::new_v4().to_string();
    let cancellation = tokio_util::sync::CancellationToken::new();
    cancellation.cancel();
    assert!(
        manager
            .begin_prompt_with_cancellation(1, &session, "cancelled", &[], cancellation)
            .await?
            .is_none()
    );
    assert!(!active_map().lock().unwrap().contains_key(&session));
    let lease = manager.begin_prompt(2, &session, "fresh", &[]).await?;
    assert!(active_map().lock().unwrap().contains_key(&session));
    assert!(manager.begin_prompt(3, &session, "must not overlap", &[]).await.is_err());
    drop(lease);
    assert!(!active_map().lock().unwrap().contains_key(&session));
    let _next_lease = manager.begin_prompt(3, &session, "after release", &[]).await?;
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn shell_preimages_respect_ignore_and_workspace_boundaries() -> Result<()> {
    let dir = TempDir::new()?;
    let outside = TempDir::new()?;
    fs::write(dir.path().join(".filesnapignore"), "ignored.txt\n")?;
    let manager = SnapshotManager::new(SnapshotConfig::new(dir.path().into()))?;
    let session = uuid::Uuid::new_v4().to_string();
    let lease = manager.begin_prompt(1, &session, "create files", &[]).await?;
    let outside_file = outside.path().join("outside.txt");
    let script = format!("echo hi > ignored.txt; echo hi > {}", shell_words::quote(&outside_file.to_string_lossy()));
    declare_prompt_edit(session.clone(), "exec_command".into(), serde_json::json!({"cmd":script})).await?;
    fs::write(dir.path().join("ignored.txt"), "ignored")?;
    fs::write(&outside_file, "outside")?;
    // No input or display text should be mistaken for a shell write.
    declare_prompt_edit(session.clone(), "exec_command".into(), serde_json::json!({"cmd":"cat ignored.txt"})).await?;
    assert!(manager.load_snapshot(1).await?.expect("checkpoint").files.is_empty());
    drop(lease);
    manager.navigate_prompt(Some(1), RevertScope::Both, &session, &[]).await?;
    assert_eq!(fs::read_to_string(dir.path().join("ignored.txt"))?, "ignored");
    assert_eq!(fs::read_to_string(outside_file)?, "outside");
    let _lease = manager.begin_prompt(2, &session, "next prompt", &[]).await?;
    assert!(manager.load_snapshot(2).await?.expect("next checkpoint").files.is_empty());
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn shell_creation_rewind_redo_then_rewind_creation_removes_file() -> Result<()> {
    let dir = TempDir::new()?;
    let manager = SnapshotManager::new(SnapshotConfig::new(dir.path().into()))?;
    let session = uuid::Uuid::new_v4().to_string();
    let nested = dir.path().join("nested");
    fs::create_dir(&nested)?;
    let file = nested.join("hello.py");
    let neighbor = dir.path().join("hello.py");
    fs::write(&neighbor, "unrelated existing file")?;
    let original = vec![SessionMessage::new(MessageRole::User, "pwd")];
    let created = vec![SessionMessage::new(MessageRole::User, "add a hello world py")];
    let current = vec![SessionMessage::new(MessageRole::User, "add greetings")];
    let hello = "print(\"Hello, World!\")\n";
    let greetings = "print(\"Hello, Ada!\")\n";

    let lease = manager.begin_prompt(1, &session, "add a hello world py", &original).await?;
    let script = "printf 'print(\"Hello, World!\")\\n' > hello.py && cat hello.py";
    declare_prompt_edit(session.clone(), "exec_command".into(), serde_json::json!({"cmd":script,"workdir":"nested"}))
        .await?;
    let output = tokio::process::Command::new("sh")
        .args(["-c", script])
        .current_dir(&nested)
        .output()
        .await?;
    assert!(output.status.success());
    assert_eq!(fs::read_to_string(&file)?, hello);
    // A subsequent write in the same prompt must retain the first absence.
    declare_prompt_edit(session.clone(), "exec_command".into(), serde_json::json!({"cmd":script,"workdir":"nested"}))
        .await?;
    assert!(
        manager
            .load_snapshot(1)
            .await?
            .expect("creation checkpoint")
            .files
            .iter()
            .any(|f| f.path == "nested/hello.py" && f.deleted)
    );
    drop(lease);

    let lease = manager.begin_prompt(2, &session, "add greetings", &created).await?;
    let script = "cat > hello.py <<'PY'\nprint(\"Hello, Ada!\")\nPY";
    declare_prompt_edit(
        session.clone(),
        "unified_exec".into(),
        serde_json::json!({"action":"run","command":script,"working_dir":nested}),
    )
    .await?;
    let output = tokio::process::Command::new("sh")
        .args(["-c", script])
        .current_dir(&nested)
        .output()
        .await?;
    assert!(output.status.success());
    assert_eq!(fs::read_to_string(&file)?, greetings);
    drop(lease);
    // A file never declared or captured must not be swept away by rewind.
    fs::write(dir.path().join("unrelated.txt"), "leave me alone")?;

    let restored = manager.navigate_prompt(Some(2), RevertScope::Both, &session, &current).await?;
    assert_eq!(restored.conversation, created);
    assert_eq!(fs::read_to_string(&file)?, hello);
    let restored = manager
        .navigate_prompt(None, RevertScope::Both, &session, &restored.conversation)
        .await?;
    assert_eq!(restored.conversation, current);
    assert_eq!(fs::read_to_string(&file)?, greetings);
    let restored = manager
        .navigate_prompt(Some(1), RevertScope::Both, &session, &restored.conversation)
        .await?;
    assert_eq!(restored.conversation, original);
    assert!(!file.exists());
    assert_eq!(fs::read_to_string(&neighbor)?, "unrelated existing file");
    assert_eq!(fs::read_to_string(dir.path().join("unrelated.txt"))?, "leave me alone");
    let resumed = SnapshotManager::new(SnapshotConfig::new(dir.path().into()))?;
    let restored = resumed
        .navigate_prompt(None, RevertScope::Both, &session, &restored.conversation)
        .await?;
    assert_eq!(restored.conversation, current);
    assert_eq!(fs::read_to_string(&file)?, greetings);
    Ok(())
}

#[tokio::test]
async fn native_history_restores_prompt_prefix_binary_assets_and_nested_redo() -> Result<()> {
    let dir = TempDir::new()?;
    let manager = SnapshotManager::new(SnapshotConfig::new(dir.path().into()))?;
    let session = uuid::Uuid::new_v4().to_string();
    let asset = dir.path().join("asset.bin");
    fs::write(&asset, [0, 255, 7])?;
    let original = vec![SessionMessage::new(MessageRole::User, "original")];
    let lease = manager.begin_prompt(1, &session, "first", &original).await?;
    fs::write(&asset, "first result")?;
    drop(lease);
    let second = vec![SessionMessage::new(MessageRole::User, "second")];
    let lease = manager.begin_prompt(2, &session, "second", &second).await?;
    declare_prompt_edit(session.clone(), "write_file".into(), serde_json::json!({"path":".created"})).await?;
    fs::write(dir.path().join(".created"), "second result")?;
    drop(lease);
    let current = vec![SessionMessage::new(MessageRole::User, "current")];
    let restored = manager.navigate_prompt(Some(2), RevertScope::Both, &session, &current).await?;
    assert_eq!(restored.conversation, second);
    assert!(!dir.path().join(".created").exists());
    let restored = manager
        .navigate_prompt(Some(1), RevertScope::Both, &session, &restored.conversation)
        .await?;
    assert_eq!(restored.conversation, original);
    assert_eq!(fs::read(&asset)?, vec![0, 255, 7]);
    let restored = manager
        .navigate_prompt(None, RevertScope::Both, &session, &restored.conversation)
        .await?;
    assert_eq!(restored.conversation, second);
    let restored = manager
        .navigate_prompt(None, RevertScope::Both, &session, &restored.conversation)
        .await?;
    assert_eq!(restored.conversation, current);
    assert_eq!(fs::read_to_string(dir.path().join(".created"))?, "second result");
    assert!(
        manager
            .navigate_prompt(None, RevertScope::Both, &session, &current)
            .await
            .is_err()
    );
    // One jump across both prompts must also reverse later file creation.
    let restored = manager.navigate_prompt(Some(1), RevertScope::Both, &session, &current).await?;
    assert_eq!(restored.conversation, original);
    assert!(!dir.path().join(".created").exists());
    let resumed = SnapshotManager::new(SnapshotConfig::new(dir.path().into()))?;
    let restored = resumed.navigate_prompt(None, RevertScope::Both, &session, &original).await?;
    assert_eq!(restored.conversation, current);
    Ok(())
}

fn live_recovery_files(workspace: &Path) -> Vec<PathBuf> {
    let storage = workspace.join(".vtcode").join("checkpoints");
    fs::read_dir(&storage)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("turn_recovery_") && name.ends_with(".json"))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn consumed_recovery_records_are_retired_not_leaked() -> Result<()> {
    let dir = TempDir::new()?;
    let manager = SnapshotManager::new(SnapshotConfig::new(dir.path().into()))?;
    let session = uuid::Uuid::new_v4().to_string();
    let original = vec![SessionMessage::new(MessageRole::User, "original")];
    let lease = manager.begin_prompt(1, &session, "first", &original).await?;
    drop(lease);
    let second = vec![SessionMessage::new(MessageRole::User, "second")];
    let lease = manager.begin_prompt(2, &session, "second", &second).await?;
    drop(lease);
    let current = vec![SessionMessage::new(MessageRole::User, "current")];

    assert!(live_recovery_files(dir.path()).is_empty());
    let restored = manager.navigate_prompt(Some(2), RevertScope::Both, &session, &current).await?;
    assert_eq!(live_recovery_files(dir.path()).len(), 1);

    let restored = manager
        .navigate_prompt(None, RevertScope::Both, &session, &restored.conversation)
        .await?;
    assert_eq!(restored.conversation, current);
    assert!(live_recovery_files(dir.path()).is_empty(), "consumed redo must retire its recovery record");

    // A new rewind followed by a new prompt must also retire the discarded redo.
    let restored = manager.navigate_prompt(Some(2), RevertScope::Both, &session, &current).await?;
    assert_eq!(live_recovery_files(dir.path()).len(), 1);
    let _lease = manager.begin_prompt(3, &session, "third", &restored.conversation).await?;
    assert!(live_recovery_files(dir.path()).is_empty(), "begin_prompt must retire discarded redo records");
    Ok(())
}

#[tokio::test]
async fn recover_pending_fails_closed_without_touching_redo() -> Result<()> {
    let dir = TempDir::new()?;
    let manager = SnapshotManager::new(SnapshotConfig::new(dir.path().into()))?;
    let session = uuid::Uuid::new_v4().to_string();
    let original = vec![SessionMessage::new(MessageRole::User, "original")];
    let lease = manager.begin_prompt(1, &session, "first", &original).await?;
    drop(lease);
    let current = vec![SessionMessage::new(MessageRole::User, "current")];

    // No interrupted rewind: recovery must fail and must not consume redo.
    assert!(manager.recover_pending_rewind(&session).await.is_err());
    let restored = manager.navigate_prompt(Some(1), RevertScope::Both, &session, &current).await?;
    assert!(manager.recover_pending_rewind(&session).await.is_err());
    // Redo is still intact for the completed rewind.
    let redone = manager
        .navigate_prompt(None, RevertScope::Both, &session, &restored.conversation)
        .await?;
    assert_eq!(redone.conversation, current);
    Ok(())
}

#[tokio::test]
async fn complete_session_navigation_trims_active_and_clears_redo() -> Result<()> {
    let dir = TempDir::new()?;
    let manager = SnapshotManager::new(SnapshotConfig::new(dir.path().into()))?;
    let storage = dir.path().join(".vtcode").join("checkpoints");
    let session = uuid::Uuid::new_v4().to_string();
    let key: String = session.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
    let branch_path = storage.join(format!("branch_{key}.json"));
    let lock_path = storage.join("rewind.lock");
    fs::write(&lock_path, b"")?;

    // 20 protected turns plus a redo entry: the completion trim must keep
    // only the newest REWIND_ACTIVE_KEEP actives and drop redo entirely.
    let active: Vec<usize> = (1435..=1454).collect();
    let recovery = Recovery {
        policy: String::new(),
        snapshot: uuid::Uuid::new_v4().to_string(),
        active: vec![1435],
    };
    let recovery_path = recovery_path(&storage, &recovery.snapshot);
    atomic_json(&recovery_path, &serde_json::json!({}))?;
    let state = Navigation {
        active: active.clone(),
        redo: vec![recovery],
        pending: None,
    };
    atomic_json(&branch_path, &state)?;

    manager.complete_session_navigation(&session).await?;

    let saved: Navigation = serde_json::from_slice(&fs::read(&branch_path)?)?;
    assert_eq!(
        saved.active,
        active[active.len() - REWIND_ACTIVE_KEEP..].to_vec(),
        "only the newest rewind window stays pinned"
    );
    assert!(saved.redo.is_empty(), "completion must clear redo");
    assert!(saved.pending.is_none());
    assert!(!lock_path.exists(), "completion must release the workspace rewind lock");
    assert!(!recovery_path.exists(), "completion must retire redo recovery records");
    Ok(())
}
