use tempfile::TempDir;

use super::*;

fn setup_manager() -> (TempDir, SnapshotManager) {
    let dir = TempDir::new().expect("tempdir");
    let workspace = dir.path().to_path_buf();
    let manager = SnapshotManager::new(SnapshotConfig::new(workspace.clone())).expect("manager");
    (dir, manager)
}

#[tokio::test]
async fn no_retirement_prune_does_not_parse_live_records_or_open_store() -> Result<()> {
    let (_dir, manager) = setup_manager();
    let path = manager.snapshot_path(1);
    fs::write(&path, b"deliberately corrupt live metadata")?;
    manager.prune_snapshot_budget().await?;
    assert_eq!(fs::read(&path)?, b"deliberately corrupt live metadata");
    let entries: Vec<_> = fs::read_dir(&manager.storage_dir)?.collect::<std::io::Result<Vec<_>>>()?;
    assert_eq!(entries.len(), 1, "hot cleanup must not create an engine store");
    assert_eq!(entries[0].path(), path);
    Ok(())
}

#[tokio::test]
async fn retired_cleanup_preserves_shared_references_and_defers_on_corrupt_live_metadata() -> Result<()> {
    let (_dir, manager) = setup_manager();
    let file = manager.workspace.join("shared.txt");
    fs::write(&file, b"original")?;
    manager
        .create_snapshot(1, "first", &[], &BTreeSet::from([file.clone()]), None, None, None)
        .await?;
    let stored = manager.load_snapshot(1).await?.expect("snapshot");
    let engine = stored.files[0].data.clone().expect("engine reference");
    fs::write(manager.snapshot_path(2), serde_json::to_vec(&stored)?)?;
    let retired = manager.snapshot_path(1).with_extension("retired-test");
    fs::rename(manager.snapshot_path(1), &retired)?;
    let corrupt = manager.snapshot_path(3);
    fs::write(&corrupt, b"{")?;
    manager.cleanup_retired_snapshots(false).await;
    assert!(retired.exists(), "corrupt live records must defer reclamation");
    let store = filesnap::WorkspaceStore::open(&manager.storage_dir, &manager.workspace)?;
    assert!(store.target_for_turn(&engine)?.is_some());
    fs::remove_file(corrupt)?;
    manager.cleanup_retired_snapshots(false).await;
    assert!(!retired.exists());
    assert!(store.target_for_turn(&engine)?.is_some(), "a live shared reference pins the engine session");
    fs::write(&file, b"external edit")?;
    manager.restore_snapshot(2, RevertScope::Code).await?;
    assert_eq!(fs::read(file)?, b"original");
    Ok(())
}

#[tokio::test]
async fn full_maintenance_collects_orphans_without_retired_records() -> Result<()> {
    let (_dir, manager) = setup_manager();
    // Use the versioned engine store, rather than a standalone BlobStore
    // that the workspace collector cannot discover.
    let blob_dir = manager
        .storage_dir
        .join("filesnap")
        .join(format!("v{}", filesnap::FORMAT_VERSION))
        .join("blobs");
    let blobs = filesnap::BlobStore::open(&blob_dir)?;
    let hash = blobs.store_bytes(b"orphaned content")?;
    let blob = blob_dir.join(&hash[..2]).join(&hash[2..]);
    fs::File::options()
        .write(true)
        .open(&blob)?
        .set_times(fs::FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(3600)))?;
    manager.prune_snapshot_budget().await?;
    assert!(blob.exists(), "hot pruning skips unrelated orphan maintenance");
    manager.cleanup_old_snapshots().await?;
    assert!(!blob.exists(), "explicit maintenance must still collect old orphans");
    assert!(blobs.hashes()?.is_empty());
    Ok(())
}

#[tokio::test]
async fn create_and_list_snapshots() -> Result<()> {
    let (_dir, manager) = setup_manager();
    let mut conversation = Vec::new();
    conversation.push(SessionMessage::new(crate::llm::provider::MessageRole::User, "Hello"));
    let files = BTreeSet::new();
    manager
        .create_snapshot(1, "First turn", &conversation, &files, None, None, None)
        .await?
        .expect("metadata");
    conversation.push(SessionMessage::new(crate::llm::provider::MessageRole::Assistant, "Hi"));
    manager
        .create_snapshot(2, "Second turn", &conversation, &files, None, None, None)
        .await?
        .expect("metadata");

    let snapshots = manager.list_snapshots().await?;
    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[0].turn_number, 2);
    assert_eq!(snapshots[1].turn_number, 1);
    Ok(())
}

