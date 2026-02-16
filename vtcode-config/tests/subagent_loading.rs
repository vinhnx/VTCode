use std::fs;

use tempfile::TempDir;
use vtcode_config::subagent::{discover_subagents_in_dir, load_subagent_from_file};
use vtcode_config::{
    SubagentConfig, SubagentModel, SubagentParseError, SubagentPermissionMode, SubagentSource,
};

#[test]
fn load_subagent_from_file_parses_metadata_and_body() {
    let temp_dir = TempDir::new().expect("temp dir should be created");
    let agent_path = temp_dir.path().join("explorer.md");

    let markdown = r#"---
name: explorer-custom
description: Explore the repository quickly
tools: read_file, grep_file
model: inherit
permissionMode: plan
skills: rust, docs
---

You are a focused codebase explorer.
"#;
    fs::write(&agent_path, markdown).expect("subagent markdown should be written");

    let config =
        load_subagent_from_file(&agent_path, SubagentSource::Project).expect("should parse agent");

    assert_eq!(config.name, "explorer-custom");
    assert_eq!(config.description, "Explore the repository quickly");
    assert_eq!(
        config.tools,
        Some(vec!["read_file".to_string(), "grep_file".to_string()])
    );
    assert_eq!(config.model, SubagentModel::Inherit);
    assert_eq!(config.permission_mode, SubagentPermissionMode::Plan);
    assert_eq!(config.skills, vec!["rust".to_string(), "docs".to_string()]);
    assert!(config.is_read_only());
    assert_eq!(config.source, SubagentSource::Project);
    assert_eq!(config.file_path, Some(agent_path));
    assert!(
        config.system_prompt.contains("focused codebase explorer"),
        "system prompt body should be retained"
    );
}

#[test]
fn discover_subagents_in_dir_loads_markdown_and_ignores_other_files() {
    let temp_dir = TempDir::new().expect("temp dir should be created");
    let dir = temp_dir.path();

    let valid = r#"---
name: linter
description: Lint checker
---
Check lint issues.
"#;

    fs::write(dir.join("linter.md"), valid).expect("markdown should be written");
    fs::write(dir.join("notes.txt"), "not an agent").expect("non-markdown file should be written");

    let discovered = discover_subagents_in_dir(dir, SubagentSource::User);
    assert_eq!(discovered.len(), 1, "only markdown files should be scanned");

    let config = discovered[0]
        .as_ref()
        .expect("markdown agent should parse successfully");
    assert_eq!(config.name, "linter");
    assert_eq!(config.source, SubagentSource::User);
}

#[test]
fn discover_subagents_in_dir_keeps_successes_and_reports_parse_errors() {
    let temp_dir = TempDir::new().expect("temp dir should be created");
    let dir = temp_dir.path();

    let valid = r#"---
name: valid-agent
description: Valid entry
---
Body.
"#;
    fs::write(dir.join("valid.md"), valid).expect("valid markdown should be written");
    fs::write(dir.join("invalid.md"), "missing frontmatter").expect("invalid markdown should exist");

    let discovered = discover_subagents_in_dir(dir, SubagentSource::Project);
    assert_eq!(discovered.len(), 2);

    let success_count = discovered.iter().filter(|item| item.is_ok()).count();
    let error_count = discovered.iter().filter(|item| item.is_err()).count();
    assert_eq!(success_count, 1);
    assert_eq!(error_count, 1);

    let parse_error = discovered
        .iter()
        .find_map(|item| item.as_ref().err())
        .expect("one entry should fail parsing");
    assert!(
        matches!(parse_error, SubagentParseError::MissingFrontmatter),
        "invalid markdown should fail with MissingFrontmatter"
    );
}

#[test]
fn discover_subagents_in_dir_returns_empty_for_missing_directory() {
    let temp_dir = TempDir::new().expect("temp dir should be created");
    let missing = temp_dir.path().join("does-not-exist");

    let discovered = discover_subagents_in_dir(&missing, SubagentSource::User);
    assert!(
        discovered.is_empty(),
        "missing directories should produce no discovered subagents"
    );
}

#[test]
fn from_json_requires_description_field() {
    let json = serde_json::json!({
        "prompt": "no description provided"
    });

    let result = SubagentConfig::from_json("broken-agent", &json);
    assert!(result.is_err());
    assert!(matches!(
        result,
        Err(SubagentParseError::MissingField(field)) if field == "description"
    ));
}

#[test]
fn markdown_with_unknown_permission_mode_defaults_to_default_mode() {
    let markdown = r#"---
name: permissive-agent
description: Unknown mode should fallback
permissionMode: not-a-real-mode
---
Body.
"#;

    let config = SubagentConfig::from_markdown(markdown, SubagentSource::User, None)
        .expect("config should parse");
    assert_eq!(config.permission_mode, SubagentPermissionMode::Default);
}
