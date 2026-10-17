use super::*;
use crate::skills::CommandSkillBackend;
use crate::skills::command_skills::command_skill_specs;
use crate::skills::system::{install_system_skills, system_cache_root_dir};
use crate::skills::types::SkillManifest;
use hashbrown::HashMap;
use serial_test::serial;
use std::fs;
use tempfile::TempDir;
use tempfile::tempdir;

fn manifest(name: &str, description: &str) -> SkillManifest {
    SkillManifest {
        name: name.to_string(),
        description: description.to_string(),
        ..Default::default()
    }
}

#[test]
fn detects_explicit_skill_mentions() {
    let skills = vec![manifest("pdf-analyzer", "Analyze PDF files and extract tables")];
    let mentions = detect_skill_mentions("Use $pdf-analyzer for this file", &skills);
    assert_eq!(mentions, vec!["pdf-analyzer".to_string()]);
}

#[test]
fn description_keywords_drive_implicit_matches() {
    let skills = vec![manifest(
        "api-fetcher",
        "Fetch data from API endpoints and summarize responses",
    )];

    let mentions = detect_skill_mentions("Fetch and summarize API responses for these endpoints", &skills);
    assert_eq!(mentions, vec!["api-fetcher".to_string()]);
}

#[test]
fn metadata_keywords_drive_implicit_matches() {
    let mut ast_grep = manifest("ast-grep", "Structural search workflows");
    ast_grep.metadata = Some(HashMap::from([
        ("keywords".to_string(), serde_json::json!(["tree-sitter parser"])),
        ("tags".to_string(), serde_json::json!(["optional chaining"])),
    ]));

    let mentions = detect_skill_mentions("Debug an optional chaining rewrite", &[ast_grep]);
    assert_eq!(mentions, vec!["ast-grep".to_string()]);
}

#[test]
fn unrelated_input_does_not_match_description() {
    let skills = vec![manifest(
        "api-fetcher",
        "Fetch data from API endpoints and summarize responses",
    )];

    let mentions = detect_skill_mentions("Please update this local markdown file and fix headings", &skills);
    assert!(mentions.is_empty());
}

#[test]
fn auto_trigger_can_be_disabled() {
    let skills = vec![manifest("sql-checker", "Validate SQL migration scripts for safety")];
    let options = SkillMentionDetectionOptions { enable_auto_trigger: false, ..Default::default() };
    let mentions = detect_skill_mentions_with_options("Use $sql-checker", &skills, &options);
    assert!(mentions.is_empty());
}

#[test]
#[serial]
fn lightweight_metadata_discovery_reuses_process_wide_cache() {
    clear_lightweight_skill_metadata_cache();

    let codex_home = tempdir().expect("codex home");
    let workspace = tempdir().expect("workspace");
    let skill_dir = workspace.path().join(".agents/skills/process-wide-cache-skill");
    fs::create_dir_all(&skill_dir).expect("create skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: process-wide-cache-skill\ndescription: process-wide cache test\n---\n# Body\n",
    )
    .expect("write skill");

    let config = SkillLoaderConfig {
        codex_home: codex_home.path().to_path_buf(),
        cwd: workspace.path().to_path_buf(),
        project_root: Some(workspace.path().to_path_buf()),
        include_bundled_system_skills: false,
    };

    let first = discover_skill_metadata_lightweight_hermetic(&config);
    assert!(
        first.skills.iter().any(|skill| skill.name == "process-wide-cache-skill"),
        "expected first discovery to find test skill",
    );

    fs::remove_dir_all(&skill_dir).expect("remove cached skill dir");

    let second = discover_skill_metadata_lightweight_hermetic(&config);
    assert!(
        second.skills.iter().any(|skill| skill.name == "process-wide-cache-skill"),
        "expected cached discovery to preserve removed skill until cache is cleared",
    );

    clear_lightweight_skill_metadata_cache();

    let third = discover_skill_metadata_lightweight_hermetic(&config);
    assert!(
        !third.skills.iter().any(|skill| skill.name == "process-wide-cache-skill"),
        "expected cleared cache to force rediscovery",
    );
}

