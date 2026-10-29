use super::*;

#[tokio::test]
async fn forget_planner_selects_exact_candidate_ids() {
    let workspace = tempdir().expect("workspace");
    let provider = StaticProvider::new(
        r#"{
          "kind": "forget",
          "facts": [],
          "selected_ids": [1],
          "missing": null,
          "message": "Remove the pnpm preference."
        }"#,
    );
    let route = MemoryModelRoute {
        provider_name: "stub".to_string(),
        model: "stub-model".to_string(),
        temperature: 0.0,
    };
    let candidates = vec![
        MemoryOpCandidate {
            id: 0,
            source: "manual_memory".to_string(),
            fact: "Prefer cargo nextest".to_string(),
        },
        MemoryOpCandidate {
            id: 1,
            source: "manual_memory".to_string(),
            fact: "Prefer pnpm".to_string(),
        },
    ];
    let plan = plan_memory_operation_with_provider(
        &provider,
        &route,
        workspace.path(),
        MemoryOpKind::Forget,
        "forget my pnpm preference",
        None,
        None,
        &candidates,
    )
    .await
    .expect("plan");

    assert_eq!(plan.kind, MemoryOpKind::Forget);
    assert_eq!(plan.selected_ids, vec![1]);
}
#[tokio::test]
async fn classification_falls_back_to_prompt_only_json_when_native_schema_is_unsupported() {
    let workspace = tempdir().expect("workspace");
    let provider = StaticProvider::prompt_only_json(
        "Here is the JSON:\n```json\n{\n  \"keep\": [\n    {\"id\": 0, \"topic\": \"preferences\", \"fact\": \"Prefer cargo nextest for test runs\"}\n  ]\n}\n```",
    );
    let route = MemoryModelRoute {
        provider_name: "stub".to_string(),
        model: "stub-model".to_string(),
        temperature: 0.0,
    };

    let classified =
        classify_facts_with_provider(&provider, &route, workspace.path(), &dedup_latest_facts(&message_history(), 8))
            .await
            .expect("classify");

    assert_eq!(classified.preferences.len(), 1);
    let request = provider.last_request();
    assert!(request.output_format.is_none());
    assert!(request.messages[0].content.as_text().contains("Return JSON only."));
}
#[tokio::test]
async fn planner_falls_back_to_prompt_only_json_when_native_schema_is_unsupported() {
    let workspace = tempdir().expect("workspace");
    let provider = StaticProvider::prompt_only_json(
        "```json\n{\n  \"kind\": \"ask_missing\",\n  \"facts\": [],\n  \"selected_ids\": [],\n  \"missing\": {\"field\": \"name\", \"prompt\": \"What name should VT Code remember?\"},\n  \"message\": null\n}\n```",
    );
    let route = MemoryModelRoute {
        provider_name: "stub".to_string(),
        model: "stub-model".to_string(),
        temperature: 0.0,
    };

    let plan = plan_memory_operation_with_provider(
        &provider,
        &route,
        workspace.path(),
        MemoryOpKind::Remember,
        "remember my name",
        None,
        None,
        &[],
    )
    .await
    .expect("plan");

    assert_eq!(plan.kind, MemoryOpKind::AskMissing);
    let request = provider.last_request();
    assert!(request.output_format.is_none());
    assert!(request.messages[0].content.as_text().contains("Return JSON only."));
}
#[tokio::test]
async fn planner_supports_streaming_only_provider() {
    let workspace = tempdir().expect("workspace");
    let provider = StreamingOnlyMemoryProvider {
        response: "{\"kind\":\"ask_missing\",\"facts\":[],\"selected_ids\":[],\"missing\":{\"field\":\"name\",\"prompt\":\"What name should VT Code remember?\"},\"message\":null}",
    };
    let route = MemoryModelRoute {
        provider_name: "stub".to_string(),
        model: "stub-model".to_string(),
        temperature: 0.0,
    };

    let plan = plan_memory_operation_with_provider(
        &provider,
        &route,
        workspace.path(),
        MemoryOpKind::Remember,
        "remember my name",
        None,
        None,
        &[],
    )
    .await
    .expect("streaming planner should succeed");

    assert_eq!(plan.kind, MemoryOpKind::AskMissing);
}
#[tokio::test]
async fn summary_falls_back_to_prompt_only_json_when_native_schema_is_unsupported() {
    let workspace = tempdir().expect("workspace");
    let provider = StaticProvider::prompt_only_json(
        "Summary:\n{\"bullets\":[\"Prefer cargo nextest\",\"Tests live under vtcode-core/tests\"]}",
    );
    let route = MemoryModelRoute {
        provider_name: "stub".to_string(),
        model: "stub-model".to_string(),
        temperature: 0.0,
    };

    let summary = summarize_memory_with_provider(
        &provider,
        &route,
        workspace.path(),
        &[GroundedFactRecord {
            fact: "Prefer cargo nextest".to_string(),
            source: encode_topic_source(MemoryTopic::Preferences, "manual_memory"),
        }],
        &[GroundedFactRecord {
            fact: "Tests live under vtcode-core/tests".to_string(),
            source: encode_topic_source(MemoryTopic::RepositoryFacts, "tool:code_search"),
        }],
        &[],
    )
    .await
    .expect("summary");

    assert!(summary.contains("Prefer cargo nextest"));
    let request = provider.last_request();
    assert!(request.output_format.is_none());
}
#[tokio::test]
async fn rebuild_summary_uses_summary_file_not_registry() {
    let workspace = tempdir().expect("workspace");
    std::fs::write(workspace.path().join(".git"), "gitdir: /tmp/git").expect("git marker");
    let memory_config = enabled_memory_config_for(workspace.path());
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.features.memories = true;
    vt_cfg.agent.persistent_memory = memory_config.clone();

    let memory_dir = resolve_persistent_memory_dir(&memory_config, workspace.path())
        .expect("memory dir")
        .expect("resolved dir");
    let files = PersistentMemoryFiles::new(memory_dir.clone(), false);
    let mut created_files = Vec::new();
    ensure_memory_layout(&files, &mut created_files).await.expect("layout");
    tokio::fs::write(
        &files.preferences_file,
        render_topic_file(
            MemoryTopic::Preferences,
            &[GroundedFactRecord {
                fact: "Prefer cargo nextest".to_string(),
                source: encode_topic_source(MemoryTopic::Preferences, "manual_memory"),
            }],
        ),
    )
    .await
    .expect("write prefs");

    let excerpt = read_persistent_memory_excerpt(&memory_config, workspace.path())
        .await
        .expect("excerpt")
        .expect("present");
    assert!(excerpt.contents.contains("No durable memory notes"));

    rebuild_persistent_memory_summary(&runtime_config(workspace.path()), Some(&vt_cfg))
        .await
        .expect("rebuild")
        .expect("report");

    let excerpt = read_persistent_memory_excerpt(&memory_config, workspace.path())
        .await
        .expect("excerpt")
        .expect("present");
    assert!(excerpt.contents.contains("Prefer cargo nextest"));
}
#[tokio::test]
async fn scaffold_creates_memory_layout_even_when_disabled() {
    let workspace = tempdir().expect("workspace");
    let config = PersistentMemoryConfig {
        enabled: false,
        directory_override: Some(workspace.path().join(".memory").display().to_string()),
        ..PersistentMemoryConfig::default()
    };

    let status = scaffold_persistent_memory(&config, workspace.path())
        .await
        .expect("scaffold succeeds")
        .expect("status");

    assert!(!status.enabled);
    assert!(status.summary_file.exists());
    assert!(status.memory_file.exists());
    assert!(status.preferences_file.exists());
    assert!(status.repository_facts_file.exists());
    assert!(status.notes_dir.exists());
    assert!(status.rollout_summaries_dir.exists());
}
#[tokio::test]
async fn rebuild_generated_files_include_notes_as_canonical_inputs() {
    let workspace = tempdir().expect("workspace");
    let config = PersistentMemoryConfig {
        enabled: true,
        directory_override: Some(workspace.path().join(".memory").display().to_string()),
        ..PersistentMemoryConfig::default()
    };

    scaffold_persistent_memory(&config, workspace.path())
        .await
        .expect("scaffold")
        .expect("status");
    let memory_dir = resolve_persistent_memory_dir(&config, workspace.path())
        .expect("memory dir")
        .expect("resolved dir");
    let files = PersistentMemoryFiles::new(memory_dir, false);
    tokio::fs::write(
        files.notes_dir.join("project.md"),
        "# Project Notes\n\n- Keep Anthropic memory backed by shared storage.\n",
    )
    .await
    .expect("write note");

    rebuild_generated_memory_files(&config, workspace.path())
        .await
        .expect("rebuild");

    let summary = tokio::fs::read_to_string(&files.summary_file).await.expect("summary");
    let index = tokio::fs::read_to_string(&files.memory_file).await.expect("index");

    assert!(summary.contains("Keep Anthropic memory backed by shared storage"));
    assert!(index.contains("## Note Files"), "{index}");
    assert!(index.contains("Keep Anthropic memory backed by shared storage"));
}