#[tokio::test]
async fn snapshot_restores_file_contents() -> Result<()> {
    let (dir, manager) = setup_manager();
    let workspace = dir.path();
    let file_path = workspace.join("example.txt");
    fs::write(&file_path, "v1")?;

    let mut files = BTreeSet::new();
    files.insert(PathBuf::from("example.txt"));
    let conversation = vec![SessionMessage::new(
        crate::llm::provider::MessageRole::User,
        "edit example",
    )];
    manager
        .create_snapshot(1, "save", &conversation, &files, None, None, None)
        .await?
        .expect("metadata");

    fs::write(&file_path, "v2")?;
    manager.restore_snapshot(1, RevertScope::Code).await?.expect("restore");
    let restored = fs::read_to_string(&file_path)?;
    assert_eq!(restored, "v1");
    Ok(())
}

#[tokio::test]
async fn snapshot_handles_deleted_files() -> Result<()> {
    let (dir, manager) = setup_manager();
    let workspace = dir.path();
    let file_path = workspace.join("remove.txt");
    fs::write(&file_path, "data")?;

    let mut files = BTreeSet::new();
    files.insert(PathBuf::from("remove.txt"));
    let conversation = vec![SessionMessage::new(crate::llm::provider::MessageRole::User, "remove")];
    manager
        .create_snapshot(1, "save", &conversation, &files, None, None, None)
        .await?
        .expect("metadata");

    fs::remove_file(&file_path)?;
    manager.restore_snapshot(1, RevertScope::Code).await?.expect("restore");
    assert!(file_path.exists());
    let content = fs::read_to_string(&file_path)?;
    assert_eq!(content, "data");
    Ok(())
}

#[tokio::test]
async fn cleanup_respects_limit() -> Result<()> {
    let (_dir, manager) = setup_manager();
    let conversation = vec![SessionMessage::new(crate::llm::provider::MessageRole::User, "hi")];
    let files = BTreeSet::new();

    for turn in 1..=5 {
        manager
            .create_snapshot(turn, "turn", &conversation, &files, None, None, None)
            .await?
            .expect("metadata");
    }

    // Manager default limit is 50, shrink artificially for test
    let mut config = SnapshotConfig::new(manager.workspace.clone());
    config.max_snapshots = 3;
    let trimmed = SnapshotManager::new(config)?;
    trimmed.cleanup_old_snapshots().await?;
    let listed = trimmed.list_snapshots().await?;
    assert_eq!(listed.len(), 3);
    assert_eq!(listed[0].turn_number, 5);
    assert_eq!(listed[2].turn_number, 3);
    Ok(())
}

#[tokio::test]
async fn cleanup_preserves_navigation_referenced_turns() -> Result<()> {
    let (_dir, manager) = setup_manager();
    let conversation = vec![SessionMessage::new(crate::llm::provider::MessageRole::User, "nav")];
    let files = BTreeSet::new();

    for turn in 1..=5 {
        manager
            .create_snapshot(turn, "turn", &conversation, &files, None, None, None)
            .await?
            .expect("metadata");
    }

    // Simulate a live session navigation that still needs turns 1 and 2
    // for rewind/redo. Unreferenced turns 3-5 are fair game.
    let branch = serde_json::json!({
        "active": [1, 2],
        "redo": [{ "policy": "", "snapshot": "00000000-0000-0000-0000-000000000000", "active": [1] }],
        "pending": null,
    });
    fs::write(manager.storage_dir.join("branch_74657374.json"), serde_json::to_vec(&branch)?)?;

    let mut config = SnapshotConfig::new(manager.workspace.clone());
    config.max_snapshots = 1;
    let trimmed = SnapshotManager::new(config)?;
    trimmed.cleanup_old_snapshots().await?;

    assert!(trimmed.load_snapshot(1).await?.is_some(), "active turn must survive");
    assert!(trimmed.load_snapshot(2).await?.is_some(), "active turn must survive");
    // Unreferenced turns 3-5 compete for a single budget slot; the oldest
    // two are retired and the newest (5) remains.
    assert!(trimmed.load_snapshot(3).await?.is_none(), "unreferenced turn must be pruned");
    assert!(trimmed.load_snapshot(4).await?.is_none(), "unreferenced turn must be pruned");
    assert!(trimmed.load_snapshot(5).await?.is_some(), "newest unreferenced turn fits the budget");
    Ok(())
}

