use super::*;
use tempfile::TempDir;
use tokio::io::{AsyncWriteExt, duplex};

#[cfg(unix)]
use std::os::unix::fs::symlink;
#[cfg(windows)]
use std::os::windows::fs::symlink_file as symlink;

async fn workspace(mutations_allowed: bool) -> (TempDir, FilesystemWorkspace) {
    let temp = TempDir::new().expect("temp dir");
    tokio::fs::write(temp.path().join("main.js"), "console.log('old');\n")
        .await
        .expect("seed");
    let adapter = FilesystemWorkspace::new(temp.path(), [], mutations_allowed)
        .await
        .expect("adapter");
    (temp, adapter)
}

#[tokio::test]
async fn proposal_apply_check_and_revert_validate_current_digests() {
    let (_temp, adapter) = workspace(true).await;
    let adapter = adapter.with_allowed_commands(["cargo", "printf"]);
    let snapshot = adapter.read_file("main.js").await.expect("read");
    let proposal = adapter
        .propose_changes(vec![FileChange {
            path: "main.js".to_string(),
            base_digest: snapshot.digest,
            content: "console.log('new');\n".to_string(),
        }])
        .await
        .expect("propose");
    let applied = adapter.apply_proposal(&proposal.proposal_id).await.expect("apply");
    let result = adapter.run_checks("printf ok").await.expect("check");
    assert!(
        result.exit_code == Some(0)
            || (result.exit_code == Some(71) && result.stderr.contains("sandbox_apply: Operation not permitted")),
        "unexpected check result: {result:?}"
    );
    assert_eq!(adapter.read_file("main.js").await.expect("read").content, "console.log('new');\n");
    let _ = adapter.revert_last_change(&applied.change_id).await.expect("revert");
    assert_eq!(adapter.read_file("main.js").await.expect("read").content, "console.log('old');\n");
}

#[test]
fn proposal_diff_contains_context_and_correct_file_ranges() {
    let before = FileSnapshot {
        path: "src/main.js".to_string(),
        content: (1..=12).map(|line| format!("line-{line}\n")).collect(),
        digest: String::new(),
    };
    let after = (1..=12)
        .map(|line| match line {
            2 => "changed-2\n".to_string(),
            10 => "changed-10\n".to_string(),
            _ => format!("line-{line}\n"),
        })
        .collect::<String>();
    let diff = FilesystemWorkspace::proposal_diff(
        &[before],
        &[FileChange {
            path: "src/main.js".to_string(),
            base_digest: String::new(),
            content: after,
        }],
    );

    assert_eq!(
        diff,
        "--- a/src/main.js\n+++ b/src/main.js\n@@ -1,5 +1,5 @@\n line-1\n-line-2\n+changed-2\n line-3\n line-4\n line-5\n@@ -7,6 +7,6 @@\n line-7\n line-8\n line-9\n-line-10\n+changed-10\n line-11\n line-12\n"
    );
}

#[test]
fn proposal_diff_handles_empty_files_and_missing_final_newlines() {
    let empty_before = FileSnapshot {
        path: "new.txt".to_string(),
        content: String::new(),
        digest: String::new(),
    };
    let empty_diff = FilesystemWorkspace::proposal_diff(
        &[empty_before],
        &[FileChange {
            path: "new.txt".to_string(),
            base_digest: String::new(),
            content: "first\nsecond\n".to_string(),
        }],
    );
    assert_eq!(empty_diff, "--- a/new.txt\n+++ b/new.txt\n@@ -0,0 +1,2 @@\n+first\n+second\n");

    let no_newline_before = FileSnapshot {
        path: "line.txt".to_string(),
        content: "old".to_string(),
        digest: String::new(),
    };
    let no_newline_diff = FilesystemWorkspace::proposal_diff(
        &[no_newline_before],
        &[FileChange {
            path: "line.txt".to_string(),
            base_digest: String::new(),
            content: "new".to_string(),
        }],
    );
    assert_eq!(
        no_newline_diff,
        "--- a/line.txt\n+++ b/line.txt\n@@ -1 +1 @@\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n"
    );

    let cr_diff = FilesystemWorkspace::proposal_diff(
        &[FileSnapshot {
            path: "cr.txt".to_string(),
            content: "one\rtwo\r".to_string(),
            digest: String::new(),
        }],
        &[FileChange {
            path: "cr.txt".to_string(),
            base_digest: String::new(),
            content: "one\rchanged\r".to_string(),
        }],
    );
    assert_eq!(cr_diff, "--- a/cr.txt\n+++ b/cr.txt\n@@ -1,2 +1,2 @@\n one\n-two\n+changed\n");
}