#[test]
#[serial]
fn lightweight_discovery_honors_disable_model_invocation() {
    use crate::skills::command_skills::is_model_catalog_eligible;

    clear_lightweight_skill_metadata_cache();

    let codex_home = tempdir().expect("codex home");
    let workspace = tempdir().expect("workspace");
    let hidden_dir = workspace.path().join(".agents/skills/manual-only-skill");
    fs::create_dir_all(&hidden_dir).expect("create hidden skill dir");
    fs::write(
        hidden_dir.join("SKILL.md"),
        "---\nname: manual-only-skill\ndescription: human invoked only\ndisable-model-invocation: true\n---\n# Body\n",
    )
    .expect("write hidden skill");
    let visible_dir = workspace.path().join(".agents/skills/auto-skill");
    fs::create_dir_all(&visible_dir).expect("create visible skill dir");
    fs::write(visible_dir.join("SKILL.md"), "---\nname: auto-skill\ndescription: model invocable\n---\n# Body\n")
        .expect("write visible skill");

    let config = SkillLoaderConfig {
        codex_home: codex_home.path().to_path_buf(),
        cwd: workspace.path().to_path_buf(),
        project_root: Some(workspace.path().to_path_buf()),
        include_bundled_system_skills: false,
    };

    let outcome = discover_skill_metadata_lightweight_hermetic(&config);
    let hidden = outcome
        .skills
        .iter()
        .find(|skill| skill.name == "manual-only-skill")
        .expect("hidden skill is still discovered");
    assert!(
        !is_model_catalog_eligible(hidden),
        "disable-model-invocation skill must stay out of the model catalog",
    );
    let visible = outcome
        .skills
        .iter()
        .find(|skill| skill.name == "auto-skill")
        .expect("visible skill is discovered");
    assert!(is_model_catalog_eligible(visible));

    clear_lightweight_skill_metadata_cache();
}

#[test]
#[serial]
fn lightweight_discovery_skips_dependency_and_vcs_trees() {
    clear_lightweight_skill_metadata_cache();

    let codex_home = tempdir().expect("codex home");
    let workspace = tempdir().expect("workspace");
    let skills_root = workspace.path().join(".agents/skills");
    let write_skill = |dir: &Path, name: &str| {
        fs::create_dir_all(dir).expect("create skill dir");
        fs::write(dir.join("SKILL.md"), format!("---\nname: {name}\ndescription: test skill {name}\n---\n# Body\n"))
            .expect("write skill");
    };
    write_skill(&skills_root.join("visible-skill"), "visible-skill");
    write_skill(&skills_root.join("node_modules/hidden-skill"), "hidden-skill");
    write_skill(&skills_root.join(".git/hidden-git-skill"), "hidden-git-skill");

    let config = SkillLoaderConfig {
        codex_home: codex_home.path().to_path_buf(),
        cwd: workspace.path().to_path_buf(),
        project_root: Some(workspace.path().to_path_buf()),
        include_bundled_system_skills: false,
    };

    let outcome = discover_skill_metadata_lightweight_hermetic(&config);
    let names: Vec<&str> = outcome.skills.iter().map(|skill| skill.name.as_str()).collect();
    assert!(names.contains(&"visible-skill"), "expected visible skill, got {names:?}");
    assert!(!names.contains(&"hidden-skill"), "node_modules must not be scanned, got {names:?}");
    assert!(!names.contains(&"hidden-git-skill"), ".git must not be scanned, got {names:?}");

    clear_lightweight_skill_metadata_cache();
}