#[tokio::test]
async fn snapshot_normalizes_absolute_paths() -> Result<()> {
    let (dir, manager) = setup_manager();
    let workspace = dir.path();
    let absolute = workspace.join("abs.txt");
    fs::write(&absolute, "contents")?;

    let mut files = BTreeSet::new();
    files.insert(absolute.clone());
    let conversation = vec![SessionMessage::new(crate::llm::provider::MessageRole::User, "absolute")];

    manager
        .create_snapshot(1, "abs", &conversation, &files, None, None, None)
        .await?
        .expect("metadata");

    let stored = manager.load_snapshot(1).await?.expect("stored snapshot");
    assert_eq!(stored.files.len(), 1);
    assert_eq!(stored.files[0].path, "abs.txt");
    assert!(!stored.files[0].deleted);
    Ok(())
}

#[tokio::test]
async fn cleanup_removes_expired_snapshots() -> Result<()> {
    let (_dir, manager) = setup_manager();
    let conversation = vec![SessionMessage::new(crate::llm::provider::MessageRole::User, "cleanup")];
    let files = BTreeSet::new();

    manager
        .create_snapshot(1, "old", &conversation, &files, None, None, None)
        .await?
        .expect("metadata");

    let snapshot_path = manager.snapshot_path(1);
    let mut stored: StoredSnapshot = serde_json::from_slice(&fs::read(&snapshot_path)?)?;
    stored.metadata.created_at = 1;
    let updated = serde_json::to_vec_pretty(&stored)?;
    fs::write(&snapshot_path, updated)?;

    let mut config = SnapshotConfig::new(manager.workspace.clone());
    config.max_age_days = Some(1);
    let janitor = SnapshotManager::new(config)?;
    janitor.cleanup_old_snapshots().await?;

    assert!(janitor.load_snapshot(1).await?.is_none());
    Ok(())
}

#[tokio::test]
async fn snapshot_persists_prompt_metadata() -> Result<()> {
    let (_dir, manager) = setup_manager();
    let conversation = vec![
        SessionMessage::new(crate::llm::provider::MessageRole::User, "Explain checkpointing"),
        SessionMessage::new(crate::llm::provider::MessageRole::Assistant, "Working on it"),
    ];

    manager
        .create_snapshot(
            1,
            "assistant reply",
            &conversation,
            &BTreeSet::new(),
            Some("Explain checkpointing"),
            Some(0),
            None,
        )
        .await?
        .expect("metadata");

    let stored = manager.load_snapshot(1).await?.expect("stored snapshot");
    assert_eq!(stored.metadata.prompt_text.as_deref(), Some("Explain checkpointing"));
    assert_eq!(stored.metadata.prompt_message_index, Some(0));
    assert_eq!(stored.metadata.description, "Explain checkpointing");
    Ok(())
}

#[tokio::test]
async fn load_snapshot_hydrates_prompt_metadata_for_legacy_files() -> Result<()> {
    let (_dir, manager) = setup_manager();
    let stored = StoredSnapshot {
        metadata: SnapshotMetadata {
            id: "turn_1".to_string(),
            turn_number: 1,
            created_at: 1,
            description: "legacy".to_string(),
            message_count: 2,
            file_count: 0,
            touched_files: Vec::new(),
            prompt_text: None,
            prompt_message_index: None,
            session_id: None,
            runtime_turn_id: None,
            session_turn_number: None,
            turn_diagnostics: None,
        },
        conversation: vec![
            SessionMessage::new(crate::llm::provider::MessageRole::User, "Legacy prompt"),
            SessionMessage::new(crate::llm::provider::MessageRole::Assistant, "Legacy reply"),
        ],
        files: Vec::new(),
        schema_version: None,
    };
    let path = manager.snapshot_path(1);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(&stored)?)?;

    let loaded = manager.load_snapshot(1).await?.expect("loaded snapshot");
    assert_eq!(loaded.metadata.prompt_text.as_deref(), Some("Legacy prompt"));
    assert_eq!(loaded.metadata.prompt_message_index, Some(0));
    Ok(())
}

