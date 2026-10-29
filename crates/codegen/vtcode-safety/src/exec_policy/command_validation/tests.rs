use super::*;
use tempfile::tempdir;

#[test]
fn test_validate_echo() {
    validate_echo(&[]).unwrap();
    validate_echo(&["hello".to_owned()]).unwrap();
    validate_echo(&["-n".to_owned(), "hello".to_owned()]).unwrap();
    validate_echo(&["-e".to_owned(), "test".to_owned()]).unwrap();
    assert!(validate_echo(&["--invalid".to_owned()]).is_err());
}

#[test]
fn test_validate_pwd() {
    validate_pwd(&[]).unwrap();
    assert!(validate_pwd(&["arg".to_owned()]).is_err());
}

#[test]
fn test_validate_printenv() {
    validate_printenv(&[]).unwrap();
    validate_printenv(&["PATH".to_owned()]).unwrap();
    validate_printenv(&["MY_VAR_123".to_owned()]).unwrap();
    assert!(validate_printenv(&["MY-VAR".to_owned()]).is_err());
    assert!(validate_printenv(&["MY VAR".to_owned()]).is_err());
    assert!(validate_printenv(&["VAR1".to_owned(), "VAR2".to_owned()]).is_err());
}

#[tokio::test]
async fn test_validate_git_read_only() {
    // Safe read-only operations
    validate_git_read_only("status", &[]).unwrap();
    validate_git_read_only("log", &["--oneline".to_owned()]).unwrap();
    validate_git_read_only("diff", &["-p".to_owned()]).unwrap();
    validate_git_read_only("show", &["HEAD".to_owned()]).unwrap();
    validate_git_read_only("branch", &["-a".to_owned()]).unwrap();

    // Dangerous patterns blocked
    assert!(validate_git_read_only("log", &["--format".to_owned(), "test;cat".to_owned()]).is_err());
}

#[test]
fn test_validate_git_commit() {
    // Valid commits
    validate_git_commit(&["-m".to_owned(), "fix: test".to_owned()]).unwrap();
    validate_git_commit(&["-a".to_owned()]).unwrap();
    validate_git_commit(&["--amend".to_owned()]).unwrap();

    // Invalid commits
    assert!(validate_git_commit(&["-m".to_owned()]).is_err()); // Missing message
    assert!(validate_git_commit(&["--invalid-flag".to_owned()]).is_err());
}

#[test]
fn test_validate_git_reset() {
    // Safe reset modes
    validate_git_reset(&["--soft".to_owned()], false).unwrap();
    validate_git_reset(&["--mixed".to_owned()], false).unwrap();
    validate_git_reset(&["--unstage".to_owned()], false).unwrap();
    validate_git_reset(&[], false).unwrap();

    // Dangerous reset modes
    assert!(validate_git_reset(&["--hard".to_owned()], false).is_err());
    assert!(validate_git_reset(&["--merge".to_owned()], false).is_err());
    assert!(validate_git_reset(&["--keep".to_owned()], false).is_err());
}

#[test]
fn test_validate_git_stash() {
    // Safe stash operations
    validate_git_stash(&["list".to_owned()]).unwrap();
    validate_git_stash(&["show".to_owned()]).unwrap();
    validate_git_stash(&["pop".to_owned()]).unwrap();
    validate_git_stash(&["apply".to_owned()]).unwrap();
    validate_git_stash(&["drop".to_owned()]).unwrap();

    // Dangerous operations
    assert!(validate_git_stash(&["push".to_owned()]).is_err());
    assert!(validate_git_stash(&["save".to_owned()]).is_err());
}

#[tokio::test]
async fn test_validate_git_safe_operations() {
    let workspace = PathBuf::from("/tmp");
    let working = PathBuf::from("/tmp");

    // Safe read-only operations should be allowed
    validate_git(&["status".to_owned()], &workspace, &working, false).await.unwrap();
    validate_git(&["log".to_owned(), "--oneline".to_owned()], &workspace, &working, false)
        .await
        .unwrap();
    validate_git(&["diff".to_owned()], &workspace, &working, false).await.unwrap();
    validate_git(&["show".to_owned(), "HEAD".to_owned()], &workspace, &working, false)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_validate_git_dangerous_operations_blocked() {
    let workspace = PathBuf::from("/tmp");
    let working = PathBuf::from("/tmp");

    // Dangerous operations should be blocked
    assert!(
        validate_git(&["push".to_owned(), "--force".to_owned()], &workspace, &working, false)
            .await
            .is_err()
    );
    assert!(
        validate_git(&["push".to_owned(), "-f".to_owned()], &workspace, &working, false)
            .await
            .is_err()
    );
    assert!(validate_git(&["clean".to_owned()], &workspace, &working, false).await.is_err());
    assert!(
        validate_git(&["filter-branch".to_owned()], &workspace, &working, false)
            .await
            .is_err()
    );
    assert!(validate_git(&["rebase".to_owned()], &workspace, &working, false).await.is_err());
    assert!(
        validate_git(&["cherry-pick".to_owned()], &workspace, &working, false)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn validate_command_rejects_workspace_traversal() {
    let workspace = tempdir().unwrap();
    let root = workspace.path();
    let command = vec!["cat".to_owned(), "../outside-secret".to_owned()];

    assert!(validate_command(&command, root, root, false).await.is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn validate_command_rejects_symlink_escape() {
    use std::os::unix::fs::symlink;

    let workspace = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let secret = outside.path().join("secret.txt");
    fs::write(&secret, b"outside workspace").await.unwrap();
    symlink(outside.path(), workspace.path().join("linked-outside")).unwrap();

    let command = vec!["cat".to_owned(), "linked-outside/secret.txt".to_owned()];

    assert!(
        validate_command(&command, workspace.path(), workspace.path(), false)
            .await
            .is_err()
    );
}

#[test]
fn test_validate_which() {
    validate_which(&["ls".to_owned()]).unwrap();
    validate_which(&["git".to_owned(), "-a".to_owned()]).unwrap();
    assert!(validate_which(&[]).is_err());
    assert!(validate_which(&["/usr/bin/ls".to_owned()]).is_err()); // Contains /
    assert!(validate_which(&["ls git".to_owned()]).is_err()); // Contains space
}