#[test]
#[serial]
fn lightweight_discovery_loads_skill_with_renamed_directory() {
    // Agent Skills client guide: directory-name mismatch warns but loads.
    clear_lightweight_skill_metadata_cache();

    let codex_home = tempdir().expect("codex home");
    let workspace = tempdir().expect("workspace");
    let skill_dir = workspace.path().join(".agents/skills/renamed-dir");
    fs::create_dir_all(&skill_dir).expect("create skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: original-name\ndescription: renamed on install\n---\n# Body\n",
    )
    .expect("write skill");

    let config = SkillLoaderConfig {
        codex_home: codex_home.path().to_path_buf(),
        cwd: workspace.path().to_path_buf(),
        project_root: Some(workspace.path().to_path_buf()),
        include_bundled_system_skills: false,
    };

    let outcome = discover_skill_metadata_lightweight_hermetic(&config);
    assert!(outcome.skills.iter().any(|skill| skill.name == "original-name"), "renamed skill must still load",);

    clear_lightweight_skill_metadata_cache();
}

#[test]
fn full_discovery_loads_skill_with_renamed_directory() {
    // Full discovery goes through parse_skill_file (not just frontmatter),
    // so this covers the warn-and-load path end to end.
    let codex_home = tempdir().expect("codex home");
    let workspace = tempdir().expect("workspace");
    let skill_dir = workspace.path().join(".agents/skills/renamed-dir");
    fs::create_dir_all(&skill_dir).expect("create skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: original-name\ndescription: renamed on install\n---\n# Body\n",
    )
    .expect("write skill");

    let config = SkillLoaderConfig {
        codex_home: codex_home.path().to_path_buf(),
        cwd: workspace.path().to_path_buf(),
        project_root: Some(workspace.path().to_path_buf()),
        include_bundled_system_skills: false,
    };

    let outcome = load_skills_hermetic(&config);
    assert!(
        outcome.skills.iter().any(|skill| skill.name == "original-name"),
        "renamed skill must still load in full mode",
    );
    assert!(
        outcome.errors.is_empty(),
        "renamed skill must not record a load error, got {:?}",
        outcome.errors.iter().map(|e| e.to_string()).collect::<Vec<_>>(),
    );
}

#[tokio::test]
async fn enhanced_loader_discovers_and_loads_built_in_command_skills() {
    let temp_dir = TempDir::new().expect("temp dir");
    let mut loader = EnhancedSkillLoader::new(temp_dir.path().to_path_buf());

    let discovery = loader.discover_all_skills().await.expect("discover skills");
    assert!(
        discovery
            .skills
            .iter()
            .any(|skill_ctx| skill_ctx.manifest().name == "cmd-status")
    );

    let skill = loader.get_skill("cmd-status").await.expect("load cmd-status");
    assert!(matches!(skill, EnhancedSkill::BuiltInCommand(_)));
}

#[tokio::test]
async fn enhanced_loader_discovers_and_loads_bundled_command_skills() {
    let workspace = TempDir::new().expect("workspace");
    let codex_home = TempDir::new().expect("codex home");
    install_system_skills(codex_home.path()).expect("install bundled system skills");
    let cmd_review_dir = system_cache_root_dir(codex_home.path()).join("cmd-review");
    assert!(
        cmd_review_dir.join("SKILL.md").exists(),
        "expected bundled cmd-review at {}",
        cmd_review_dir.display()
    );
    let (manifest, _) = crate::skills::manifest::parse_skill_file(&cmd_review_dir).expect("parse cmd-review");
    assert_eq!(manifest.name, "cmd-review");
    let config = discovery_config_for_codex_home(workspace.path(), codex_home.path());
    assert!(
        config
            .skill_paths
            .iter()
            .any(|path| path == &system_cache_root_dir(codex_home.path()))
    );
    let mut loader =
        EnhancedSkillLoader::with_codex_home(workspace.path().to_path_buf(), codex_home.path().to_path_buf());

    let discovery = loader.discover_all_skills().await.expect("discover skills");
    assert!(
        discovery
            .skills
            .iter()
            .any(|skill_ctx| skill_ctx.manifest().name == "cmd-review")
    );

    let skill = loader.get_skill("cmd-review").await.expect("load cmd-review");
    assert!(matches!(skill, EnhancedSkill::Traditional(_)));
}

