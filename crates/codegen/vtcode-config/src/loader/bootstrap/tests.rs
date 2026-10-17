use super::VTCodeConfig;
use crate::constants::tool_limits;
use crate::defaults::WorkspacePathsDefaults;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::tempdir;
use vtcode_commons::paths::WorkspacePaths;

struct TestWorkspacePaths {
    root: PathBuf,
}

impl WorkspacePaths for TestWorkspacePaths {
    fn workspace_root(&self) -> &Path {
        &self.root
    }

    fn config_dir(&self) -> PathBuf {
        self.root.join(".vtcode")
    }
}

#[test]
fn bootstrap_home_selection_uses_canonical_path_and_keeps_project_scaffolds_local() {
    for home_available in [true, false] {
        let workspace = tempdir().expect("workspace");
        let home = tempdir().expect("home");
        let legacy_path = home.path().join("legacy").join("custom.toml");
        let canonical_path = home.path().join("canonical").join("custom.toml");
        let paths = Arc::new(TestWorkspacePaths { root: workspace.path().to_owned() });
        let home_paths = if home_available {
            vec![legacy_path.clone(), canonical_path.clone()]
        } else {
            Vec::new()
        };
        let provider = WorkspacePathsDefaults::new(paths)
            .with_config_file_name("custom.toml")
            .with_home_paths(home_paths);
        let created = VTCodeConfig::bootstrap_project_with_provider(workspace.path(), false, true, &provider)
            .expect("bootstrap with explicit defaults");
        assert_eq!(created[0], "custom.toml");
        assert_eq!(created[1], ".vtcodegitignore");
        assert!(!legacy_path.exists());
        assert_eq!(canonical_path.exists(), home_available);
        assert_eq!(workspace.path().join("custom.toml").exists(), !home_available);
        assert_eq!(workspace.path().join(".vtcodegitignore").exists(), !home_available);
        assert!(workspace.path().join(".vtcode/README.md").exists());
        assert!(workspace.path().join("sgconfig.yml").exists());

        if home_available {
            assert!(home.path().join("canonical/.vtcodegitignore").exists());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let permissions = std::fs::metadata(home.path().join("canonical")).expect("canonical directory");
                assert_eq!(permissions.permissions().mode() & 0o777, 0o700);
            }
        }
    }
}

#[test]
fn bootstrap_preserves_all_existing_files_without_force_and_replaces_them_with_force() {
    let workspace = tempdir().expect("workspace");
    let paths = [
        "vtcode.toml",
        ".vtcodegitignore",
        ".vtcode/README.md",
        "sgconfig.yml",
        "rules/examples/no-console-log.yml",
        "rule-tests/examples/no-console-log-test.yml",
        "rule-tests/__snapshots__/no-console-log-snapshot.yml",
    ];
    for (index, relative) in paths.iter().enumerate() {
        let path = workspace.path().join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
        std::fs::write(path, format!("owner content {index}: {relative}\n")).expect("write owner content");
    }

    let preserved = VTCodeConfig::bootstrap_project(workspace.path(), false).expect("preserve files");
    assert!(preserved.is_empty());
    for (index, relative) in paths.iter().enumerate() {
        assert_eq!(
            std::fs::read_to_string(workspace.path().join(relative)).expect("read owner content"),
            format!("owner content {index}: {relative}\n")
        );
    }

    let replaced = VTCodeConfig::bootstrap_project(workspace.path(), true).expect("replace files");
    assert_eq!(replaced, paths);
    for relative in paths {
        let contents = std::fs::read_to_string(workspace.path().join(relative)).expect("read replacement");
        assert!(!contents.contains("owner content"), "{relative} was not replaced");
        assert!(!contents.is_empty(), "{relative} is empty");
    }
    let config_text = std::fs::read_to_string(workspace.path().join("vtcode.toml")).expect("read config");
    let config: VTCodeConfig = toml::from_str(&config_text).expect("parse config");
    config.validate().expect("validate config");
}

