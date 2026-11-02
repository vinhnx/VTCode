use super::*;
use tempfile::TempDir;

#[test]
fn native_agent_render_preserves_advanced_frontmatter_keys() {
    let temp = TempDir::new().expect("temp dir");
    let path = temp.path().join("reviewer.md");
    fs::write(
        &path,
        r#"---
name: reviewer
description: Review code
tools:
  - exec_command
  - apply_patch
  - code_search
model: inherit
color: blue
reasoning_effort: medium
permissions:
  default: deny
  allow:
    - code_search
  ask:
    - exec_command
background: true
maxTurns: 7
memory: project
skills:
  - rust-skills
nickname_candidates:
  - rev
---

Review the target changes."#,
    )
    .expect("write file");

    let spec = load_subagent_from_file(&path, SubagentSource::ProjectVtcode).expect("load subagent");
    let content = fs::read_to_string(&path).expect("read file");
    let mut draft = NativeAgentDraft::from_spec(temp.path().to_path_buf(), &spec, &content).expect("draft");
    draft.description = "Updated description".to_string();
    let rendered = draft.render_markdown().expect("rendered markdown");

    assert!(rendered.contains("description: Updated description"));
    assert!(rendered.contains("permissions:\n  default: deny\n"));
    assert!(rendered.contains("allow:"));
    assert!(rendered.contains("- code_search"));
    assert!(rendered.contains("ask:"));
    assert!(rendered.contains("- exec_command"));
    assert!(rendered.contains("skills:"));
    assert!(rendered.contains("nickname_candidates:"));
    assert!(rendered.ends_with("\nReview the target changes."));

    fs::write(&path, rendered).expect("write rendered file");
    let rendered_spec = load_subagent_from_file(&path, SubagentSource::ProjectVtcode).expect("reload rendered");
    assert_eq!(rendered_spec.permissions.allow, vec!["code_search".to_string()]);
    assert_eq!(rendered_spec.permissions.ask, vec!["exec_command".to_string()]);
}

#[test]
fn ordered_tools_keeps_known_tools_stable_and_sorts_custom_values() {
    let ordered = ordered_tools(vec![
        "zzz".to_string(),
        tools::CODE_SEARCH.to_string(),
        tools::EXEC_COMMAND.to_string(),
        "aaa".to_string(),
    ]);

    assert_eq!(
        ordered,
        vec![
            tools::EXEC_COMMAND.to_string(),
            tools::CODE_SEARCH.to_string(),
            "aaa".to_string(),
            "zzz".to_string(),
        ]
    );
}

#[test]
fn create_defaults_use_expected_frontmatter_and_prompt_body() {
    let workspace = TempDir::new().expect("temp dir");
    let draft =
        NativeAgentDraft::new(workspace.path().to_path_buf(), Some(AgentDefinitionScope::Project), Some("reviewer"));

    assert_eq!(draft.model, "inherit");
    assert_eq!(draft.color.as_deref(), Some("blue"));
    assert_eq!(draft.reasoning_effort.as_deref(), Some("medium"));
    assert_eq!(draft.permissions.default, PermissionDefault::Deny);
    assert!(!draft.background);
    assert_eq!(draft.tools, default_agent_tools());
    assert_eq!(draft.current_path().expect("path"), workspace.path().join(".vtcode/agents/reviewer.md"));

    let rendered = draft.render_markdown().expect("render");
    assert!(rendered.contains("name: reviewer\n"));
    assert!(rendered.contains("permissions:\n  default: deny\n"));
    assert!(rendered.contains("background: false\n"));
    assert!(rendered.contains("\nYou are a focused VT Code subagent.\n"));

    let path = workspace.path().join("reviewer.md");
    fs::write(&path, rendered).expect("write rendered agent");
    let spec = load_subagent_from_file(&path, SubagentSource::ProjectVtcode).expect("load subagent");
    assert_eq!(spec.permissions.default, PermissionDefault::Deny);
    assert!(spec.permissions.allow.is_empty());
    assert!(spec.permissions.ask.is_empty());
    assert!(spec.permissions.auto.is_empty());
    assert!(spec.permissions.deny.is_empty());
    assert_eq!(spec.mode, vtcode_config::AgentMode::Subagent);
}

#[test]
fn normalize_for_save_trims_and_defaults_fields() {
    let mut draft =
        NativeAgentDraft::new(PathBuf::from("/tmp/workspace"), Some(AgentDefinitionScope::Project), Some(" reviewer "));
    draft.description = "  Review auth changes  ".to_string();
    draft.model = "   ".to_string();
    draft.color = Some(" teal ".to_string());
    draft.reasoning_effort = Some(" medium ".to_string());
    draft.tools = vec![
        "zzz".to_string(),
        tools::EXEC_COMMAND.to_string(),
        tools::EXEC_COMMAND.to_string(),
    ];

    draft.normalize_for_save();

    assert_eq!(draft.name, "reviewer");
    assert_eq!(draft.description, "Review auth changes");
    assert_eq!(draft.model, "inherit");
    assert_eq!(draft.color.as_deref(), Some("teal"));
    assert_eq!(draft.reasoning_effort.as_deref(), Some("medium"));
    assert_eq!(draft.tools.len(), 2);
}