#[test]
fn legacy_snapshot_versions_migrate_without_inventing_diagnostics() -> Result<()> {
    let legacy = StoredSnapshot {
        metadata: SnapshotMetadata {
            id: "turn_1".to_string(),
            turn_number: 1,
            created_at: 1,
            description: "legacy".to_string(),
            message_count: 0,
            file_count: 0,
            touched_files: Vec::new(),
            prompt_text: None,
            prompt_message_index: None,
            session_id: None,
            runtime_turn_id: None,
            session_turn_number: None,
            turn_diagnostics: None,
        },
        conversation: Vec::new(),
        files: Vec::new(),
        schema_version: None,
    };

    let migrated_v0 = legacy.clone().migrate(SNAPSHOT_SCHEMA_VERSION)?;
    assert_eq!(migrated_v0.schema_version, Some(SNAPSHOT_SCHEMA_VERSION));
    assert!(migrated_v0.metadata.turn_diagnostics.is_none());

    let migrated_v1 =
        StoredSnapshot { schema_version: Some(SchemaVersion::V1), ..legacy }.migrate(SNAPSHOT_SCHEMA_VERSION)?;
    assert_eq!(migrated_v1.schema_version, Some(SNAPSHOT_SCHEMA_VERSION));
    assert!(migrated_v1.metadata.turn_diagnostics.is_none());
    Ok(())
}

#[tokio::test]
async fn v2_snapshot_round_trips_session_linkage_and_canonical_usage() -> Result<()> {
    let (_dir, manager) = setup_manager();
    let usage = Usage {
        input_tokens: 43_282,
        output_tokens: 911,
        cached_input_tokens: 8_000,
        cache_creation_tokens: 512,
    };
    let diagnostics = SnapshotTurnDiagnostics {
        usage: usage.clone(),
        elapsed_ms: 12_345,
        requested_tool_calls: 9,
        admitted_tool_calls: 7,
        unadmitted_tool_calls: 2,
        failed_tool_calls: 2,
        denied_tool_calls: 1,
        preflight_failures: 1,
        reused_results: 2,
        spooled_results: 3,
        raw_spooled_bytes: 139_000,
        model_visible_output_bytes: 6_100,
        suppressed_tool_previews: 5,
        model_visible_tool_preview_budget_exhausted: true,
        low_signal_tool_calls: 4,
        recovery_activations: 1,
        in_progress_exec_sessions: vec![CompactStr::from("run-42")],
    };
    let context = SnapshotTurnContext {
        session_id: Some(CompactStr::from("session-911")),
        runtime_turn_id: Some(CompactStr::from("turn-runtime-911")),
        session_turn_number: Some(911),
        turn_diagnostics: Some(diagnostics.clone()),
        touched_files: Vec::new(),
    };

    manager
        .create_snapshot(1, "diagnostic checkpoint", &[], &BTreeSet::new(), None, None, Some(context))
        .await?
        .expect("metadata");

    let stored = manager.load_snapshot(1).await?.expect("stored snapshot");
    assert_eq!(stored.schema_version, Some(SNAPSHOT_SCHEMA_VERSION));
    assert_eq!(stored.metadata.session_id.as_deref(), Some("session-911"));
    assert_eq!(stored.metadata.runtime_turn_id.as_deref(), Some("turn-runtime-911"));
    assert_eq!(stored.metadata.session_turn_number, Some(911));
    assert_eq!(stored.metadata.turn_diagnostics, Some(diagnostics));
    assert_eq!(stored.metadata.turn_diagnostics.expect("diagnostics").usage, usage);
    Ok(())
}

