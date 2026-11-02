use super::*;

#[test]
fn sparse_config_omits_default_primary_agent_when_unmodified() {
    let config = VTCodeConfig::default();
    let value = ConfigManager::sparse_config_value(&config).expect("sparse config");

    assert!(value.get("default_primary_agent").is_none());
}

#[test]
fn sparse_config_persists_non_default_primary_agent() {
    let config = VTCodeConfig {
        default_primary_agent: "auto".to_string(),
        ..Default::default()
    };

    let value = ConfigManager::sparse_config_value(&config).expect("sparse config");

    assert_eq!(value.get("default_primary_agent").and_then(toml::Value::as_str), Some("auto"));
}

#[test]
fn has_explicit_top_level_key_matches_effective_config() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(workspace.join(".vtcode")).expect("workspace dot dir");

    let manager = crate::loader::ConfigBuilder::new()
        .workspace(workspace.clone())
        .build()
        .expect("manager");
    assert!(!manager.has_explicit_top_level_key("default_primary_agent"));
    assert!(
        manager
            .effective_config()
            .as_table()
            .is_none_or(|table| !table.contains_key("default_primary_agent"))
    );

    let overridden = crate::loader::ConfigBuilder::new()
        .workspace(workspace)
        .cli_override("default_primary_agent".to_owned(), toml::Value::String("duck".to_owned()))
        .build()
        .expect("overridden manager");
    assert!(overridden.has_explicit_top_level_key("default_primary_agent"));
    assert!(
        overridden
            .effective_config()
            .as_table()
            .is_some_and(|table| table.contains_key("default_primary_agent"))
    );
    assert!(!overridden.has_explicit_top_level_key("definitely_missing_key"));
}

#[test]
fn save_config_migrates_legacy_permissions_auto_permission_table() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let config_path = temp_dir.path().join("vtcode.toml");
    fs::write(
        &config_path,
        r#"
[permissions.auto_permission]
model = "legacy-reviewer"
max_consecutive_denials = 2
"#,
    )
    .expect("write legacy config");

    let mut config = VTCodeConfig::default();
    config.permissions.auto_permission.model = "legacy-reviewer".to_string();
    config.permissions.auto_permission.max_consecutive_denials = 2;

    ConfigManager::save_config_to_path(&config_path, &config).expect("save config");

    let saved_content = fs::read_to_string(&config_path).expect("read saved config");
    assert!(
        saved_content.contains("[permissions.auto]"),
        "canonical auto permission table should be persisted. Got:\n{saved_content}"
    );
    assert!(
        !saved_content.contains("auto_permission"),
        "legacy auto_permission table should be removed. Got:\n{saved_content}"
    );

    let reloaded = ConfigManager::load_from_file(&config_path).expect("reload saved config");
    assert_eq!(reloaded.config().permissions.auto_permission.model, "legacy-reviewer");
    assert_eq!(reloaded.config().permissions.auto_permission.max_consecutive_denials, 2);
}

#[test]
fn save_config_removes_legacy_permissions_auto_permission_when_sparse_default() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let config_path = temp_dir.path().join("vtcode.toml");
    fs::write(
        &config_path,
        r#"
[permissions.auto_permission]
max_consecutive_denials = 3
"#,
    )
    .expect("write legacy config");

    ConfigManager::save_config_to_path(&config_path, &VTCodeConfig::default()).expect("save config");

    let saved_content = fs::read_to_string(&config_path).expect("read saved config");
    assert!(
        !saved_content.contains("auto_permission"),
        "legacy auto_permission table should be removed. Got:\n{saved_content}"
    );
    assert!(
        !saved_content.contains("[permissions.auto]"),
        "default auto permission config should remain sparse. Got:\n{saved_content}"
    );

    ConfigManager::load_from_file(&config_path).expect("reload saved config");
}

#[test]
fn inaccessible_optional_global_layers_are_skipped() {
    let error = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
    assert!(should_skip_optional_global_error(
        &ConfigLayerSource::System { file: PathBuf::from("/etc/vtcode/vtcode.toml") },
        &error
    ));
    assert!(should_skip_optional_global_error(
        &ConfigLayerSource::User {
            file: PathBuf::from("/home/user/.config/vtcode/vtcode.toml")
        },
        &error
    ));
    assert!(!should_skip_optional_global_error(
        &ConfigLayerSource::Workspace { file: PathBuf::from("/workspace/vtcode.toml") },
        &error
    ));
}

#[test]
fn disabled_layer_reason_tracks_concrete_error_type() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let parse_path = temp_dir.path().join("invalid.toml");
    fs::write(&parse_path, "key = [").expect("write invalid TOML");
    let parse_error = ConfigManager::load_toml_from_file(&parse_path).expect_err("invalid TOML should fail");
    let parse_layer =
        ConfigManager::disabled_layer_from_error(ConfigLayerSource::Project { file: parse_path }, parse_error);

    assert_eq!(parse_layer.disabled_reason, Some(LayerDisabledReason::ParseError));

    let read_path = temp_dir.path().join("parse");
    fs::create_dir(&read_path).expect("create directory at config path");
    let read_error = ConfigManager::load_toml_from_file(&read_path).expect_err("reading a directory should fail");
    assert!(read_error.to_string().contains("parse"));
    let read_layer =
        ConfigManager::disabled_layer_from_error(ConfigLayerSource::Project { file: read_path }, read_error);

    assert_eq!(read_layer.disabled_reason, Some(LayerDisabledReason::LoadError));
}