#[tokio::test]
async fn proposal_for_turn_rechecks_the_staged_snapshot() {
    let (temp, adapter) = workspace(false).await;
    let snapshot = adapter.read_file("main.js").await.expect("snapshot");
    let proposal = adapter
        .propose_changes(vec![FileChange {
            path: "main.js".to_string(),
            base_digest: snapshot.digest,
            content: "console.log('new');\n".to_string(),
        }])
        .await
        .expect("proposal");

    let handed_off = adapter
        .proposal_for_turn(&proposal.proposal_id)
        .await
        .expect("current proposal");
    assert_eq!(handed_off.unified_diff, proposal.unified_diff);

    tokio::fs::write(temp.path().join("main.js"), "external\n")
        .await
        .expect("external edit");
    assert!(matches!(
        adapter.proposal_for_turn(&proposal.proposal_id).await,
        Err(WebmcpError::Conflict { path, .. }) if path == "main.js"
    ));
}

#[tokio::test]
async fn headless_adapter_does_not_fake_agent_turns() {
    let (_temp, adapter) = workspace(false).await;
    let status = adapter.status().await.expect("status");
    assert!(!status.turns_available);
    assert!(matches!(
        adapter.request_turn("review the draft", None).await,
        Err(WebmcpError::Unsupported(message)) if message.contains("active VT Code runtime")
    ));
}

#[tokio::test]
async fn listing_skips_dependency_and_generated_directories() {
    let temp = TempDir::new().expect("temp dir");
    tokio::fs::write(temp.path().join("visible.txt"), "visible")
        .await
        .expect("visible file");
    tokio::fs::write(temp.path().join(".env"), "TOKEN=secret")
        .await
        .expect("dotenv file");
    tokio::fs::write(temp.path().join(".npmrc"), "//registry.example/:_authToken=secret")
        .await
        .expect("credential file");
    tokio::fs::create_dir_all(temp.path().join(".ssh"))
        .await
        .expect("credential directory");
    tokio::fs::write(temp.path().join(".ssh/id_ed25519"), "private key")
        .await
        .expect("private key");
    tokio::fs::create_dir_all(temp.path().join(".env.secrets"))
        .await
        .expect("dotenv directory");
    tokio::fs::write(temp.path().join(".env.secrets/token.txt"), "secret")
        .await
        .expect("dotenv secret");
    for directory in ["node_modules", "target", ".git", ".worktrees", "dist"] {
        let directory = temp.path().join(directory);
        tokio::fs::create_dir_all(&directory).await.expect("directory");
        tokio::fs::write(directory.join("hidden.txt"), "hidden")
            .await
            .expect("hidden file");
    }

    let adapter = FilesystemWorkspace::new(temp.path(), [], false).await.expect("adapter");
    let files = adapter.list_files().await.expect("list files");
    assert_eq!(files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), vec!["visible.txt"]);
    assert!(matches!(adapter.read_file(".env").await, Err(WebmcpError::PathRejected(_))));
    assert!(matches!(adapter.read_file(".npmrc").await, Err(WebmcpError::PathRejected(_))));
    assert!(matches!(adapter.read_file(".ssh/id_ed25519").await, Err(WebmcpError::PathRejected(_))));
    assert!(matches!(adapter.read_file(".env.secrets/token.txt").await, Err(WebmcpError::PathRejected(_))));
}

#[test]
fn webmcp_check_sandbox_blocks_case_variants_of_sensitive_files() {
    let temp = TempDir::new().expect("workspace");
    for name in ["ID_ECDSA", ".ENV", "credentials.JSON"] {
        std::fs::write(temp.path().join(name), "secret").expect("sensitive file");
    }
    let nested = temp.path().join("project");
    std::fs::create_dir_all(nested.join(".ssh")).expect("nested credential directory");
    std::fs::write(nested.join(".ssh/id_ed25519"), "private key").expect("nested private key");
    std::fs::write(nested.join(".env"), "TOKEN=secret").expect("nested dotenv file");

    let policy = check_sandbox_policy(temp.path()).expect("sandbox policy");

    for path in [
        temp.path().join("ID_ECDSA"),
        temp.path().join(".ENV"),
        temp.path().join("credentials.JSON"),
        nested.join(".ssh/id_ed25519"),
        nested.join(".env"),
    ] {
        assert!(!policy.is_path_readable(&path), "check sandbox read allowed: {path:?}");
        assert!(!policy.is_path_writable(&path, temp.path()), "check sandbox write allowed: {path:?}");
    }
}