#[test]
fn parse_revert_scope_variants() {
    assert_eq!(SnapshotManager::parse_revert_scope("conversation"), Some(RevertScope::Conversation));
    assert_eq!(SnapshotManager::parse_revert_scope("code"), Some(RevertScope::Code));
    assert_eq!(SnapshotManager::parse_revert_scope("full"), Some(RevertScope::Both));
    assert_eq!(SnapshotManager::parse_revert_scope("unknown"), None);
}
#[tokio::test]
async fn binary_checkpoint_reuses_content_and_does_not_delete_untracked_neighbors() -> Result<()> {
    let (_dir, manager) = setup_manager();
    let file = manager.workspace.join("asset.bin");
    fs::write(&file, [0, 255, 4])?;
    let files = BTreeSet::from([PathBuf::from("asset.bin"), PathBuf::from("created.bin")]);
    manager.create_snapshot(1, "binary", &[], &files, None, None, None).await?;
    manager.create_snapshot(2, "same", &[], &files, None, None, None).await?;
    let store = filesnap::WorkspaceStore::open(&manager.storage_dir, &manager.workspace)?;
    let first = manager.load_snapshot(1).await?.expect("first");
    let second = manager.load_snapshot(2).await?.expect("second");
    let first_target = store
        .target_for_turn(first.files[0].data.as_deref().expect("ref"))?
        .expect("target");
    let second_target = store
        .target_for_turn(second.files[0].data.as_deref().expect("ref"))?
        .expect("target");
    let first_manifest = store.manifest(first_target.manifest_id())?;
    let second_manifest = store.manifest(second_target.manifest_id())?;
    let first_hash = &first_manifest.entries.values().next().expect("file").hash;
    let second_hash = &second_manifest.entries.values().next().expect("file").hash;
    assert_eq!(first_hash, second_hash);
    fs::write(&file, b"changed")?;
    fs::write(manager.workspace.join("created.bin"), b"created")?;
    fs::write(manager.workspace.join("neighbor"), b"keep")?;
    manager.restore_snapshot(1, RevertScope::Code).await?;
    assert_eq!(fs::read(file)?, [0, 255, 4]);
    assert!(!manager.workspace.join("created.bin").exists());
    assert_eq!(fs::read(manager.workspace.join("neighbor"))?, b"keep");
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn refuses_symlink_escape_before_restoring_any_file() -> Result<()> {
    let (dir, manager) = setup_manager();
    fs::write(manager.workspace.join("a.txt"), "before")?;
    fs::create_dir(manager.workspace.join("nested"))?;
    fs::write(manager.workspace.join("nested/b.txt"), "before")?;
    let files = BTreeSet::from([PathBuf::from("a.txt"), PathBuf::from("nested/b.txt")]);
    manager.create_snapshot(1, "paths", &[], &files, None, None, None).await?;
    fs::write(manager.workspace.join("a.txt"), "after")?;
    fs::remove_dir_all(manager.workspace.join("nested"))?;
    let outside = dir.path().join("outside");
    fs::create_dir(&outside)?;
    fs::write(outside.join("b.txt"), "outside")?;
    std::os::unix::fs::symlink(&outside, manager.workspace.join("nested"))?;
    assert!(manager.restore_snapshot(1, RevertScope::Code).await.is_err());
    assert_eq!(fs::read_to_string(manager.workspace.join("a.txt"))?, "after");
    assert_eq!(fs::read_to_string(outside.join("b.txt"))?, "outside");
    Ok(())
}

#[tokio::test]
async fn legacy_inline_contents_still_restore() -> Result<()> {
    let (_dir, manager) = setup_manager();
    fs::write(manager.workspace.join("old.txt"), "original")?;
    let files = BTreeSet::from([PathBuf::from("old.txt")]);
    manager.create_snapshot(1, "legacy", &[], &files, None, None, None).await?;
    let mut stored = manager.load_snapshot(1).await?.expect("snapshot");
    stored.schema_version = Some(SchemaVersion::V2);
    stored.files[0].encoding = Some(FileEncoding::Utf8);
    stored.files[0].data = Some("legacy".into());
    fs::write(manager.snapshot_path(1), serde_json::to_vec(&stored)?)?;
    manager.restore_snapshot(1, RevertScope::Code).await?;
    assert_eq!(fs::read_to_string(manager.workspace.join("old.txt"))?, "legacy");
    Ok(())
}

#[tokio::test]
async fn retention_releases_engine_sessions_but_preserves_live_content() -> Result<()> {
    let (_dir, manager) = setup_manager();
    let path = manager.workspace.join("retained.txt");
    fs::write(&path, "shared")?;
    let files = BTreeSet::from([PathBuf::from("retained.txt")]);
    manager.create_snapshot(1, "first", &[], &files, None, None, None).await?;
    let first = manager.load_snapshot(1).await?.expect("first").files[0]
        .data
        .clone()
        .expect("reference");
    manager.create_snapshot(2, "second", &[], &files, None, None, None).await?;
    let second = manager.load_snapshot(2).await?.expect("second").files[0]
        .data
        .clone()
        .expect("reference");
    let mut config = SnapshotConfig::new(manager.workspace.clone());
    config.max_snapshots = 1;
    let janitor = SnapshotManager::new(config)?;
    janitor.cleanup_old_snapshots().await?;
    let store = filesnap::WorkspaceStore::open(&manager.storage_dir, &manager.workspace)?;
    assert_eq!(store.sessions()?, vec![second.clone()]);
    assert!(store.target_for_turn(&first)?.is_none());
    fs::write(&path, "changed")?;
    janitor.restore_snapshot(2, RevertScope::Code).await?;
    assert_eq!(fs::read_to_string(&path)?, "shared");
    janitor.create_snapshot(2, "replacement", &[], &files, None, None, None).await?;
    assert!(store.target_for_turn(&second)?.is_none());
    assert_eq!(store.sessions()?.len(), 1);
    Ok(())
}
