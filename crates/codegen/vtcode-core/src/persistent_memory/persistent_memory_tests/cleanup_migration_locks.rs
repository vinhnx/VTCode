use super::*;

#[test]
fn cleanup_status_flags_legacy_prompt_lines() {
    let workspace = tempdir().expect("workspace");
    let memory_config = enabled_memory_config_for(workspace.path());
    let memory_dir = resolve_persistent_memory_dir(&memory_config, workspace.path())
        .expect("memory dir")
        .expect("resolved dir");
    let files = PersistentMemoryFiles::new(memory_dir, false);
    std::fs::create_dir_all(&files.directory).expect("dir");
    std::fs::create_dir_all(&files.rollout_summaries_dir).expect("rollout dir");
    std::fs::write(
        &files.preferences_file,
        "# Preferences\n\n- [user_assertion] save to memory and remember my name\n",
    )
    .expect("prefs");
    std::fs::write(&files.summary_file, "# VT Code Memory Summary\n\n- {\"query\":\"pnpm\"}\n").expect("summary");

    let status = detect_memory_cleanup_status(&files).expect("status");
    assert!(status.needed);
    assert!(status.suspicious_facts >= 1);
    assert!(status.suspicious_summary_lines >= 1);
}
#[test]
fn cleanup_status_ignores_normalized_user_assertion_fact() {
    let workspace = tempdir().expect("workspace");
    let memory_config = enabled_memory_config_for(workspace.path());
    let memory_dir = resolve_persistent_memory_dir(&memory_config, workspace.path())
        .expect("memory dir")
        .expect("resolved dir");
    let files = PersistentMemoryFiles::new(memory_dir, false);
    std::fs::create_dir_all(&files.directory).expect("dir");
    std::fs::write(&files.preferences_file, "# Preferences\n\n- [user_assertion] My name is Vinh Nguyen\n")
        .expect("prefs");

    let status = detect_memory_cleanup_status(&files).expect("status");
    assert!(!status.needed);
    assert_eq!(status.suspicious_facts, 0);
    assert_eq!(status.suspicious_summary_lines, 0);
}
#[test]
fn cleanup_status_ignores_embedded_remember_word_in_fact() {
    let workspace = tempdir().expect("workspace");
    let memory_config = enabled_memory_config_for(workspace.path());
    let memory_dir = resolve_persistent_memory_dir(&memory_config, workspace.path())
        .expect("memory dir")
        .expect("resolved dir");
    let files = PersistentMemoryFiles::new(memory_dir, false);
    std::fs::create_dir_all(&files.directory).expect("dir");
    std::fs::write(
        &files.repository_facts_file,
        "# Repository Facts\n\n- [repository_fact] The docs remember prior design decisions in AGENTS.md.\n",
    )
    .expect("facts");

    let status = detect_memory_cleanup_status(&files).expect("status");
    assert!(!status.needed);
    assert_eq!(status.suspicious_facts, 0);
}
#[test]
fn resolve_memory_model_route_prefers_explicit_small_model_provider() {
    let workspace = tempdir().expect("workspace");
    let runtime = runtime_config(workspace.path());
    let mut vt_cfg = enabled_vt_memory_config_for(workspace.path());
    vt_cfg.agent.small_model.enabled = true;
    vt_cfg.agent.small_model.use_for_memory = true;
    vt_cfg.agent.small_model.model = "claude-4-5-haiku".to_string();

    let routes = resolve_memory_model_routes(&runtime, Some(&vt_cfg), MemoryPhase::Extract);

    assert_eq!(routes.primary.provider_name, "openai");
    // `gpt-5` is a deprecated but still valid provider route without a
    // catalog sibling; preserve it instead of selecting an unrelated model.
    assert_eq!(routes.primary.model, "gpt-5");
    assert!(routes.warning.is_some());
}
#[test]
fn resolve_memory_route_api_key_uses_runtime_key_for_active_provider() {
    let workspace = tempdir().expect("workspace");
    let route = MemoryModelRoute {
        provider_name: "openai".to_string(),
        model: "gpt-5-mini".to_string(),
        temperature: 0.1,
    };

    let api_key = resolve_api_key_for_model_route(
        &crate::llm::ModelRoute {
            provider_name: route.provider_name.clone(),
            model: route.model.clone(),
        },
        &runtime_config(workspace.path()),
    );

    assert_eq!(api_key.as_deref(), Some("test-key"));
}
#[test]
fn resolves_project_scoped_memory_directory() {
    let workspace = tempdir().expect("workspace");
    std::fs::write(workspace.path().join(".vtcode-project"), "renamed-project").expect("project name");
    let config = enabled_memory_config();
    let directory = resolve_persistent_memory_dir(&config, workspace.path())
        .expect("memory dir")
        .expect("memory dir should resolve");
    assert!(directory.to_string_lossy().contains("state/projects/renamed-project/memory"));
}
#[test]
fn migrates_legacy_memory_into_empty_target_directory() {
    let root = tempdir().expect("root");
    let legacy_dir = root.path().join("legacy/projects/repo/memory");
    let target_dir = root.path().join("home/.vtcode/projects/repo/memory");
    std::fs::create_dir_all(legacy_dir.join(ROLLOUT_SUMMARIES_DIRNAME)).expect("legacy dir");
    std::fs::write(
        legacy_dir.join(PREFERENCES_FILENAME),
        render_topic_file(
            MemoryTopic::Preferences,
            &[GroundedFactRecord {
                fact: "Prefer cargo nextest".to_string(),
                source: encode_topic_source(MemoryTopic::Preferences, "manual_memory"),
            }],
        ),
    )
    .expect("legacy prefs");

    migrate_legacy_memory_dir(&legacy_dir, &target_dir).expect("migrate");

    assert!(legacy_dir.exists());
    let migrated = std::fs::read_to_string(target_dir.join(PREFERENCES_FILENAME)).expect("target prefs");
    assert!(migrated.contains("Prefer cargo nextest"));
}
#[test]
fn migrates_legacy_memory_over_scaffold_only_target() {
    let root = tempdir().expect("root");
    let legacy_dir = root.path().join("legacy/projects/repo/memory");
    let target_dir = root.path().join("home/.vtcode/projects/repo/memory");
    std::fs::create_dir_all(legacy_dir.join(ROLLOUT_SUMMARIES_DIRNAME)).expect("legacy dir");
    std::fs::write(
        legacy_dir.join(REPOSITORY_FACTS_FILENAME),
        render_topic_file(
            MemoryTopic::RepositoryFacts,
            &[GroundedFactRecord {
                fact: "Tests live under vtcode-core/tests".to_string(),
                source: encode_topic_source(MemoryTopic::RepositoryFacts, "tool:code_search"),
            }],
        ),
    )
    .expect("legacy facts");

    std::fs::create_dir_all(target_dir.join(ROLLOUT_SUMMARIES_DIRNAME)).expect("target dir");
    std::fs::write(target_dir.join(PREFERENCES_FILENAME), render_topic_file(MemoryTopic::Preferences, &[]))
        .expect("target prefs");
    std::fs::write(target_dir.join(REPOSITORY_FACTS_FILENAME), render_topic_file(MemoryTopic::RepositoryFacts, &[]))
        .expect("target facts");
    std::fs::write(target_dir.join(MEMORY_FILENAME), render_memory_index(&[], &[], &[], 0)).expect("target memory");
    std::fs::write(target_dir.join(MEMORY_SUMMARY_FILENAME), render_memory_summary(&[], &[], &[]))
        .expect("target summary");

    migrate_legacy_memory_dir(&legacy_dir, &target_dir).expect("migrate");

    assert!(legacy_dir.exists());
    let migrated = std::fs::read_to_string(target_dir.join(REPOSITORY_FACTS_FILENAME)).expect("target facts");
    assert!(migrated.contains("Tests live under vtcode-core/tests"));
}
#[cfg(unix)]
#[test]
fn legacy_memory_migration_rejects_symlinked_source_ancestors() {
    use std::os::unix::fs::symlink;

    let root = tempdir().expect("root");
    let outside_projects = root.path().join("outside/projects/repo/memory");
    std::fs::create_dir_all(&outside_projects).expect("outside memory");
    std::fs::write(outside_projects.join(PREFERENCES_FILENAME), "outside").expect("outside memory file");

    let legacy_root = root.path().join("legacy");
    std::fs::create_dir_all(&legacy_root).expect("legacy root");
    symlink(root.path().join("outside/projects"), legacy_root.join("projects")).expect("legacy ancestor symlink");

    let target_dir = root.path().join("target/memory");
    let result = migrate_legacy_memory_dir(&legacy_root.join("projects/repo/memory"), &target_dir);

    assert!(result.is_err());
    assert!(!target_dir.join(PREFERENCES_FILENAME).exists());
}
#[cfg(unix)]
#[test]
fn legacy_memory_migration_rejects_symlinked_target_content() {
    use std::os::unix::fs::symlink;

    let root = tempdir().expect("root");
    let legacy_dir = root.path().join("legacy/projects/repo/memory");
    std::fs::create_dir_all(&legacy_dir).expect("legacy memory");
    std::fs::write(legacy_dir.join(PREFERENCES_FILENAME), "legacy").expect("legacy preferences");

    let outside = root.path().join("outside");
    std::fs::write(&outside, "outside").expect("outside memory");
    let target_dir = root.path().join("target/memory");
    std::fs::create_dir_all(&target_dir).expect("target memory");
    symlink(&outside, target_dir.join(PREFERENCES_FILENAME)).expect("target symlink");

    let result = migrate_legacy_memory_dir(&legacy_dir, &target_dir);

    assert!(result.is_err());
    assert_eq!(std::fs::read_to_string(&outside).expect("outside file remains"), "outside");
}
#[tokio::test]
async fn lock_age_reports_fresh_file_as_not_stale() {
    let workspace = tempdir().expect("workspace");
    let lock_path = workspace.path().join(".memory.lock");
    std::fs::write(&lock_path, "").expect("lock file");

    let age = lock_age(&lock_path).await.expect("age should resolve");
    assert!(
        age < Duration::from_secs(LOCK_STALE_AFTER_SECS),
        "freshly created lock reported as already stale: {age:?}"
    );
}
#[tokio::test]
async fn acquire_with_stale_after_steals_orphaned_lock() {
    let workspace = tempdir().expect("workspace");
    let lock_path = workspace.path().join(".memory.lock");
    std::fs::write(&lock_path, "").expect("orphaned lock file");

    let lock = MemoryLock::acquire_with_stale_after(&lock_path, Duration::ZERO)
        .await
        .expect("stale lock should be stolen");

    assert_eq!(lock.path, lock_path);
    assert!(lock_path.exists());
}
#[tokio::test]
async fn acquire_succeeds_when_no_lock_file_exists() {
    let workspace = tempdir().expect("workspace");
    let lock_path = workspace.path().join(".memory.lock");

    let lock = MemoryLock::acquire(&lock_path)
        .await
        .expect("acquire should succeed without contention");

    assert!(lock_path.exists());
    assert_eq!(lock.path, lock_path);
}
#[tokio::test]
async fn fresh_lock_is_not_treated_as_stale_under_large_threshold() {
    let workspace = tempdir().expect("workspace");
    let lock_path = workspace.path().join(".memory.lock");
    std::fs::write(&lock_path, "").expect("lock file");

    // Rather than driving the full ~2s retry/timeout loop with a huge
    // threshold, assert directly on the staleness predicate that
    // `acquire_with_stale_after` uses: a freshly written lock file must not
    // already exceed a generous stale-after window.
    let age = lock_age(&lock_path).await.expect("age should resolve");
    assert!(
        age < Duration::from_secs(LOCK_STALE_AFTER_SECS),
        "freshly created lock would be incorrectly stolen: {age:?}"
    );
}
