use std::fs;
use std::path::PathBuf;

use super::*;

#[test]
fn builder_initializes_with_workspace_and_persistent_paths() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let environment = SandboxEnvironment::builder(&workspace).build();
    let canonical_workspace = workspace.canonicalize().expect("canonical workspace");

    assert_eq!(environment.workspace_root(), canonical_workspace.as_path());
    assert!(
        environment
            .allowed_paths()
            .any(|path| path.ends_with("persistent"))
    );
    assert!(
        environment
            .allowed_paths()
            .any(|path| path.starts_with(&canonical_workspace))
    );
}

#[test]
fn builder_resolves_relative_sandbox_root() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let environment = SandboxEnvironment::builder(&workspace)
        .sandbox_root(".vtcode/sandbox")
        .build();

    assert!(environment.sandbox_root().starts_with(&workspace));
    assert!(environment.settings_path().starts_with(&workspace));
}

#[test]
fn allow_path_rejects_escape_attempts() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let mut environment = SandboxEnvironment::builder(&workspace).build();

    let result = environment.allow_path("../outside");
    assert!(result.is_err());
}

#[test]
fn write_settings_persists_json() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");

    let environment = SandboxEnvironment::builder(&workspace)
        .sandbox_root("sandbox")
        .build();

    environment.write_settings().expect("settings to persist");

    let contents = fs::read_to_string(environment.settings_path()).expect("read settings");
    assert!(contents.contains("\"sandbox\""));
}

#[test]
fn create_profile_captures_allowed_paths() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    let bin_dir = temp_dir.path().join("bin");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::create_dir_all(&bin_dir).expect("bin");

    let mut environment = SandboxEnvironment::builder(&workspace)
        .sandbox_root("sandbox")
        .build();

    environment
        .allow_path(PathBuf::from("data"))
        .expect("allow data path");

    let runtime_binary = bin_dir.join("srt");
    fs::write(&runtime_binary, "#!/bin/sh").expect("runtime stub");

    let profile = environment.create_profile(runtime_binary);

    assert_eq!(
        profile.allowed_paths().len(),
        environment.allowed_paths().count()
    );
    assert_eq!(profile.runtime_kind(), environment.runtime_kind());
}