#[cfg(any(unix, windows))]
#[tokio::test]
async fn stale_and_traversal_requests_fail_closed() {
    let (temp, adapter) = workspace(false).await;
    let stale = FileChange {
        path: "main.js".to_string(),
        base_digest: "sha256:stale".to_string(),
        content: "new".to_string(),
    };
    assert!(matches!(adapter.propose_changes(vec![stale]).await, Err(WebmcpError::Conflict { .. })));
    assert!(matches!(adapter.read_file("../outside").await, Err(WebmcpError::PathRejected(_))));
    assert!(matches!(
        adapter.run_checks("printf safe; echo injected").await,
        Err(WebmcpError::InvalidRequest(_))
    ));
    assert!(matches!(adapter.run_checks("printf safe").await, Err(WebmcpError::InvalidRequest(_))));
    assert!(matches!(adapter.run_checks("cargo run").await, Err(WebmcpError::InvalidRequest(_))));
    assert!(matches!(adapter.run_checks("npm run check").await, Err(WebmcpError::InvalidRequest(_))));
    assert!(matches!(adapter.run_checks("python3 -c 'print(1)'").await, Err(WebmcpError::InvalidRequest(_))));
    assert!(matches!(adapter.run_checks("env").await, Err(WebmcpError::InvalidRequest(_))));
    assert!(matches!(adapter.run_checks("cargo check").await, Err(WebmcpError::ApprovalRequired)));

    let (_checks_temp, checks_disabled) = workspace(true).await;
    let checks_disabled = checks_disabled.with_checks_allowed(false);
    assert!(matches!(checks_disabled.run_checks("cargo check").await, Err(WebmcpError::ApprovalRequired)));

    let outside = temp.path().join("outside.txt");
    tokio::fs::write(&outside, "secret").await.expect("outside");
    symlink(&outside, temp.path().join("link.txt")).expect("symlink");
    assert!(matches!(adapter.read_file("link.txt").await, Err(WebmcpError::PathRejected(_))));
    assert!(matches!(adapter.apply_proposal("missing").await, Err(WebmcpError::ApprovalRequired)));
}

#[cfg(unix)]
#[tokio::test]
async fn nested_symlink_components_are_not_read() {
    let (_temp, adapter) = workspace(false).await;
    let outside = TempDir::new().expect("outside temp dir");
    tokio::fs::write(outside.path().join("secret.txt"), "secret")
        .await
        .expect("outside file");
    symlink(outside.path(), adapter.root().join("linked")).expect("directory symlink");

    let result = adapter.read_file("linked/secret.txt").await;
    assert!(matches!(&result, Err(WebmcpError::PathRejected(_))), "result={result:?}");
}

#[tokio::test]
async fn invalid_explicit_allowed_roots_are_rejected() {
    let temp = TempDir::new().expect("temp dir");
    let file_root = temp.path().join("not-a-directory");
    tokio::fs::write(&file_root, "file").await.expect("seed file");
    assert!(matches!(
        FilesystemWorkspace::new(temp.path(), [file_root], false).await,
        Err(WebmcpError::PathRejected(_))
    ));
}

#[tokio::test]
async fn multiple_allowed_roots_are_rejected_until_root_selection_exists() {
    let temp = TempDir::new().expect("temp dir");
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    tokio::fs::create_dir_all(&first).await.expect("first root");
    tokio::fs::create_dir_all(&second).await.expect("second root");
    assert!(matches!(
        FilesystemWorkspace::new(&first, [first.clone(), second], false).await,
        Err(WebmcpError::InvalidRequest(_))
    ));
}

#[tokio::test]
async fn reads_and_check_output_are_bounded() {
    let (_temp, adapter) = workspace(true).await;
    let limited = adapter.with_limits(FilesystemLimits { max_file_bytes: 4, ..FilesystemLimits::default() });
    assert!(matches!(limited.read_file("main.js").await, Err(WebmcpError::LimitExceeded)));

    let (mut writer, reader) = duplex(8192);
    let writer_task = tokio::spawn(async move {
        writer
            .write_all(&vec![b'x'; MAX_CHECK_OUTPUT_BYTES + 1])
            .await
            .expect("write test output");
    });
    assert!(matches!(read_process_output(reader).await, Err(WebmcpError::LimitExceeded)));
    writer_task.await.expect("test output writer");
}

