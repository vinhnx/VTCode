use super::*;

use crate::defaults::{self, SyntaxHighlightingDefaults, WorkspacePathsDefaults};
use crate::loader::layers::ConfigLayerSource;
use serial_test::serial;
use std::fs;
use std::io::Write;
use std::sync::Arc;
use tempfile::NamedTempFile;
use vtcode_commons::reference::StaticWorkspacePaths;

#[test]
#[serial]
fn test_layered_config_loading() {
    let workspace = assert_fs::TempDir::new().expect("failed to create workspace");
    let workspace_root = workspace.path();

    // 1. User config
    let home_dir = workspace_root.join("home");
    fs::create_dir_all(&home_dir).expect("failed to create home dir");
    let user_config_path = home_dir.join("vtcode.toml");
    fs::write(&user_config_path, "agent.provider = \"anthropic\"")
        .expect("failed to write user config");

    // 2. Workspace config
    let workspace_config_path = workspace_root.join("vtcode.toml");
    fs::write(
        &workspace_config_path,
        "agent.default_model = \"claude-haiku-4-5\"",
    )
    .expect("failed to write workspace config");

    let static_paths = StaticWorkspacePaths::new(workspace_root, workspace_root.join(".vtcode"));
    let provider = WorkspacePathsDefaults::new(Arc::new(static_paths))
        .with_home_paths(vec![user_config_path.clone()]);

    defaults::provider::with_config_defaults_provider_for_test(Arc::new(provider), || {
        let manager =
            ConfigManager::load_from_workspace(workspace_root).expect("failed to load config");

        assert_eq!(manager.config().agent.provider, "anthropic");
        assert_eq!(manager.config().agent.default_model, "claude-haiku-4-5");

        let layers = manager.layer_stack().layers();
        // User + Workspace
        assert_eq!(layers.len(), 2);
        assert!(matches!(layers[0].source, ConfigLayerSource::User { .. }));
        assert!(matches!(
            layers[1].source,
            ConfigLayerSource::Workspace { .. }
        ));
    });
}

#[test]
#[serial]
fn test_config_builder_overrides() {
    let workspace = assert_fs::TempDir::new().expect("failed to create workspace");
    let workspace_root = workspace.path();

    let workspace_config_path = workspace_root.join("vtcode.toml");
    fs::write(&workspace_config_path, "agent.provider = \"openai\"")
        .expect("failed to write workspace config");

    let static_paths = StaticWorkspacePaths::new(workspace_root, workspace_root.join(".vtcode"));
    let provider = WorkspacePathsDefaults::new(Arc::new(static_paths)).with_home_paths(vec![]);

    defaults::provider::with_config_defaults_provider_for_test(Arc::new(provider), || {
        let manager = ConfigBuilder::new()
            .workspace(workspace_root.to_path_buf())
            .cli_override(
                "agent.provider".to_string(),
                toml::Value::String("gemini".to_string()),
            )
            .cli_override(
                "agent.default_model".to_string(),
                toml::Value::String("gemini-1.5-pro".to_string()),
            )
            .build()
            .expect("failed to build config");

        assert_eq!(manager.config().agent.provider, "gemini");
        assert_eq!(manager.config().agent.default_model, "gemini-1.5-pro");

        let layers = manager.layer_stack().layers();
        // Workspace + Runtime
        assert_eq!(layers.len(), 2);
        assert!(matches!(
            layers[0].source,
            ConfigLayerSource::Workspace { .. }
        ));
        assert!(matches!(layers[1].source, ConfigLayerSource::Runtime));
    });
}

#[test]
fn test_insert_dotted_key() {
    let mut table = toml::Table::new();
    ConfigBuilder::insert_dotted_key(
        &mut table,
        "a.b.c",
        toml::Value::String("value".to_string()),
    );

    let a = table.get("a").unwrap().as_table().unwrap();
    let b = a.get("b").unwrap().as_table().unwrap();
    let c = b.get("c").unwrap().as_str().unwrap();
    assert_eq!(c, "value");
}

