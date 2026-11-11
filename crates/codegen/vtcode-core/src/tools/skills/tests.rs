use super::*;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{fs, path::Path};
use tempfile::TempDir;

const DEMO_SKILL_TOOL_NAME: &str = "demo-skill";

fn temp_codex_home(workspace: &Path) -> PathBuf {
    let codex_home = workspace.join(".test-vtcode-home");
    crate::skills::system::install_system_skills(&codex_home)
        .expect("install embedded system skills for test codex home");
    codex_home
}

#[tokio::test]
async fn load_skill_requires_explicit_approval() {
    let temp_dir = TempDir::new().expect("temp dir");
    let active_skills = Arc::new(RwLock::new(HashMap::new()));
    let registry = Arc::new(ToolRegistry::new(temp_dir.path().to_path_buf()).await);
    let runtime = SkillToolSessionRuntime::new(
        registry,
        None,
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
        None,
    );
    let tool = LoadSkillTool::new(temp_dir.path().to_path_buf(), active_skills, runtime);

    assert_eq!(tool.default_permission(), ToolPolicy::Prompt);
    assert!(tool.is_mutating());
}

#[tokio::test]
async fn skill_tool_descriptions_state_routing_without_prohibitions() {
    let temp_dir = TempDir::new().expect("temp dir");
    let active_skills = Arc::new(RwLock::new(HashMap::new()));
    let registry = Arc::new(ToolRegistry::new(temp_dir.path().to_path_buf()).await);
    let runtime = SkillToolSessionRuntime::new(
        registry,
        None,
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
        None,
    );
    let load_tool = LoadSkillTool::new(temp_dir.path().to_path_buf(), Arc::clone(&active_skills), runtime);
    let resource_tool = LoadSkillResourceTool::new(active_skills);

    let load = load_tool.description();
    assert!(load.contains("returns the cached instructions without reloading"));
    let resource = resource_tool.description();
    assert!(resource.contains("The skill must already be loaded via load_skill"));
    for description in [load, resource] {
        assert!(!description.contains("Do NOT"), "{description}");
    }
}

fn write_skill_fixture(workspace: &Path, name: &str) {
    let skill_dir = workspace.join(".agents/skills").join(name);
    let references_dir = skill_dir.join("references");
    fs::create_dir_all(&references_dir).expect("skill fixture dirs");
    fs::write(
        skill_dir.join("SKILL.md"),
        format!(
            r#"---
name: {name}
description: Demo skill
---
Use the activated helper.

See `references/notes.txt`.
"#
        ),
    )
    .expect("skill file");
    fs::write(references_dir.join("notes.txt"), "demo notes").expect("skill resource");
}

fn write_invalid_skill_fixture(workspace: &Path, name: &str) {
    let skill_dir = workspace.join(".agents/skills").join(name);
    fs::create_dir_all(&skill_dir).expect("invalid skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        format!(
            r#"---
name: {name}
description:
  - invalid
---
Broken skill
"#
        ),
    )
    .expect("invalid skill file");
}

fn write_rust_skills_metadata_fixture(workspace: &Path) {
    let skill_dir = workspace.join(".agents/skills").join("rust-skills");
    fs::create_dir_all(&skill_dir).expect("rust-skills dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        r#"---
name: rust-skills
description: Rust guidance
license: MIT
metadata:
  author: leonardomso
  version: "1.0.0"
  sources:
    - Rust API Guidelines
    - Rust Performance Book
---
Use `/rust-skills`.
"#,
    )
    .expect("rust-skills skill file");
}

#[tokio::test]
async fn traditional_skill_registration_exposes_native_cgp_factory() {
    let temp_dir = TempDir::new().expect("temp dir");
    write_skill_fixture(temp_dir.path(), DEMO_SKILL_TOOL_NAME);

    let mut loader = EnhancedSkillLoader::new(temp_dir.path().to_path_buf());
    let skill = match loader.get_skill(DEMO_SKILL_TOOL_NAME).await.expect("discover skill") {
        EnhancedSkill::Traditional(skill) => *skill,
        _ => panic!("expected traditional skill"),
    };

    let registration = build_traditional_skill_tool_registration(&skill, None);
    assert!(registration.native_cgp_factory().is_some());
}

