use super::*;

#[tokio::test]
async fn completion_verification_code_targets_cannot_use_prose_or_planning_exemptions() {
    for path in ["CMakeLists.txt", "requirements.txt"] {
        let dir = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
        registry.allow_all_tools().await.unwrap();
        let check = registry
            .execute_harness_command_session(json!({"action":"run", "command":"rustc --version"}))
            .await
            .unwrap();
        assert_eq!(check["exit_code"], 0, "{check}");
        let output = registry
            .execute_tool(tools::WRITE_FILE, json!({"path":path, "content":"changed\n"}))
            .await
            .unwrap();
        assert!(output.get("error").is_none(), "{output}");
        assert_eq!(std::fs::read_to_string(dir.path().join(path)).unwrap(), "changed\n");
        assert!(registry.completion_verification_pending(), "code target {path}: {output}");
        assert!(!registry.completion_verifier_passed("rustc --version"));
    }
}
#[test]
fn completion_verification_temp_plans_root_matches_real_temp_dir() {
    use crate::tools::tool_intent::tmp_plans_dir;
    let root = tmp_plans_dir();
    let root_str = root.to_str().expect("temp plans root must be UTF-8").to_owned();
    for path in [
        root_str.clone(),
        format!("{root_str}/draft.md"),
        format!("{root_str}/workspace-slug/draft.md"),
    ] {
        assert!(
            tool_intent::is_plan_artifact_write(tools::WRITE_FILE, &json!({"path": path})),
            "{path} must classify as a plan artifact under {}",
            root.display()
        );
    }
}
#[test]
fn completion_verification_exemptions_require_all_normalized_targets() {
    for args in [
        json!({"action":"move", "path":".vtcode/plans/draft.rs", "destination":"src/main.rs"}),
        json!({"action":"write", "path":"src/main.rs", "filePath":".vtcode/plans/draft.rs"}),
        json!({"action":"write", "path":".vtcode/plans/../../src/main.rs"}),
    ] {
        assert!(!tool_intent::is_plan_artifact_write(tools::UNIFIED_FILE, &args), "{args}");
    }
    assert!(tool_intent::is_plan_artifact_write(tools::WRITE_FILE, &json!({"path":".vtcode/plans/draft.md"})));
    assert!(tool_intent::is_docs_only_write(tools::WRITE_FILE, &json!({"path":"notes.txt"})));
}
#[cfg(unix)]
#[tokio::test]
async fn completion_verification_uses_effective_targets_for_prose_and_plan_symlinks() {
    for alias in [".vtcode/plans/alias.rs", "notes.txt"] {
        let dir = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
        registry.allow_all_tools().await.unwrap();
        let code = dir.path().join("main.rs");
        std::fs::write(&code, "original\n").unwrap();
        let link = dir.path().join(alias);
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&code, &link).unwrap();
        let check = registry
            .execute_harness_command_session(json!({"action":"run", "command":"rustc --version"}))
            .await
            .unwrap();
        assert_eq!(check["exit_code"], 0, "{check}");
        let output = registry
            .execute_tool(tools::WRITE_FILE, json!({"path":alias, "content":"changed\n", "overwrite":true}))
            .await
            .unwrap();
        assert!(output.get("error").is_none(), "{output}");
        assert_eq!(std::fs::read_to_string(&code).unwrap(), "changed\n");
        assert!(registry.completion_verification_pending(), "effective code target via {alias}: {output}");
    }
}
#[tokio::test]
async fn completion_verification_exempt_file_results_preserve_existing_debt() {
    for debt in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
        registry.allow_all_tools().await.unwrap();
        if debt {
            registry.restore_completion_verification_debt();
        }
        let patch = registry
            .execute_tool(
                tools::APPLY_PATCH,
                json!({
                    "input":"*** Begin Patch\n*** Add File: README.md\n+Documentation\n*** End Patch"
                }),
            )
            .await
            .unwrap();
        assert!(patch.get("error").is_none(), "{patch}");
        assert_eq!(registry.completion_verification_pending(), debt, "{patch}");
        for path in ["notes.txt", ".vtcode/plans/draft.rs"] {
            let output = registry
                .execute_tool(tools::WRITE_FILE, json!({"path":path, "content":"prose\n"}))
                .await
                .unwrap();
            assert!(output.get("error").is_none(), "{output}");
            assert_eq!(registry.completion_verification_pending(), debt, "{output}");
        }
    }
}
