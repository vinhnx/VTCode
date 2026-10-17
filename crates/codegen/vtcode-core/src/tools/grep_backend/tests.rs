use super::*;

#[tokio::test]
async fn literal_backend_leading_dash_paths_and_empty_matches() {
    let workspace = tempfile::Builder::new().prefix("-literal-").tempdir_in(".").unwrap();
    let path = Path::new(workspace.path().file_name().unwrap()).join("-guide.md");
    std::fs::write(&path, "- [Sessions](#sessions)\nplain\n").unwrap();
    let found = search_literal_bounded("- [Sessions](#sessions)", &path, &[], 5).await.unwrap();
    assert_eq!(found.candidates.len(), 1);
    assert_eq!(found.candidates[0].line, 1);
    assert_eq!(found.candidates[0].matched_text, "- [Sessions](#sessions)");
    let empty = search_literal_bounded("absent", &path, &[], 5).await.unwrap();
    assert!(empty.candidates.is_empty());
    assert!(!empty.truncated);
    let error = search_literal_bounded("plain", &workspace.path().join("missing"), &[], 5)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("ripgrep literal search failed"));
}

#[cfg(unix)]
#[tokio::test]
async fn literal_backend_terminates_controlled_children_on_cancel_error_and_truncation() {
    use std::process::Command;

    for mode in ["cancel", "malformed", "truncated"] {
        let workspace = tempfile::tempdir().unwrap();
        let pid_path = workspace.path().join("pid");
        let script = match mode {
            "cancel" => "echo $$ > \"$1\"; exec sleep 30",
            "malformed" => "echo $$ > \"$1\"; printf 'invalid json\\n'; exec sleep 30",
            _ => {
                "echo $$ > \"$1\"; printf '%s\\n' '{\"type\":\"match\",\"data\":{\"path\":{\"text\":\"x.md\"},\"lines\":{\"text\":\"abc\"},\"submatches\":[{\"start\":0,\"end\":3,\"match\":{\"text\":\"abc\"}}]}}'; exec sleep 30"
            }
        };
        let mut command = TokioCommand::new("sh");
        command
            .args(["-c", script, "literal-test"])
            .arg(&pid_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let task = tokio::spawn(run_literal_child(command, "abc", 1));
        let pid = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Ok(pid) = tokio::fs::read_to_string(&pid_path).await
                    && !pid.trim().is_empty()
                {
                    break pid.trim().to_string();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        if mode == "cancel" {
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        } else {
            let result = tokio::time::timeout(Duration::from_secs(3), task).await.unwrap().unwrap();
            if mode == "malformed" {
                assert!(result.unwrap_err().to_string().contains("failed to parse ripgrep"));
            } else {
                let result = result.unwrap();
                assert!(result.truncated);
                assert_eq!(result.candidates.len(), 1);
            }
        }
        tokio::time::timeout(Duration::from_secs(3), async {
            while Command::new("kill")
                .args(["-0", &pid])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success()
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the owned search child must terminate");
    }
}