#[tokio::test]
async fn traditional_skill_native_factory_preserves_registration_metadata() {
    let temp_dir = TempDir::new().expect("temp dir");
    write_skill_fixture(temp_dir.path(), DEMO_SKILL_TOOL_NAME);

    let mut loader = EnhancedSkillLoader::new(temp_dir.path().to_path_buf());
    let skill = match loader.get_skill(DEMO_SKILL_TOOL_NAME).await.expect("discover skill") {
        EnhancedSkill::Traditional(skill) => *skill,
        _ => panic!("expected traditional skill"),
    };

    let registration = build_traditional_skill_tool_registration(&skill, None);
    let native_factory = registration
        .native_cgp_factory()
        .expect("registration should expose native factory");
    let wrapped = native_factory(&registration, temp_dir.path().to_path_buf(), CgpRuntimeMode::Interactive);

    assert_eq!(wrapped.name(), DEMO_SKILL_TOOL_NAME);
    assert_eq!(wrapped.description(), skill.description());
    assert_eq!(wrapped.prompt_path().as_deref(), Some(SKILL_TOOL_PROMPT_PATH));
    assert_eq!(wrapped.default_permission(), ToolPolicy::Prompt);
    assert!(wrapped.parameter_schema().is_some());
}

#[tokio::test]
async fn traditional_skill_registration_schema_includes_empty_properties() {
    let temp_dir = TempDir::new().expect("temp dir");
    write_skill_fixture(temp_dir.path(), DEMO_SKILL_TOOL_NAME);

    let mut loader = EnhancedSkillLoader::new(temp_dir.path().to_path_buf());
    let skill = match loader.get_skill(DEMO_SKILL_TOOL_NAME).await.expect("discover skill") {
        EnhancedSkill::Traditional(skill) => *skill,
        _ => panic!("expected traditional skill"),
    };

    let registration = build_traditional_skill_tool_registration(&skill, None);
    let schema = registration.parameter_schema().expect("skill schema");

    assert_eq!(schema["type"].as_str(), Some("object"));
    assert_eq!(schema["properties"], json!({}));
    assert_eq!(schema["additionalProperties"], json!(true));
}

#[tokio::test]
async fn skill_activation_lifecycle_tracks_metrics() {
    let temp_dir = TempDir::new().expect("temp dir");
    write_skill_fixture(temp_dir.path(), DEMO_SKILL_TOOL_NAME);

    let mut loader = EnhancedSkillLoader::new(temp_dir.path().to_path_buf());
    let skill = match loader.get_skill(DEMO_SKILL_TOOL_NAME).await.expect("discover skill") {
        EnhancedSkill::Traditional(skill) => *skill,
        _ => panic!("expected traditional skill"),
    };

    let registry = Arc::new(ToolRegistry::new(temp_dir.path().to_path_buf()).await);
    let active_skills = Arc::new(RwLock::new(HashMap::new()));
    let runtime = SkillToolSessionRuntime::new(
        Arc::clone(&registry),
        None,
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
        None,
    );

    assert!(matches!(
        runtime.activate_skill(&active_skills, skill).await.expect("activate"),
        SkillActivationState::Activated
    ));
    let tracked = registry.metrics_collector().get_skill_metrics();
    let stats = tracked.skill_stats.get(DEMO_SKILL_TOOL_NAME).expect("created entry");
    assert_eq!(stats.language, "agent_skill");
    assert_eq!(stats.execution_count, 0);
    assert_eq!(tracked.active_skills, 1);

    assert!(
        runtime
            .deactivate_skill(&active_skills, DEMO_SKILL_TOOL_NAME)
            .await
            .expect("deactivate"),
        "deactivation settles the skill"
    );
    let after = registry.metrics_collector().get_skill_metrics();
    assert!(!after.skill_stats.contains_key(DEMO_SKILL_TOOL_NAME));
    assert_eq!(after.active_skills, 0);
}

