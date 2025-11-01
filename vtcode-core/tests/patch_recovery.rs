use tempfile::TempDir;
use vtcode_core::tools::editing::{Patch, PatchError};

#[tokio::test]
async fn update_failure_preserves_original_content() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("file.txt");
    tokio::fs::write(&file_path, "original\n").await.unwrap();

    let patch_text =
        "*** Begin Patch\n*** Update File: file.txt\n@@\n-missing\n+changed\n*** End Patch";
    let patch = Patch::parse(patch_text).unwrap();

    let err = patch.apply(temp_dir.path()).await.unwrap_err();
    let patch_err = err.downcast::<PatchError>().unwrap();
    assert!(matches!(patch_err, PatchError::SegmentNotFound { .. }));

    let contents = tokio::fs::read_to_string(&file_path).await.unwrap();
    assert_eq!(contents, "original\n");
}

#[cfg(unix)]
#[tokio::test]
async fn update_failure_restores_file_after_partial_write() {
    use nix::unistd::Uid;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new().unwrap();
    let source_path = temp_dir.path().join("source.txt");
    tokio::fs::write(&source_path, "line\n").await.unwrap();

    if Uid::effective().is_root() {
        eprintln!("skipping permission failure test when running as root");
        return;
    }

    let locked_dir = temp_dir.path().join("locked");
    tokio::fs::create_dir(&locked_dir).await.unwrap();
    fs::set_permissions(&locked_dir, fs::Permissions::from_mode(0o555)).unwrap();

    let patch_text = "*** Begin Patch\n*** Update File: source.txt\n*** Move to: locked/target.txt\n@@\n-line\n+line updated\n*** End Patch";
    let patch = Patch::parse(patch_text).unwrap();

    let err = patch.apply(temp_dir.path()).await.unwrap_err();
    let patch_err = err.downcast::<PatchError>().unwrap();
    if let PatchError::Io { action, .. } = &patch_err {
        assert_eq!(*action, "create");
    } else {
        panic!("expected I/O error, got {patch_err:?}");
    }

    let contents = tokio::fs::read_to_string(&source_path).await.unwrap();
    assert_eq!(contents, "line\n");
    assert!(!locked_dir.join("target.txt").exists());

    fs::set_permissions(&locked_dir, fs::Permissions::from_mode(0o755)).unwrap();
}

#[tokio::test]
async fn add_file_validation_errors_when_target_exists() {
    let temp_dir = TempDir::new().unwrap();
    let existing_path = temp_dir.path().join("duplicate.txt");
    tokio::fs::write(&existing_path, "original\n")
        .await
        .unwrap();

    let patch_text = "*** Begin Patch\n*** Add File: duplicate.txt\n+hello\n*** End Patch";
    let patch = Patch::parse(patch_text).unwrap();

    let err = patch.apply(temp_dir.path()).await.unwrap_err();
    let patch_err = err.downcast::<PatchError>().unwrap();
    assert!(matches!(patch_err, PatchError::InvalidOperation { .. }));

    let contents = tokio::fs::read_to_string(existing_path).await.unwrap();
    assert_eq!(contents, "original\n");
}

#[tokio::test]
async fn update_move_writes_destination_and_cleans_backup() {
    let temp_dir = TempDir::new().unwrap();
    let source_path = temp_dir.path().join("source.txt");
    tokio::fs::write(&source_path, "alpha\n").await.unwrap();

    let patch_text = "*** Begin Patch\n*** Update File: source.txt\n*** Move to: renamed.txt\n@@\n-alpha\n+beta\n*** End Patch";
    let patch = Patch::parse(patch_text).unwrap();

    let messages = patch.apply(temp_dir.path()).await.unwrap();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("renamed.txt"));

    assert!(!source_path.exists());
    let renamed_path = temp_dir.path().join("renamed.txt");
    let renamed_contents = tokio::fs::read_to_string(&renamed_path).await.unwrap();
    assert_eq!(renamed_contents, "beta\n");
}

#[cfg(unix)]
#[tokio::test]
async fn update_preserves_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new().unwrap();
    let source_path = temp_dir.path().join("mode.txt");
    tokio::fs::write(&source_path, "keep\n").await.unwrap();

    let original_mode = 0o444;
    std::fs::set_permissions(&source_path, std::fs::Permissions::from_mode(original_mode)).unwrap();

    let patch_text =
        "*** Begin Patch\n*** Update File: mode.txt\n@@\n-keep\n+changed\n*** End Patch";
    let patch = Patch::parse(patch_text).unwrap();

    patch.apply(temp_dir.path()).await.unwrap();

    let contents = tokio::fs::read_to_string(&source_path).await.unwrap();
    assert_eq!(contents, "changed\n");

    let metadata = tokio::fs::metadata(&source_path).await.unwrap();
    assert_eq!(metadata.permissions().mode() & 0o777, original_mode);
}