#[test]
fn sample_config_entrypoint_writes_a_valid_config_and_preserves_write_error_context() {
    let workspace = tempdir().expect("workspace");
    let path = workspace.path().join("sample.toml");
    VTCodeConfig::create_sample_config(&path).expect("create sample config");
    let contents = std::fs::read_to_string(&path).expect("read sample config");
    let config: VTCodeConfig = toml::from_str(&contents).expect("parse sample config");
    config.validate().expect("validate sample config");
    assert_eq!(config.default_primary_agent, "build");
    assert!(!config.automation.full_auto.enabled);
    assert_eq!(config.file_opener, crate::codex::FileOpener::None);

    let invalid_path = workspace.path().join("missing-parent").join("sample.toml");
    let error = VTCodeConfig::create_sample_config(&invalid_path).expect_err("missing parent should fail");
    assert_eq!(error.to_string(), format!("Failed to write config file: {}", invalid_path.display()));
    assert!(error.source().is_some(), "underlying I/O error must survive");
    assert!(!invalid_path.exists());
}

#[test]
fn sample_config_template_contains_release_loop_budgets() {
    let template = VTCodeConfig::default_vtcode_toml_template();
    let config: VTCodeConfig = toml::from_str(&template).expect("sample config template should parse");

    assert_eq!(config.agent.harness.max_tool_calls_per_turn, tool_limits::DEFAULT_MAX_TOOL_CALLS_PER_TURN);
    assert_eq!(config.tools.max_tool_loops, tool_limits::DEFAULT_MAX_TOOL_LOOPS);
    assert_eq!(config.automation.full_auto.max_turns, tool_limits::DEFAULT_FULL_AUTO_MAX_TURNS);
    assert_eq!(config.agent.max_conversation_turns, tool_limits::DEFAULT_MAX_CONVERSATION_TURNS);
}

#[test]
fn bootstrap_project_creates_vtcode_readme() {
    let workspace = tempdir().expect("workspace");
    let created = VTCodeConfig::bootstrap_project(workspace.path(), false).expect("bootstrap project should succeed");

    assert!(created.iter().any(|entry| entry == ".vtcode/README.md"), "created files: {created:?}");
    assert!(workspace.path().join(".vtcode/README.md").exists());
}

#[test]
fn bootstrap_project_creates_ast_grep_scaffold() {
    let workspace = tempdir().expect("workspace");
    let created = VTCodeConfig::bootstrap_project(workspace.path(), false).expect("bootstrap project should succeed");

    assert!(created.iter().any(|entry| entry == "sgconfig.yml"));
    assert!(created.iter().any(|entry| entry == "rules/examples/no-console-log.yml"));
    assert!(
        created
            .iter()
            .any(|entry| entry == "rule-tests/examples/no-console-log-test.yml")
    );
    assert!(
        created
            .iter()
            .any(|entry| entry == "rule-tests/__snapshots__/no-console-log-snapshot.yml")
    );

    assert!(workspace.path().join("sgconfig.yml").exists());
    assert!(workspace.path().join("rules/examples/no-console-log.yml").exists());
    assert!(workspace.path().join("rule-tests/examples/no-console-log-test.yml").exists());
    assert!(
        workspace
            .path()
            .join("rule-tests/__snapshots__/no-console-log-snapshot.yml")
            .exists()
    );
}

#[test]
fn bootstrap_project_preserves_existing_ast_grep_files_without_force() {
    let workspace = tempdir().expect("workspace");
    let sgconfig_path = workspace.path().join("sgconfig.yml");
    let rule_path = workspace.path().join("rules/examples/no-console-log.yml");

    std::fs::create_dir_all(workspace.path().join("rules/examples")).expect("create rules dir");
    std::fs::write(&sgconfig_path, "ruleDirs:\n  - custom-rules\n").expect("write sgconfig");
    std::fs::write(&rule_path, "id: custom-rule\n").expect("write rule");

    let created = VTCodeConfig::bootstrap_project(workspace.path(), false).expect("bootstrap project should succeed");

    assert!(!created.iter().any(|entry| entry == "sgconfig.yml"), "created files: {created:?}");
    assert!(
        !created.iter().any(|entry| entry == "rules/examples/no-console-log.yml"),
        "created files: {created:?}"
    );
    assert_eq!(std::fs::read_to_string(&sgconfig_path).expect("read sgconfig"), "ruleDirs:\n  - custom-rules\n");
    assert_eq!(std::fs::read_to_string(&rule_path).expect("read rule"), "id: custom-rule\n");
}