#[tokio::test]
async fn enhanced_loader_does_not_open_repository_native_plugins() {
    let workspace = tempdir().expect("workspace");
    let codex_home = tempdir().expect("codex home");
    let plugin_root = workspace.path().join(".agents/plugins");
    fs::create_dir_all(&plugin_root).expect("create repository plugin root");

    let plugin_name = format!("repository-native-plugin-{}", std::process::id());
    fs::write(
        plugin_root.join("plugin.json"),
        format!(
            r#"{{
                    "name": "{plugin_name}",
                    "description": "repository-controlled native plugin",
                    "version": "1.0.0",
                    "abi_version": 1
                }}"#
        ),
    )
    .expect("write plugin metadata");
    let library_name = crate::skills::native_plugin::PluginLoader::new().library_filename(&plugin_name);
    fs::write(plugin_root.join(library_name), b"not a dynamic library").expect("write fake library");

    let mut loader =
        EnhancedSkillLoader::with_codex_home(workspace.path().to_path_buf(), codex_home.path().to_path_buf());
    let error = loader
        .get_skill(&plugin_name)
        .await
        .expect_err("repository native plugin must not be opened by generic skill lookup");

    assert!(error.to_string().contains("not found"), "unexpected error: {error}");
    assert!(!error.to_string().contains("dynamic library"));
}

#[tokio::test]
async fn enhanced_loader_discovers_every_command_skill() {
    let workspace = TempDir::new().expect("workspace");
    let codex_home = TempDir::new().expect("codex home");
    let mut loader =
        EnhancedSkillLoader::with_codex_home(workspace.path().to_path_buf(), codex_home.path().to_path_buf());

    let discovery = loader.discover_all_skills().await.expect("discover skills");
    let discovered_names = discovery
        .skills
        .iter()
        .map(|skill_ctx| skill_ctx.manifest().name.as_str())
        .collect::<std::collections::HashSet<_>>();

    for spec in command_skill_specs() {
        assert!(discovered_names.contains(spec.skill_name), "missing command skill {}", spec.skill_name);

        let skill = loader
            .get_skill(spec.skill_name)
            .await
            .unwrap_or_else(|error| panic!("failed to load {}: {error}", spec.skill_name));

        match spec.backend {
            CommandSkillBackend::TraditionalSkill { .. } => {
                assert!(
                    matches!(skill, EnhancedSkill::Traditional(_)),
                    "{} should load as a traditional skill",
                    spec.skill_name
                );
            }
            CommandSkillBackend::BuiltInCommand { .. } => {
                assert!(
                    matches!(skill, EnhancedSkill::BuiltInCommand(_)),
                    "{} should load as a built-in command skill",
                    spec.skill_name
                );
            }
        }
    }
}

#[test]
fn bundled_command_skill_prefers_system_over_workspace_shadow() {
    use crate::skills::types::SkillContext;
    let workspace = TempDir::new().expect("workspace");
    let codex_home = TempDir::new().expect("codex home");
    install_system_skills(codex_home.path()).expect("install bundled system skills");
    let system_dir = system_cache_root_dir(codex_home.path()).join("cmd-review");
    let workspace_dir = workspace.path().join(".agents/skills/cmd-review");
    // Simulate discovery order: workspace first, system last.
    let system_ctx = SkillContext::MetadataOnly(
        crate::skills::manifest::parse_skill_file(&system_dir).expect("parse system").0,
        system_dir.clone(),
    );
    let workspace_ctx = SkillContext::MetadataOnly(manifest("cmd-review", "stale workspace shadow"), workspace_dir);
    let skills = vec![workspace_ctx, system_ctx];
    let selected = select_traditional_skill_ctx(&skills, "cmd-review", workspace.path()).expect("select skill");
    assert_eq!(selected.path(), &system_dir);
    assert!(
        selected.manifest().description.contains("[instructions |"),
        "bundled cmd-review must support free-form instructions, got: {}",
        selected.manifest().description
    );
}