#[tokio::test]
async fn load_skill_notifies_when_tool_snapshot_changes() {
    let temp_dir = TempDir::new().expect("temp dir");
    let skill_name = DEMO_SKILL_TOOL_NAME;
    write_skill_fixture(temp_dir.path(), skill_name);

    let active_tools = Arc::new(RwLock::new(Vec::new()));
    let change_count = Arc::new(AtomicUsize::new(0));
    let notifier_count = Arc::clone(&change_count);
    let registry = Arc::new(ToolRegistry::new(temp_dir.path().to_path_buf()).await);
    let active_skills = Arc::new(RwLock::new(HashMap::new()));
    let runtime = SkillToolSessionRuntime::new(
        Arc::clone(&registry),
        Some(Arc::clone(&active_tools)),
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
        Some(Arc::new(move |_| {
            notifier_count.fetch_add(1, Ordering::SeqCst);
        })),
    )
    .with_tool_profile(ToolProfile::AdvancedVtCode);

    let tool = LoadSkillTool::with_codex_home(
        temp_dir.path().to_path_buf(),
        Arc::clone(&active_skills),
        runtime,
        Some(temp_codex_home(temp_dir.path())),
    );

    let result = tool.execute(json!({ "name": skill_name })).await.expect("load skill succeeds");

    assert_eq!(result["activation_status"].as_str(), Some("Associated tools activated and added to context."));
    assert_eq!(change_count.load(Ordering::SeqCst), 1);
    assert!(active_skills.read().await.contains_key(skill_name));
    assert!(active_tools.read().await.iter().any(|tool| tool.function_name() == skill_name));
    assert!(
        active_tools
            .read()
            .await
            .iter()
            .any(|tool| tool.function_name() == crate::config::constants::tools::CODE_SEARCH)
    );
}

#[tokio::test]
async fn skill_refresh_retains_planning_tools_in_default_base() {
    let temp_dir = TempDir::new().expect("temp dir");
    let active_tools = Arc::new(RwLock::new(Vec::new()));
    let registry = Arc::new(ToolRegistry::new(temp_dir.path().to_path_buf()).await);
    let runtime = SkillToolSessionRuntime::new(
        registry,
        Some(Arc::clone(&active_tools)),
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
        None,
    );

    runtime.refresh_tool_snapshot("test_refresh").await;

    let active_tools = active_tools.read().await;
    assert!(
        active_tools
            .iter()
            .any(|tool| { tool.function_name() == crate::config::constants::tools::CODE_SEARCH })
    );
    assert!(
        active_tools
            .iter()
            .any(|tool| { tool.function_name() == crate::config::constants::tools::REQUEST_USER_INPUT })
    );
}

#[tokio::test]
async fn load_skill_resource_reads_from_active_skill_map() {
    let temp_dir = TempDir::new().expect("temp dir");
    let skill_name = DEMO_SKILL_TOOL_NAME;
    write_skill_fixture(temp_dir.path(), skill_name);

    let registry = Arc::new(ToolRegistry::new(temp_dir.path().to_path_buf()).await);
    let active_skills = Arc::new(RwLock::new(HashMap::new()));
    let runtime = SkillToolSessionRuntime::new(
        Arc::clone(&registry),
        None,
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
        None,
    );
    let tool = LoadSkillTool::with_codex_home(
        temp_dir.path().to_path_buf(),
        Arc::clone(&active_skills),
        runtime,
        Some(temp_codex_home(temp_dir.path())),
    );

    tool.execute(json!({ "name": skill_name })).await.expect("skill loads");

    let resource_tool = LoadSkillResourceTool::new(Arc::clone(&active_skills));
    let result = resource_tool
        .execute(json!({
            "skill_name": skill_name,
            "resource_path": "references/notes.txt"
        }))
        .await
        .expect("resource loads");

    assert_eq!(result["content"].as_str(), Some("demo notes"));
}