#[test]
fn test_merge_toml_values() {
    let mut base = toml::from_str::<toml::Value>(
        r#"
            [agent]
            provider = "openai"
            [tools]
            default_policy = "prompt"
        "#,
    )
    .unwrap();

    let overlay = toml::from_str::<toml::Value>(
        r#"
            [agent]
            provider = "anthropic"
            default_model = "claude-3"
        "#,
    )
    .unwrap();

    merge_toml_values(&mut base, &overlay);

    let agent = base.get("agent").unwrap().as_table().unwrap();
    assert_eq!(
        agent.get("provider").unwrap().as_str().unwrap(),
        "anthropic"
    );
    assert_eq!(
        agent.get("default_model").unwrap().as_str().unwrap(),
        "claude-3"
    );

    let tools = base.get("tools").unwrap().as_table().unwrap();
    assert_eq!(
        tools.get("default_policy").unwrap().as_str().unwrap(),
        "prompt"
    );
}

#[test]
fn syntax_highlighting_defaults_are_valid() {
    let config = SyntaxHighlightingConfig::default();
    config
        .validate()
        .expect("default syntax highlighting config should be valid");
    assert!(
        config
            .enabled_languages
            .iter()
            .any(|lang| lang.eq_ignore_ascii_case("markdown"))
    );
    assert!(
        config
            .enabled_languages
            .iter()
            .any(|lang| lang.eq_ignore_ascii_case("md"))
    );
    assert!(
        config
            .enabled_languages
            .iter()
            .any(|lang| lang.eq_ignore_ascii_case("bash"))
    );
    assert!(
        config
            .enabled_languages
            .iter()
            .any(|lang| lang.eq_ignore_ascii_case("sh"))
    );
    assert!(
        config
            .enabled_languages
            .iter()
            .any(|lang| lang.eq_ignore_ascii_case("shell"))
    );
    assert!(
        config
            .enabled_languages
            .iter()
            .any(|lang| lang.eq_ignore_ascii_case("zsh"))
    );
}

#[test]
fn vtcode_config_validation_fails_for_invalid_highlight_timeout() {
    let mut config = VTCodeConfig::default();
    config.syntax_highlighting.highlight_timeout_ms = 0;
    let error = config
        .validate()
        .expect_err("validation should fail for zero highlight timeout");
    assert!(
        format!("{:#}", error).contains("highlight"),
        "expected error to mention highlight, got: {:#}",
        error
    );
}

#[test]
fn load_from_file_rejects_invalid_syntax_highlighting() {
    let mut temp_file = NamedTempFile::new().expect("failed to create temp file");
    writeln!(
        temp_file,
        "[syntax_highlighting]\nhighlight_timeout_ms = 0\n"
    )
    .expect("failed to write temp config");

    let result = ConfigManager::load_from_file(temp_file.path());
    assert!(result.is_err(), "expected validation error");
    let error = format!("{:?}", result.err().unwrap());
    assert!(
        error.contains("validate"),
        "expected validation context in error, got: {}",
        error
    );
}

#[test]
fn loader_loads_prompt_cache_retention_from_toml() {
    use std::fs::File;
    use std::io::Write;

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("vtcode.toml");
    let mut file = File::create(&path).unwrap();
    let contents = r#"
[prompt_cache]
enabled = true
[prompt_cache.providers.openai]
prompt_cache_retention = "24h"
prompt_cache_key_mode = "off"
"#;
    file.write_all(contents.as_bytes()).unwrap();

    let manager = ConfigManager::load_from_file(&path).unwrap();
    let config = manager.config();
    assert_eq!(
        config.prompt_cache.providers.openai.prompt_cache_retention,
        Some("24h".to_string())
    );
    assert_eq!(
        config.prompt_cache.providers.openai.prompt_cache_key_mode,
        crate::core::OpenAIPromptCacheKeyMode::Off
    );
}

#[test]
fn loader_loads_tools_editor_config_from_toml() {
    use std::fs::File;
    use std::io::Write;

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("vtcode.toml");
    let mut file = File::create(&path).unwrap();
    let contents = r#"
[tools.editor]
enabled = true
preferred_editor = "code --wait"
suspend_tui = false
"#;
    file.write_all(contents.as_bytes()).unwrap();

    let manager = ConfigManager::load_from_file(&path).unwrap();
    let config = manager.config();
    assert!(config.tools.editor.enabled);
    assert_eq!(config.tools.editor.preferred_editor, "code --wait");
    assert!(!config.tools.editor.suspend_tui);
}