#[test]
fn non_command_skill_keeps_workspace_first_match() {
    use crate::skills::types::SkillContext;
    let workspace = TempDir::new().expect("workspace");
    let workspace_dir = workspace.path().join(".agents/skills/my-skill");
    let other_dir = workspace.path().join("other/my-skill");
    let skills = vec![
        SkillContext::MetadataOnly(manifest("my-skill", "workspace"), workspace_dir),
        SkillContext::MetadataOnly(manifest("my-skill", "other"), other_dir),
    ];
    let selected = select_traditional_skill_ctx(&skills, "my-skill", workspace.path()).expect("select skill");
    assert_eq!(selected.manifest().description, "workspace");
}

#[test]
fn workspace_nested_system_shape_does_not_spoof_command_skill() {
    use crate::skills::types::SkillContext;
    let workspace = TempDir::new().expect("workspace");
    let codex_home = TempDir::new().expect("codex home");
    install_system_skills(codex_home.path()).expect("install bundled system skills");
    let system_dir = system_cache_root_dir(codex_home.path()).join("cmd-review");
    let spoof_dir = workspace.path().join(".agents/skills/skills/.system/cmd-review");
    let system_ctx = SkillContext::MetadataOnly(
        crate::skills::manifest::parse_skill_file(&system_dir).expect("parse system").0,
        system_dir.clone(),
    );
    let spoof_ctx = SkillContext::MetadataOnly(manifest("cmd-review", "workspace spoof"), spoof_dir);
    let skills = vec![spoof_ctx, system_ctx];
    let selected = select_traditional_skill_ctx(&skills, "cmd-review", workspace.path()).expect("select skill");
    assert_eq!(selected.path(), &system_dir);
}

fn write_skill(dir: &Path, name: &str, description: &str) {
    fs::create_dir_all(dir).expect("create skill dir");
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n\nUse this skill.\n"),
    )
    .expect("write SKILL.md");
}

fn write_codex_skill_config(home_dir: &Path, contents: &str) {
    let config_dir = home_dir.join(".codex");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(config_dir.join("config.toml"), contents).expect("write config");
}

fn skill_loader_config_for(workspace: &Path, codex_home: &Path) -> SkillLoaderConfig {
    SkillLoaderConfig {
        codex_home: codex_home.to_path_buf(),
        cwd: workspace.to_path_buf(),
        project_root: find_git_root(workspace),
        include_bundled_system_skills: false,
    }
}

#[test]
fn disabled_skill_config_supports_stable_names() {
    let workspace = tempdir().expect("workspace");
    fs::create_dir(workspace.path().join(".git")).expect("create .git");

    let home = tempdir().expect("home");
    let codex_home = tempdir().expect("codex home");

    let old_plugin_skill_dir = workspace.path().join(".agents/plugins/example-plugin-v1/skills/release-helper");
    write_skill(&old_plugin_skill_dir, "release-helper", "Prepare release notes");

    write_codex_skill_config(
        home.path(),
        &format!(
            "[[skills.config]]\nname = \"release-helper\"\npath = \"{}\"\nenabled = false\n",
            old_plugin_skill_dir.display()
        ),
    );

    let new_plugin_skill_dir = workspace.path().join(".agents/plugins/example-plugin-v2/skills/release-helper");
    write_skill(&new_plugin_skill_dir, "release-helper", "Prepare release notes");

    fs::remove_dir_all(workspace.path().join(".agents/plugins/example-plugin-v1")).expect("remove old plugin version");

    let outcome =
        load_skills_with_home_dir(&skill_loader_config_for(workspace.path(), codex_home.path()), Some(home.path()));

    assert!(
        outcome.skills.iter().all(|skill| skill.name != "release-helper"),
        "expected release-helper to stay disabled after plugin path changed"
    );
}

#[test]
fn disabled_skill_config_preserves_path_based_entries() {
    let workspace = tempdir().expect("workspace");
    let home = tempdir().expect("home");
    let codex_home = tempdir().expect("codex home");

    let skill_dir = home.path().join(".agents/skills/path-disabled");
    write_skill(&skill_dir, "path-disabled", "Disabled by explicit path");

    write_codex_skill_config(
        home.path(),
        &format!("[[skills.config]]\npath = \"{}\"\nenabled = false\n", skill_dir.join("SKILL.md").display()),
    );

    let outcome =
        load_skills_with_home_dir(&skill_loader_config_for(workspace.path(), codex_home.path()), Some(home.path()));

    assert!(
        outcome.skills.iter().all(|skill| skill.name != "path-disabled"),
        "expected path-disabled to remain filtered by legacy path config"
    );
}