#[tokio::test]
async fn load_skill_resource_rejects_path_traversal() {
    let temp_dir = TempDir::new().expect("temp dir");
    let skill_name = DEMO_SKILL_TOOL_NAME;
    write_skill_fixture(temp_dir.path(), skill_name);

    let registry = Arc::new(ToolRegistry::new(temp_dir.path().to_path_buf()).await);
    let active_skills = Arc::new(RwLock::new(HashMap::new()));
    let runtime = SkillToolSessionRuntime::new(
        Arc::clone(&registry),
        None,
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
        None,
    );
    let tool = LoadSkillTool::with_codex_home(
        temp_dir.path().to_path_buf(),
        Arc::clone(&active_skills),
        runtime,
        Some(temp_codex_home(temp_dir.path())),
    );

    tool.execute(json!({ "name": skill_name })).await.expect("skill loads");

    let resource_tool = LoadSkillResourceTool::new(Arc::clone(&active_skills));
    let error = resource_tool
        .execute(json!({
            "skill_name": skill_name,
            "resource_path": "../outside.txt"
        }))
        .await
        .expect_err("path traversal should fail");

    assert!(error.to_string().contains("must be relative"));
}

#[tokio::test]
async fn load_skill_resource_fails_before_activation() {
    let active_skills = Arc::new(RwLock::new(HashMap::new()));
    let resource_tool = LoadSkillResourceTool::new(active_skills);

    let error = resource_tool
        .execute(json!({
            "skill_name": DEMO_SKILL_TOOL_NAME,
            "resource_path": "references/notes.txt"
        }))
        .await
        .expect_err("resource load should fail before activation");

    assert!(error.to_string().contains("Use `load_skill` (or `/skills load <name>`) first."));
}

#[tokio::test]
async fn deactivate_skill_unregisters_tool() {
    let temp_dir = TempDir::new().expect("temp dir");
    let skill_name = DEMO_SKILL_TOOL_NAME;
    write_skill_fixture(temp_dir.path(), skill_name);

    let registry = Arc::new(ToolRegistry::new(temp_dir.path().to_path_buf()).await);
    let active_tools = Arc::new(RwLock::new(Vec::new()));
    let active_skills = Arc::new(RwLock::new(HashMap::new()));
    let runtime = SkillToolSessionRuntime::new(
        Arc::clone(&registry),
        Some(Arc::clone(&active_tools)),
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
        None,
    );
    let mut loader = EnhancedSkillLoader::new(temp_dir.path().to_path_buf());
    let skill = match loader.get_skill(skill_name).await.expect("discover skill for activation") {
        EnhancedSkill::Traditional(skill) => *skill,
        _ => panic!("expected traditional skill"),
    };

    let activation_state = runtime.activate_skill(&active_skills, skill).await.expect("activate skill");
    assert_eq!(activation_state, SkillActivationState::Activated);
    assert!(registry.has_tool(skill_name).await);

    let removed = runtime
        .deactivate_skill(&active_skills, skill_name)
        .await
        .expect("deactivate skill");
    assert!(removed);
    assert!(!active_skills.read().await.contains_key(skill_name));
    assert!(!registry.has_tool(skill_name).await);
    assert!(active_tools.read().await.iter().all(|tool| tool.function_name() != skill_name));
}

#[tokio::test]
async fn list_skills_discovers_bundled_skill_creator_from_vtcode_home() {
    let temp_dir = TempDir::new().expect("temp dir");
    let active_skills = Arc::new(RwLock::new(HashMap::new()));
    let tool = ListSkillsTool::with_codex_home(
        temp_dir.path().to_path_buf(),
        active_skills,
        Some(temp_codex_home(temp_dir.path())),
    );

    let result = tool
        .execute(json!({ "query": "skill-creator" }))
        .await
        .expect("list skills succeeds");

    assert_eq!(result["count"].as_u64(), Some(1));
    let groups = result["groups"]["agent_skill"].as_array().expect("agent skill group");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0]["name"].as_str(), Some("skill-creator"));
}

#[tokio::test]
async fn load_skill_activates_bundled_skill_creator_from_vtcode_home() {
    let temp_dir = TempDir::new().expect("temp dir");
    let registry = Arc::new(ToolRegistry::new(temp_dir.path().to_path_buf()).await);
    let active_skills = Arc::new(RwLock::new(HashMap::new()));
    let runtime = SkillToolSessionRuntime::new(
        Arc::clone(&registry),
        None,
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
        None,
    );
    let tool = LoadSkillTool::with_codex_home(
        temp_dir.path().to_path_buf(),
        Arc::clone(&active_skills),
        runtime,
        Some(temp_codex_home(temp_dir.path())),
    );

    let result = tool
        .execute(json!({ "name": "skill-creator" }))
        .await
        .expect("load bundled skill succeeds");

    assert_eq!(result["name"].as_str(), Some("skill-creator"));
    assert_eq!(result["activation_status"].as_str(), Some("Associated tools activated and added to context."));
    assert!(active_skills.read().await.contains_key("skill-creator"));
}

