use super::*;

#[tokio::test]
async fn persistent_memory_parallel_reads_preserve_source_order() {
    let workspace = tempdir().expect("workspace");
    let config = enabled_memory_config_for(workspace.path());
    scaffold_persistent_memory(&config, workspace.path())
        .await
        .expect("scaffold")
        .expect("status");
    let memory_dir = resolve_persistent_memory_dir(&config, workspace.path())
        .expect("memory dir")
        .expect("resolved dir");
    let files = PersistentMemoryFiles::new(memory_dir, false);

    tokio::fs::write(
        &files.preferences_file,
        render_topic_file(
            MemoryTopic::Preferences,
            &[GroundedFactRecord {
                fact: "Prefer cargo nextest".to_string(),
                source: encode_topic_source(MemoryTopic::Preferences, "preference.md"),
            }],
        ),
    )
    .await
    .expect("preferences");
    tokio::fs::write(
        &files.repository_facts_file,
        render_topic_file(
            MemoryTopic::RepositoryFacts,
            &[GroundedFactRecord {
                fact: "The repository uses Rust stable".to_string(),
                source: encode_topic_source(MemoryTopic::RepositoryFacts, "repository.md"),
            }],
        ),
    )
    .await
    .expect("repository facts");
    tokio::fs::write(
        files.rollout_summaries_dir.join("rollout.md"),
        render_topic_file(
            MemoryTopic::Preferences,
            &[GroundedFactRecord {
                fact: "Keep the runtime path measured".to_string(),
                source: encode_topic_source(MemoryTopic::Preferences, "rollout.md"),
            }],
        ),
    )
    .await
    .expect("rollout");
    tokio::fs::write(files.notes_dir.join("note.md"), "# Notes\n\n- Keep async reads ordered at assembly time.\n")
        .await
        .expect("note");

    let candidates = list_persistent_memory_candidates(&config, workspace.path())
        .await
        .expect("candidates")
        .expect("enabled");
    let facts = candidates.into_iter().map(|candidate| candidate.fact).collect::<Vec<_>>();

    assert_eq!(
        facts,
        vec![
            "Prefer cargo nextest",
            "The repository uses Rust stable",
            "Keep the runtime path measured",
            "Keep async reads ordered at assembly time.",
        ]
    );
}
#[tokio::test]
async fn persistent_memory_parallel_reads_preserve_error_context() {
    let workspace = tempdir().expect("workspace");
    let config = enabled_memory_config_for(workspace.path());
    scaffold_persistent_memory(&config, workspace.path())
        .await
        .expect("scaffold")
        .expect("status");
    let memory_dir = resolve_persistent_memory_dir(&config, workspace.path())
        .expect("memory dir")
        .expect("resolved dir");
    let files = PersistentMemoryFiles::new(memory_dir, false);
    tokio::fs::remove_file(&files.preferences_file)
        .await
        .expect("remove preferences");
    tokio::fs::create_dir(&files.preferences_file)
        .await
        .expect("replace preferences with directory");

    let error = list_persistent_memory_candidates(&config, workspace.path())
        .await
        .expect_err("directory should not parse as a topic file");

    assert!(error.to_string().contains("Failed to read"), "{error:#}");
    assert!(error.to_string().contains("preferences.md"), "{error:#}");
}
#[tokio::test]
async fn remember_plan_persists_normalized_manual_memory_update() {
    let workspace = tempdir().expect("workspace");
    std::fs::write(workspace.path().join(".git"), "gitdir: /tmp/git").expect("git marker");
    let vt_cfg = enabled_vt_memory_config_for(workspace.path());
    let plan = MemoryOpPlan {
        kind: MemoryOpKind::Remember,
        facts: vec![MemoryPlannedFact {
            topic: MemoryPlannedTopic::Preferences,
            fact: "Prefer pnpm for workspace package management.".to_string(),
            source: "manual_memory".to_string(),
        }],
        selected_ids: Vec::new(),
        missing: None,
        message: None,
    };

    let report = persist_remembered_memory_plan(&runtime_config(workspace.path()), Some(&vt_cfg), &plan)
        .await
        .expect("remember plan")
        .expect("report");

    assert_eq!(report.added_facts, 1);
    let excerpt = read_persistent_memory_excerpt(&vt_cfg.agent.persistent_memory, workspace.path())
        .await
        .expect("excerpt")
        .expect("present");
    assert!(excerpt.contents.contains("Prefer pnpm"));

    let candidates = list_persistent_memory_candidates(&vt_cfg.agent.persistent_memory, workspace.path())
        .await
        .expect("candidates")
        .expect("enabled");
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.fact == "Prefer pnpm for workspace package management.")
    );
}
#[tokio::test]
async fn forget_planned_matches_remove_notes_from_memory_files() {
    let workspace = tempdir().expect("workspace");
    std::fs::write(workspace.path().join(".git"), "gitdir: /tmp/git").expect("git marker");
    let runtime = runtime_config(workspace.path());
    let vt_cfg = enabled_vt_memory_config_for(workspace.path());
    let remember_plan = MemoryOpPlan {
        kind: MemoryOpKind::Remember,
        facts: vec![MemoryPlannedFact {
            topic: MemoryPlannedTopic::Preferences,
            fact: "Prefer pnpm for workspace package management.".to_string(),
            source: "manual_memory".to_string(),
        }],
        selected_ids: Vec::new(),
        missing: None,
        message: None,
    };

    persist_remembered_memory_plan(&runtime, Some(&vt_cfg), &remember_plan)
        .await
        .expect("remember plan")
        .expect("report");

    let matches = find_persistent_memory_matches(&vt_cfg.agent.persistent_memory, workspace.path(), "pnpm")
        .await
        .expect("find matches")
        .expect("enabled");
    assert!(!matches.is_empty());

    let candidates = list_persistent_memory_candidates(&vt_cfg.agent.persistent_memory, workspace.path())
        .await
        .expect("list")
        .expect("enabled")
        .into_iter()
        .enumerate()
        .map(|(index, entry)| MemoryOpCandidate { id: index, source: entry.source, fact: entry.fact })
        .collect::<Vec<_>>();
    let plan = MemoryOpPlan {
        kind: MemoryOpKind::Forget,
        facts: Vec::new(),
        selected_ids: vec![0],
        missing: None,
        message: None,
    };

    let report = forget_planned_persistent_memory_matches(&runtime, Some(&vt_cfg), &candidates, &plan)
        .await
        .expect("forget plan")
        .expect("report");
    assert!(report.removed_facts >= 1);

    let matches = find_persistent_memory_matches(&vt_cfg.agent.persistent_memory, workspace.path(), "pnpm")
        .await
        .expect("find matches")
        .expect("enabled");
    assert!(matches.is_empty());

    let excerpt = read_persistent_memory_excerpt(&vt_cfg.agent.persistent_memory, workspace.path())
        .await
        .expect("excerpt")
        .expect("present");
    assert!(!excerpt.contents.contains("Prefer pnpm"));
}
