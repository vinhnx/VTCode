use super::*;

#[test]
fn dedup_latest_facts_extracts_user_and_tool_memory() {
    let facts = dedup_latest_facts(&message_history(), 8);
    assert_eq!(facts.len(), 2);
    assert!(facts.iter().any(|fact| fact.source == "user_assertion"));
    assert!(facts.iter().any(|fact| fact.source == "tool:code_search"));
}
#[test]
fn maybe_extract_user_fact_keeps_durable_self_facts() {
    let name = maybe_extract_user_fact(&Message::user("My name is Vinh Nguyen".to_string()))
        .expect("name should be extracted");
    assert_eq!(name.source, "user_assertion");
    assert_eq!(name.fact, "My name is Vinh Nguyen");

    let preference = maybe_extract_user_fact(&Message::user("I prefer cargo nextest for test runs".to_string()))
        .expect("preference should be extracted");
    assert_eq!(preference.source, "user_assertion");
    assert_eq!(preference.fact, "I prefer cargo nextest for test runs");
}
#[test]
fn maybe_extract_user_fact_ignores_transient_first_person_task_chatter() {
    assert!(
        maybe_extract_user_fact(&Message::user("I am debugging a failing vtcode-core test".to_string(),)).is_none()
    );
    assert!(
        maybe_extract_user_fact(&Message::user("My tests are still failing after the refactor".to_string(),)).is_none()
    );
}
#[test]
fn maybe_extract_user_fact_keeps_authored_notes_after_polite_prefixes() {
    let note =
        maybe_extract_user_fact(&Message::user("Please note that I prefer pnpm in JavaScript workspaces".to_string()))
            .expect("authored note should be extracted");
    assert_eq!(note.source, "user_assertion");
    assert_eq!(note.fact, "I prefer pnpm in JavaScript workspaces");
}
#[test]
fn maybe_extract_user_fact_ignores_memory_prompts_and_questions() {
    assert!(maybe_extract_user_fact(&Message::user("remember that I prefer cargo nextest".to_string(),)).is_none());
    assert!(maybe_extract_user_fact(&Message::user("do you remember my name?".to_string())).is_none());
    assert!(maybe_extract_user_fact(&Message::user("what do you remember about this repo?".to_string(),)).is_none());
}
#[tokio::test]
async fn finalize_persistent_memory_ignores_explicit_memory_prompts() {
    let workspace = tempdir().expect("workspace");
    std::fs::write(workspace.path().join(".git"), "gitdir: /tmp/git").expect("git marker");

    let mut runtime = runtime_config(workspace.path());
    runtime.provider = "missing-provider".to_string();

    let vt_cfg = enabled_vt_memory_config_for(workspace.path());

    let report = finalize_persistent_memory(
        &runtime,
        Some(&vt_cfg),
        &[Message::user("remember that I prefer cargo nextest".to_string())],
        "finalize-test-session",
    )
    .await
    .expect("finalize should skip ignored prompts");

    assert!(report.is_none());
}
#[tokio::test]
async fn finalize_persistent_memory_skips_existing_authored_note_duplicates() {
    let workspace = tempdir().expect("workspace");
    std::fs::write(workspace.path().join(".git"), "gitdir: /tmp/git").expect("git marker");

    let mut runtime = runtime_config(workspace.path());
    runtime.provider = "missing-provider".to_string();

    let vt_cfg = enabled_vt_memory_config_for(workspace.path());

    let memory_dir = resolve_persistent_memory_dir(&vt_cfg.agent.persistent_memory, workspace.path())
        .expect("memory dir")
        .expect("resolved dir");
    let files = PersistentMemoryFiles::new(memory_dir, false);
    std::fs::create_dir_all(&files.directory).expect("dir");
    std::fs::write(
        &files.preferences_file,
        "# Preferences\n\n- [user_assertion] I prefer cargo nextest for test runs\n",
    )
    .expect("prefs");

    let report = finalize_persistent_memory(
        &runtime,
        Some(&vt_cfg),
        &[Message::user(
            "Please note that I prefer cargo nextest for test runs".to_string(),
        )],
        "finalize-duplicate-test-session",
    )
    .await
    .expect("existing duplicate should skip routing");

    assert!(report.is_none());
}
#[tokio::test]
async fn llm_classification_rewrites_and_routes_candidates() {
    let workspace = tempdir().expect("workspace");
    let provider = StaticProvider::new(
        r#"{
          "keep": [
            {"id": 0, "topic": "preferences", "fact": "Prefer cargo nextest for test runs"},
            {"id": 1, "topic": "repository_facts", "fact": "Tests live under vtcode-core/tests"}
          ]
        }"#,
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
    assert_eq!(classified.repository_facts.len(), 1);
    assert_eq!(classified.preferences[0].fact, "Prefer cargo nextest for test runs");
    assert_eq!(classified.repository_facts[0].fact, "Tests live under vtcode-core/tests");
}
#[tokio::test]
async fn remember_planner_requests_missing_details() {
    let workspace = tempdir().expect("workspace");
    let provider = StaticProvider::new(
        r#"{
          "kind": "ask_missing",
          "facts": [],
          "selected_ids": [],
          "missing": {"field": "name", "prompt": "What name should VT Code remember?"},
          "message": null
        }"#,
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
        "save to memory and remember my name",
        None,
        None,
        &[],
    )
    .await
    .expect("plan");

    assert_eq!(plan.kind, MemoryOpKind::AskMissing);
    assert_eq!(plan.missing.as_ref().map(|missing| missing.field.as_str()), Some("name"));
    assert!(
        !provider.last_request().messages[0]
            .content
            .as_text()
            .contains("Immediately preceding assistant reply")
    );
}
#[tokio::test]
async fn remember_planner_combines_supplemental_answer_with_request() {
    let workspace = tempdir().expect("workspace");
    let provider = StaticProvider::new(
        r#"{
          "kind": "remember",
          "facts": [{"topic": "preferences", "fact": "Always run cargo nextest for Rust workspaces."}],
          "selected_ids": [],
          "missing": null,
          "message": null
        }"#,
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
        "document and make sure to remember this rule for future reference",
        Some("For missing field 'rule': Always run cargo nextest for Rust workspaces."),
        None,
        &[],
    )
    .await
    .expect("plan");

    assert_eq!(plan.kind, MemoryOpKind::Remember);
    assert_eq!(plan.facts.len(), 1);
    let prompt = provider.last_request().messages[0].content.as_text().to_string();
    assert!(prompt.contains("combine it with the user request"));
    assert!(prompt.contains("Only return ask_missing when a required value is still genuinely missing"));
    assert!(prompt.contains("For missing field 'rule': Always run cargo nextest"));
    assert!(prompt.contains("Keep it separate from the supplemental answer"));
}
#[tokio::test]
async fn remember_planner_prompt_keeps_deictic_and_supplemental_separate() {
    let workspace = tempdir().expect("workspace");
    let provider = StaticProvider::new(
        r#"{
          "kind": "remember",
          "facts": [{"topic": "preferences", "fact": "Prefer pnpm over npm for JavaScript projects."}],
          "selected_ids": [],
          "missing": null,
          "message": null
        }"#,
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
        "remember it",
        Some("For missing field 'rule': Prefer pnpm over npm."),
        Some("You should use pnpm for JavaScript workspaces."),
        &[],
    )
    .await
    .expect("plan");

    assert_eq!(plan.kind, MemoryOpKind::Remember);
    let prompt = provider.last_request().messages[0].content.as_text().to_string();
    assert!(prompt.contains("Immediately preceding assistant reply"));
    assert!(prompt.contains("You should use pnpm for JavaScript workspaces."));
    assert!(prompt.contains("For missing field 'rule': Prefer pnpm over npm."));
    assert!(prompt.contains("combine it with the user request"));
}
#[tokio::test]
async fn remember_planner_uses_immediately_preceding_assistant_reply_for_deictic_request() {
    let workspace = tempdir().expect("workspace");
    let provider = StaticProvider::new(
        r#"{
          "kind": "remember",
          "facts": [
            {
              "topic": "preferences",
              "fact": "My name is Sam Rivera; my alias is srivera.",
              "source": "assistant_reply"
            }
          ],
          "selected_ids": [],
          "missing": null,
          "message": null
        }"#,
    );
    let route = MemoryModelRoute {
        provider_name: "stub".to_string(),
        model: "stub-model".to_string(),
        temperature: 0.0,
    };
    let prior_assistant_reply = "Your name is Sam Rivera and you go by srivera.";
    let plan = plan_memory_operation_with_provider(
        &provider,
        &route,
        workspace.path(),
        MemoryOpKind::Remember,
        "remember it",
        None,
        Some(prior_assistant_reply),
        &[],
    )
    .await
    .expect("plan");

    assert_eq!(plan.kind, MemoryOpKind::Remember);
    assert_eq!(plan.facts.len(), 1);
    assert_eq!(plan.facts[0].topic, MemoryPlannedTopic::Preferences);
    assert_eq!(plan.facts[0].fact, "My name is Sam Rivera; my alias is srivera.");

    let request = provider.last_request();
    let prompt = request.messages[0].content.as_text();
    assert!(prompt.contains("Immediately preceding assistant reply"));
    assert!(prompt.contains(prior_assistant_reply));
    assert!(prompt.contains("approved by the current user"));
    // Examples are neutral templates: an identity fact with placeholders and a
    // separate non-identity preference, never a real person's name.
    assert!(prompt.contains("My name is <name>; my alias is <alias>."));
    assert!(prompt.contains("I prefer pnpm over npm for JavaScript projects."));
    assert!(!prompt.contains("vinhnx"));

    std::fs::write(workspace.path().join(".git"), "gitdir: /tmp/git").expect("git marker");
    let vt_cfg = enabled_vt_memory_config_for(workspace.path());
    let report = persist_remembered_memory_plan(&runtime_config(workspace.path()), Some(&vt_cfg), &plan)
        .await
        .expect("persist contextual plan")
        .expect("write report");
    assert_eq!(report.added_facts, 1);

    let candidates = list_persistent_memory_candidates(&vt_cfg.agent.persistent_memory, workspace.path())
        .await
        .expect("read persisted candidates")
        .expect("enabled");
    assert!(candidates.iter().any(|candidate| {
        candidate.fact == "My name is Sam Rivera; my alias is srivera." && candidate.source == "assistant_reply"
    }));
}