#[test]
fn save_config_preserves_comments() {
    use std::io::Write;

    let mut temp_file = NamedTempFile::new().expect("failed to create temp file");
    let config_with_comments = r#"# This is a test comment
[agent]
# Provider comment
provider = "openai"
default_model = "gpt-5-nano"

# Tools section comment
[tools]
default_policy = "prompt"
"#;

    write!(temp_file, "{}", config_with_comments).expect("failed to write temp config");
    temp_file.flush().expect("failed to flush");

    // Load config
    let manager = ConfigManager::load_from_file(temp_file.path()).expect("failed to load config");

    // Modify and save
    let mut modified_config = manager.config().clone();
    modified_config.agent.default_model = "gpt-5".to_string();

    ConfigManager::save_config_to_path(temp_file.path(), &modified_config)
        .expect("failed to save config");

    // Read back and verify comments are preserved
    let saved_content = fs::read_to_string(temp_file.path()).expect("failed to read saved config");

    assert!(
        saved_content.contains("# This is a test comment"),
        "top-level comment should be preserved"
    );
    assert!(
        saved_content.contains("# Provider comment"),
        "inline comment should be preserved"
    );
    assert!(
        saved_content.contains("# Tools section comment"),
        "section comment should be preserved"
    );
    assert!(
        saved_content.contains("gpt-5"),
        "modified value should be present"
    );
}

#[test]
#[serial]
fn config_defaults_provider_overrides_paths_and_theme() {
    let workspace = assert_fs::TempDir::new().expect("failed to create workspace");
    let workspace_root = workspace.path();
    let config_dir = workspace_root.join("config-root");
    fs::create_dir_all(&config_dir).expect("failed to create config directory");

    let config_file_name = "custom-config.toml";
    let config_path = config_dir.join(config_file_name);
    let serialized =
        toml::to_string(&VTCodeConfig::default()).expect("failed to serialize default config");
    fs::write(&config_path, serialized).expect("failed to write config file");

    let static_paths = StaticWorkspacePaths::new(workspace_root, &config_dir);
    let provider = WorkspacePathsDefaults::new(Arc::new(static_paths))
        .with_config_file_name(config_file_name)
        .with_home_paths(Vec::new())
        .with_syntax_theme("custom-theme")
        .with_syntax_languages(vec!["zig".to_string()]);

    defaults::provider::with_config_defaults_provider_for_test(Arc::new(provider), || {
        let manager = ConfigManager::load_from_workspace(workspace_root)
            .expect("failed to load workspace config");

        let resolved_path = manager
            .config_path()
            .expect("config path should be resolved");
        assert_eq!(resolved_path, config_path);

        assert_eq!(SyntaxHighlightingDefaults::theme(), "custom-theme");
        assert_eq!(
            SyntaxHighlightingDefaults::enabled_languages(),
            vec!["zig".to_string()]
        );
    });
}

#[test]
#[serial]
fn save_config_updates_disk_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let workspace = temp_dir.path();
    let config_path = workspace.join("vtcode.toml");

    // Write initial config
    let initial_config = r#"
[ui]
display_mode = "minimal"
show_sidebar = false
"#;
    fs::write(&config_path, initial_config).expect("failed to write initial config");

    // Load config
    let mut manager = ConfigManager::load_from_workspace(workspace).expect("failed to load config");
    assert_eq!(
        manager.config().ui.display_mode,
        crate::UiDisplayMode::Minimal
    );

    // Modify config (simulating /config palette changes)
    let mut modified_config = manager.config().clone();
    modified_config.ui.display_mode = crate::UiDisplayMode::Full;
    modified_config.ui.show_sidebar = true;

    // Save config
    manager
        .save_config(&modified_config)
        .expect("failed to save config");

    // Verify disk file was updated
    let saved_content = fs::read_to_string(&config_path).expect("failed to read saved config");
    assert!(
        saved_content.contains("display_mode = \"full\""),
        "saved config should contain full display_mode. Got:\n{}",
        saved_content
    );
    assert!(
        saved_content.contains("show_sidebar = true"),
        "saved config should contain show_sidebar = true. Got:\n{}",
        saved_content
    );

    // Create a NEW manager to simulate reopening /config palette
    let new_manager =
        ConfigManager::load_from_workspace(workspace).expect("failed to reload config");
    assert_eq!(
        new_manager.config().ui.display_mode,
        crate::UiDisplayMode::Full,
        "reloaded config should have full display_mode"
    );

    // Force disk read by loading from file directly
    let new_manager2 =
        ConfigManager::load_from_file(&config_path).expect("failed to reload from file");
    assert!(
        new_manager2.config().ui.show_sidebar,
        "reloaded config should have show_sidebar = true, got: {}",
        new_manager2.config().ui.show_sidebar
    );
}