#[test]
fn render_markdown_keeps_prompt_body_exact_and_canonical_order() {
    let mut draft =
        NativeAgentDraft::new(PathBuf::from("/tmp/workspace"), Some(AgentDefinitionScope::Project), Some("reviewer"));
    draft.description = "Review auth changes".to_string();
    draft.tools = vec![tools::EXEC_COMMAND.to_string(), tools::CODE_SEARCH.to_string()];
    draft.color = Some("teal".to_string());
    draft.reasoning_effort = Some("high".to_string());
    draft.permissions = AgentPermissionsConfig::new(PermissionDefault::Deny);
    draft.background = true;
    draft.max_turns = Some(5);
    draft.memory = Some(SubagentMemoryScope::Project);
    draft.prompt_body = "\nPrompt body\n  with indentation\n".to_string();
    draft
        .extra_frontmatter
        .insert("skills".to_string(), YamlValue::Array(vec![YamlValue::String("rust-skills".to_string())]));

    let rendered = draft.render_markdown().expect("render");
    let model_index = rendered.find("model: inherit").expect("model key");
    let color_index = rendered.find("color: teal").expect("color key");
    let reasoning_index = rendered.find("reasoning_effort: high").expect("reasoning key");
    let permissions_index = rendered.find("permissions:").expect("permission key");
    let permissions_default_index = rendered.find("default: deny").expect("permission default");
    let background_index = rendered.find("background: true").expect("background key");
    let max_turns_index = rendered.find("maxTurns: 5").expect("maxTurns key");
    let memory_index = rendered.find("memory: project").expect("memory key");
    let skills_index = rendered.find("skills:").expect("skills key");

    assert!(model_index < color_index);
    assert!(color_index < reasoning_index);
    assert!(reasoning_index < permissions_index);
    assert!(permissions_index < permissions_default_index);
    assert!(permissions_default_index < background_index);
    assert!(background_index < max_turns_index);
    assert!(max_turns_index < memory_index);
    assert!(memory_index < skills_index);
    assert!(rendered.ends_with("\nPrompt body\n  with indentation\n"));
}

#[test]
fn render_markdown_preserves_primary_availability_and_default_decision() {
    let mut draft =
        NativeAgentDraft::new(PathBuf::from("/tmp/workspace"), Some(AgentDefinitionScope::Project), Some("planner"));
    draft.description = "Plan changes before editing".to_string();
    draft.permissions = AgentPermissionsConfig::new(PermissionDefault::Ask);
    draft
        .extra_frontmatter
        .insert("mode".to_string(), YamlValue::String("primary".to_string()));

    let rendered = draft.render_markdown().expect("render");
    assert!(rendered.contains("mode: primary\n"));
    assert!(rendered.contains("permissions:\n  default: ask\n"));

    let temp = TempDir::new().expect("temp dir");
    let path = temp.path().join("planner.md");
    fs::write(&path, rendered).expect("write rendered agent");
    let spec = load_subagent_from_file(&path, SubagentSource::ProjectVtcode).expect("load primary");
    assert_eq!(spec.permissions.default, PermissionDefault::Ask);
    assert_eq!(spec.mode, vtcode_config::AgentMode::Primary);
}

#[test]
fn is_native_vtcode_spec_filters_to_vtcode_markdown_agents() {
    let base_spec = SubagentSpec {
        name: "reviewer".to_string(),
        description: "desc".to_string(),
        prompt: String::new(),
        tools: Some(default_agent_tools()),
        disallowed_tools: Vec::new(),
        model: Some("inherit".to_string()),
        color: Some("blue".to_string()),
        reasoning_effort: Some(vtcode_config::ReasoningEffortLevel::Medium),
        permissions: AgentPermissionsConfig::new(PermissionDefault::Ask),
        skills: Vec::new(),
        mcp_servers: Vec::new(),
        hooks: None,
        background: false,
        mode: vtcode_config::AgentMode::Subagent,
        max_turns: None,
        nickname_candidates: Vec::new(),
        initial_prompt: None,
        memory: None,
        isolation: None,
        aliases: Vec::new(),
        source: SubagentSource::ProjectVtcode,
        file_path: Some(PathBuf::from(".vtcode/agents/reviewer.md")),
        warnings: Vec::new(),
        tool_policy_overrides: std::collections::BTreeMap::new(),
    };
    assert!(is_native_vtcode_spec(&base_spec));

    let imported_claude = SubagentSpec {
        source: SubagentSource::ProjectClaude,
        ..base_spec.clone()
    };
    assert!(!is_native_vtcode_spec(&imported_claude));

    let imported_codex = SubagentSpec {
        source: SubagentSource::UserCodex,
        ..base_spec.clone()
    };
    assert!(!is_native_vtcode_spec(&imported_codex));

    let missing_path = SubagentSpec { file_path: None, ..base_spec };
    assert!(!is_native_vtcode_spec(&missing_path));
}