#[test]
fn digest_text_matches_known_sha256_vectors() {
    // NIST/FIPS standard vectors, independent of the implementation: the
    // single-buffer `write!` rewrite must stay byte-identical to `format!`.
    assert_eq!(digest_text(""), "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert_eq!(digest_text("abc"), "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}

#[tokio::test]
async fn rollback_does_not_overwrite_an_external_change() {
    let (_temp, adapter) = workspace(true).await;
    let original = adapter.read_file("main.js").await.expect("original");
    let applied = FileSnapshot {
        path: original.path.clone(),
        content: "applied\n".to_string(),
        digest: digest_text("applied\n"),
    };
    let proposal = adapter
        .propose_changes(vec![FileChange {
            path: applied.path.clone(),
            base_digest: original.digest.clone(),
            content: applied.content.clone(),
        }])
        .await
        .expect("proposal");
    let _ = adapter.apply_proposal(&proposal.proposal_id).await.expect("apply snapshot");
    tokio::fs::write(adapter.root().join("main.js"), "external\n")
        .await
        .expect("external change");

    assert!(matches!(
        adapter
            .rollback_snapshots(std::slice::from_ref(&applied), std::slice::from_ref(&original))
            .await,
        Err(WebmcpError::PartialApply)
    ));

    assert_eq!(adapter.read_file("main.js").await.expect("current").content, "external\n");
}

#[tokio::test]
async fn apply_rejects_an_external_change_after_proposal() {
    let (_temp, adapter) = workspace(true).await;
    let original = adapter.read_file("main.js").await.expect("original");
    let proposal = adapter
        .propose_changes(vec![FileChange {
            path: original.path.clone(),
            base_digest: original.digest.clone(),
            content: "proposed\n".to_string(),
        }])
        .await
        .expect("proposal");
    tokio::fs::write(adapter.root().join("main.js"), "external\n")
        .await
        .expect("external change");

    assert!(matches!(
        adapter.apply_proposal(&proposal.proposal_id).await,
        Err(WebmcpError::Conflict { path, .. }) if path == "main.js"
    ));
    assert_eq!(adapter.read_file("main.js").await.expect("current").content, "external\n");
}

#[tokio::test]
async fn apply_reports_a_grown_file_as_a_conflict() {
    let (_temp, adapter) = workspace(true).await;
    let original = adapter.read_file("main.js").await.expect("original");
    let proposal = adapter
        .propose_changes(vec![FileChange {
            path: original.path.clone(),
            base_digest: original.digest.clone(),
            content: "proposed\n".to_string(),
        }])
        .await
        .expect("proposal");
    let mut file = tokio::fs::OpenOptions::new()
        .append(true)
        .open(adapter.root().join("main.js"))
        .await
        .expect("external append");
    file.write_all(b"external append\n").await.expect("append");
    file.flush().await.expect("flush");

    let result = adapter.apply_proposal(&proposal.proposal_id).await;
    assert!(matches!(result, Err(WebmcpError::Conflict { actual, .. }) if actual.starts_with("size:")));
    assert!(
        tokio::fs::read_to_string(adapter.root().join("main.js"))
            .await
            .expect("current")
            .ends_with("external append\n")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn apply_rejects_a_path_replaced_with_a_symlink() {
    let (_temp, adapter) = workspace(true).await;
    let original = adapter.read_file("main.js").await.expect("original");
    let proposal = adapter
        .propose_changes(vec![FileChange {
            path: original.path.clone(),
            base_digest: original.digest,
            content: "proposed\n".to_string(),
        }])
        .await
        .expect("proposal");
    let outside = TempDir::new().expect("outside temp dir");
    let outside_file = outside.path().join("outside.txt");
    tokio::fs::write(&outside_file, "outside\n").await.expect("outside file");
    tokio::fs::remove_file(adapter.root().join("main.js"))
        .await
        .expect("remove workspace file");
    symlink(&outside_file, adapter.root().join("main.js")).expect("replacement symlink");

    assert!(matches!(adapter.apply_proposal(&proposal.proposal_id).await, Err(WebmcpError::PathRejected(_))));
    assert_eq!(tokio::fs::read_to_string(outside_file).await.expect("outside content"), "outside\n");
}