#[tokio::test]
async fn list_skills_surfaces_discovery_errors() {
    let temp_dir = TempDir::new().expect("temp dir");
    write_invalid_skill_fixture(temp_dir.path(), "broken-skill");
    let active_skills = Arc::new(RwLock::new(HashMap::new()));
    let tool = ListSkillsTool::with_codex_home(
        temp_dir.path().to_path_buf(),
        active_skills,
        Some(temp_codex_home(temp_dir.path())),
    );

    let result = tool.execute(json!({})).await.expect("list skills succeeds");

    assert_eq!(result["discovery_errors"].as_u64(), Some(1));
    let samples = result["discovery_error_samples"].as_array().expect("error samples");
    assert_eq!(samples.len(), 1);
    assert!(samples[0].as_str().expect("sample string").contains("broken-skill"));
}

#[tokio::test]
async fn list_skills_accepts_rust_skills_metadata_arrays() {
    let temp_dir = TempDir::new().expect("temp dir");
    write_rust_skills_metadata_fixture(temp_dir.path());
    let active_skills = Arc::new(RwLock::new(HashMap::new()));
    let tool = ListSkillsTool::with_codex_home(
        temp_dir.path().to_path_buf(),
        active_skills,
        Some(temp_codex_home(temp_dir.path())),
    );

    let result = tool
        .execute(json!({ "query": "rust-skills" }))
        .await
        .expect("list skills succeeds");

    assert_eq!(result["count"].as_u64(), Some(1));
    let groups = result["groups"]["agent_skill"].as_array().expect("agent skill group");
    assert_eq!(groups[0]["name"].as_str(), Some("rust-skills"));
    let samples = result
        .get("discovery_error_samples")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(
        samples
            .iter()
            .all(|sample| { !sample.as_str().expect("discovery error sample").contains("rust-skills") })
    );
}

#[tokio::test]
async fn list_skills_emits_agent_skill_routing_metadata() {
    let temp_dir = TempDir::new().expect("temp dir");
    write_skill_fixture(temp_dir.path(), DEMO_SKILL_TOOL_NAME);
    let active_skills = Arc::new(RwLock::new(HashMap::new()));
    let tool = ListSkillsTool::with_codex_home(
        temp_dir.path().to_path_buf(),
        active_skills,
        Some(temp_codex_home(temp_dir.path())),
    );

    let result = tool
        .execute(json!({ "query": DEMO_SKILL_TOOL_NAME }))
        .await
        .expect("list skills succeeds");

    let groups = result["groups"]["agent_skill"].as_array().expect("agent skill group");
    assert_eq!(groups.len(), 1);
    let entry = &groups[0];
    assert!(entry["path"].as_str().expect("path string").contains(DEMO_SKILL_TOOL_NAME));
    assert_eq!(entry["scope"].as_str(), Some("repo"));
}

#[tokio::test]
async fn list_skills_query_matches_description() {
    let temp_dir = TempDir::new().expect("temp dir");
    write_skill_fixture(temp_dir.path(), DEMO_SKILL_TOOL_NAME);
    let active_skills = Arc::new(RwLock::new(HashMap::new()));
    let tool = ListSkillsTool::with_codex_home(
        temp_dir.path().to_path_buf(),
        active_skills,
        Some(temp_codex_home(temp_dir.path())),
    );

    let result = tool
        .execute(json!({ "query": "demo skill" }))
        .await
        .expect("list skills succeeds");

    assert_eq!(result["count"].as_u64(), Some(1));
    let groups = result["groups"]["agent_skill"].as_array().expect("agent skill group");
    assert_eq!(groups[0]["name"].as_str(), Some(DEMO_SKILL_TOOL_NAME));
}