#[test]
fn discovers_agent_plugin_skills_from_user_plugin_root() {
    let workspace = tempdir().expect("workspace");
    fs::create_dir(workspace.path().join(".git")).expect("create .git");
    let home = tempdir().expect("home");
    let codex_home = tempdir().expect("codex home");

    let plugin_root = home.path().join(".agents/plugins/my-plugin");
    fs::create_dir_all(plugin_root.join("skills/migrate-agent-plugin")).expect("create skill dirs");

    fs::write(
        plugin_root.join("plugin.json"),
        r#"{
                "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
                "name": "my-plugin",
                "version": "1.0.0",
                "description": "Test plugin"
            }"#,
    )
    .expect("write plugin.json");

    fs::write(
        plugin_root.join("skills/migrate-agent-plugin/SKILL.md"),
        r#"---
name: migrate-agent-plugin
description: Migrate an existing agent plugin to Agent Plugins v1.
license: MIT
---

# Migrate an Agent Plugin

Steps here.
"#,
    )
    .expect("write skill");

    let outcome =
        load_skills_with_home_dir(&skill_loader_config_for(workspace.path(), codex_home.path()), Some(home.path()));

    assert!(
        outcome.skills.iter().any(|s| s.name == "migrate-agent-plugin"),
        "expected migrate-agent-plugin skill to be discovered from the user agent plugin root, got: {:?}",
        outcome.skills.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
}

#[test]
fn discovers_agent_plugin_skills_from_plugin_root() {
    let workspace = tempdir().expect("workspace");
    fs::create_dir(workspace.path().join(".git")).expect("create .git");
    let home = tempdir().expect("home");
    let codex_home = tempdir().expect("codex home");

    let plugin_root = workspace.path().join(".agents/plugins/my-plugin");
    fs::create_dir_all(plugin_root.join("skills/migrate-agent-plugin/references")).expect("create skill dirs");

    fs::write(
        plugin_root.join("plugin.json"),
        r#"{
                "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
                "name": "my-plugin",
                "version": "1.0.0",
                "description": "Test plugin"
            }"#,
    )
    .expect("write plugin.json");

    fs::write(
        plugin_root.join("skills/migrate-agent-plugin/SKILL.md"),
        r#"---
name: migrate-agent-plugin
description: Migrate an existing agent plugin to Agent Plugins v1.
license: MIT
---

# Migrate an Agent Plugin

Steps here.
"#,
    )
    .expect("write skill");

    let outcome =
        load_skills_with_home_dir(&skill_loader_config_for(workspace.path(), codex_home.path()), Some(home.path()));

    assert!(
        outcome.skills.iter().any(|s| s.name == "migrate-agent-plugin"),
        "expected migrate-agent-plugin skill to be discovered from agent plugin, got: {:?}",
        outcome.skills.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
}

#[test]
fn does_not_descent_into_agent_plugin_subdirs() {
    let workspace = tempdir().expect("workspace");
    fs::create_dir(workspace.path().join(".git")).expect("create .git");
    let home = tempdir().expect("home");
    let codex_home = tempdir().expect("codex home");

    let plugin_root = workspace.path().join(".agents/plugins/my-plugin");
    fs::create_dir_all(plugin_root.join("skills/nested/deep-nested")).expect("create nested dirs");

    fs::write(
        plugin_root.join("plugin.json"),
        r#"{
                "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
                "name": "my-plugin",
                "description": "Test native plugin",
                "version": "1.0.0",
                "abi_version": 1
            }"#,
    )
    .expect("write plugin.json");
    // Presence (not loadability) is what discovery checks: a complete
    // native plugin dir is consumed as one entry and never descended into.
    let lib_name = crate::skills::native_plugin::PluginLoader::new().library_filename("my-plugin");
    fs::write(plugin_root.join(&lib_name), b"").expect("write dummy plugin library");

    fs::write(
        plugin_root.join("skills/nested/deep-nested/SKILL.md"),
        r#"---
name: deep-nested
description: Should not be discovered because it is not an immediate child of skills/.
---

# Deep Nested
"#,
    )
    .expect("write deeply nested skill");

    let outcome =
        load_skills_with_home_dir(&skill_loader_config_for(workspace.path(), codex_home.path()), Some(home.path()));

    assert!(
        outcome.skills.iter().all(|s| s.name != "deep-nested"),
        "expected deeply nested skill outside skills/* to be ignored, got: {:?}",
        outcome.skills.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
}

#[test]
fn discovers_plugin_mcp_providers() {
    let workspace = tempdir().expect("workspace");
    fs::create_dir(workspace.path().join(".git")).expect("create .git");

    let plugin_root = workspace.path().join(".agents/plugins/mcp-plugin");
    fs::create_dir_all(plugin_root.join("bin")).expect("create bin dir");

    fs::write(
        plugin_root.join("plugin.json"),
        r#"{
                "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
                "name": "mcp-plugin"
            }"#,
    )
    .expect("write plugin.json");

    fs::write(
        plugin_root.join("mcp.json"),
        r#"{
                "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
                "mcpServers": {
                    "local": {
                        "type": "stdio",
                        "command": "./bin/server",
                        "args": ["--data", "${PLUGIN_DATA}/db"]
                    },
                    "remote": {
                        "type": "streamable-http",
                        "url": "https://example.com/mcp"
                    }
                }
            }"#,
    )
    .expect("write mcp.json");

    fs::write(plugin_root.join("bin/server"), "#!/bin/sh\necho hello").expect("write server script");

    let providers = crate::mcp::plugin_providers::discover_plugin_mcp_providers(workspace.path());

    let names: Vec<_> = providers.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"mcp-plugin.local"), "expected mcp-plugin.local, got: {:?}", names);
    assert!(names.contains(&"mcp-plugin.remote"), "expected mcp-plugin.remote, got: {:?}", names);
    assert_eq!(providers.len(), 2);
}

#[test]
fn dedup_keeps_first_skill_on_name_conflict() {
    use crate::skills::model::{SkillLoadOutcome, SkillMetadata, SkillScope};
    use std::path::PathBuf;

    let mut outcome = SkillLoadOutcome::default();
    outcome.skills.push(SkillMetadata {
        name: "shared-skill".to_string(),
        description: "repo copy wins".to_string(),
        short_description: None,
        path: PathBuf::from("/repo/.agents/skills/shared-skill/SKILL.md"),
        scope: SkillScope::Repo,
        manifest: None,
    });
    outcome.skills.push(SkillMetadata {
        name: "shared-skill".to_string(),
        description: "user plugin shadowed".to_string(),
        short_description: None,
        path: PathBuf::from("/home/user/.agents/plugins/some-plugin/skills/shared-skill/SKILL.md"),
        scope: SkillScope::User,
        manifest: None,
    });

    dedup_and_sort(&mut outcome);

    assert_eq!(outcome.skills.len(), 1);
    assert_eq!(outcome.skills[0].description, "repo copy wins");
}

#[test]
fn dedup_drops_same_file_discovered_twice() {
    use crate::skills::model::{SkillLoadOutcome, SkillMetadata, SkillScope};

    let temp = tempdir().expect("tempdir");
    let skill_dir = temp.path().join("dup-skill");
    write_skill(&skill_dir, "dup-skill", "Same file via overlapping roots");
    let skill_md = skill_dir.join("SKILL.md");

    let mut outcome = SkillLoadOutcome::default();
    for scope in [SkillScope::Repo, SkillScope::User] {
        outcome.skills.push(SkillMetadata {
            name: "dup-skill".to_string(),
            description: "same file".to_string(),
            short_description: None,
            path: skill_md.clone(),
            scope,
            manifest: None,
        });
    }

    dedup_and_sort(&mut outcome);

    assert_eq!(outcome.skills.len(), 1, "same canonical path must collapse to one entry");
}
