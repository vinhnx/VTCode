use super::*;

#[test]
fn source_inspection_rejects_unseen_and_stale_source() {
    let inspection = SourceInspection::default();
    inspection.set_mode(InspectionMode::Enforce);
    let path = PathBuf::from("/fixture/doc.md");
    assert!(inspection.require_full(&path, "first\nsecond\n").is_err());
    inspection.insert_lines(path.clone(), "first\nsecond\n", vec![0], "read-1".into(), false);
    inspection.0.lock().active.insert("read-1".into());
    assert!(
        inspection
            .require(&path, "first\nsecond\n", std::slice::from_ref(&(0..1)))
            .is_ok()
    );
    assert!(
        inspection
            .require(&path, "first\nsecond\n", std::slice::from_ref(&(1..2)))
            .is_err()
    );
    assert!(
        inspection
            .require(&path, "other\nsecond\n", std::slice::from_ref(&(0..1)))
            .is_err()
    );
}

use crate::config::constants::tools;
use crate::llm::provider::{Message, ToolCall};
use crate::tools::registry::ToolRegistry;

fn pair(id: &str, name: &str, args: &Value, result: &Value) -> [Message; 2] {
    let mut call = Message::assistant(String::new());
    call.tool_calls = Some(vec![ToolCall::function(id.into(), name.into(), args.to_string())]);
    [call, Message::tool_response(id.into(), result.to_string())]
}

async fn fixture() -> (tempfile::TempDir, ToolRegistry) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("sample.md"), "alpha\nbeta\ngamma\ndelta\n").unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    registry.allow_all_tools().await.unwrap();
    registry.set_inspection_mode(InspectionMode::Enforce);
    (dir, registry)
}

async fn expose(
    registry: &ToolRegistry,
    history: &mut Vec<Message>,
    id: &str,
    path: &str,
    offset: usize,
    limit: usize,
) {
    let args = json!({"path":path,"offset":offset,"limit":limit,"max_tokens":4096,"condense":false});
    let result = registry.execute_tool(tools::READ_FILE, args.clone()).await.unwrap();
    assert!(result.get("error").is_none(), "{result}");
    history.extend(pair(id, tools::READ_FILE, &args, &result));
    registry.reconcile_source_inspection(history);
}

fn patch(path: &str, old: &str, new: &str) -> Value {
    json!({"input":format!("*** Begin Patch\n*** Update File: {path}\n@@\n-{old}\n+{new}\n*** End Patch")})
}

#[tokio::test]
async fn source_inspection_partial_read_allows_only_matched_hunk_and_authored_followups() {
    let (dir, registry) = fixture().await;
    let mut history = Vec::new();
    expose(&registry, &mut history, "read", "sample.md", 2, 1).await;
    let blind = registry
        .execute_tool(tools::APPLY_PATCH, patch("sample.md", "gamma", "blind"))
        .await
        .unwrap();
    assert!(blind.to_string().contains("source_inspection_required"), "{blind}");
    let first = patch("sample.md", "beta", "authored\n+second");
    let result = registry.execute_tool(tools::APPLY_PATCH, first.clone()).await.unwrap();
    assert!(result.get("error").is_none(), "{result}");
    history.extend(pair("edit-1", tools::APPLY_PATCH, &first, &result));
    registry.reconcile_source_inspection(&history);
    let result = registry
        .execute_tool(tools::APPLY_PATCH, patch("sample.md", "authored", "again"))
        .await
        .unwrap();
    assert!(result.get("error").is_none(), "{result}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("sample.md")).unwrap(),
        "alpha\nagain\nsecond\ngamma\ndelta\n"
    );
}

#[tokio::test]
async fn source_inspection_internal_reads_and_proposed_source_do_not_authorize() {
    let (dir, registry) = fixture().await;
    registry
        .edited_file_monitor_ref()
        .track_read(&dir.path().join("sample.md"))
        .await
        .unwrap();
    let result = registry
        .execute_tool(tools::APPLY_PATCH, patch("sample.md", "alpha", "bad"))
        .await
        .unwrap();
    assert!(result.to_string().contains("source_inspection_required"), "{result}");
    assert_eq!(std::fs::read_to_string(dir.path().join("sample.md")).unwrap(), "alpha\nbeta\ngamma\ndelta\n");
}

#[tokio::test]
async fn source_inspection_clear_and_resume_revoke_read_evidence() {
    let (dir, registry) = fixture().await;
    let mut history = Vec::new();
    expose(&registry, &mut history, "read", "sample.md", 1, 4).await;
    registry.reconcile_source_inspection(&[Message::user("Summary: inspect sample.md".into())]);
    let result = registry
        .execute_tool(tools::APPLY_PATCH, patch("sample.md", "alpha", "bad"))
        .await
        .unwrap();
    assert!(result.to_string().contains("source_inspection_required"));
    // Restoring old read messages cannot resurrect evicted/cleared evidence.
    registry.reconcile_source_inspection(&history);
    let result = registry
        .execute_tool(tools::APPLY_PATCH, patch("sample.md", "alpha", "bad"))
        .await
        .unwrap();
    assert!(result.to_string().contains("source_inspection_required"));
    let resumed = ToolRegistry::new(dir.path().to_path_buf()).await;
    resumed.allow_all_tools().await.unwrap();
    resumed.set_inspection_mode(InspectionMode::Enforce);
    resumed.reconcile_source_inspection(&history);
    let result = resumed
        .execute_tool(tools::APPLY_PATCH, patch("sample.md", "alpha", "bad"))
        .await
        .unwrap();
    assert!(result.to_string().contains("source_inspection_required"));
}

#[tokio::test]
async fn source_inspection_multifile_rejection_has_no_partial_writes() {
    let (dir, registry) = fixture().await;
    let mut history = Vec::new();
    expose(&registry, &mut history, "read", "sample.md", 1, 1).await;
    std::fs::write(dir.path().join("other.txt"), "unseen\n").unwrap();
    let args = json!({"input":"*** Begin Patch\n*** Update File: sample.md\n@@\n-alpha\n+good\n*** Update File: other.txt\n@@\n-unseen\n+bad\n*** End Patch"});
    let result = registry.execute_tool(tools::APPLY_PATCH, args).await.unwrap();
    assert!(result.to_string().contains("source_inspection_required"));
    assert!(
        std::fs::read_to_string(dir.path().join("sample.md"))
            .unwrap()
            .starts_with("alpha\n")
    );
    assert_eq!(std::fs::read_to_string(dir.path().join("other.txt")).unwrap(), "unseen\n");
}

#[tokio::test]
async fn source_inspection_stale_revision_and_wrong_file_are_rejected() {
    let (dir, registry) = fixture().await;
    let mut history = Vec::new();
    expose(&registry, &mut history, "read", "sample.md", 1, 4).await;
    std::fs::write(dir.path().join("other.md"), "alpha\n").unwrap();
    let wrong = registry
        .execute_tool(tools::APPLY_PATCH, patch("other.md", "alpha", "wrong"))
        .await
        .unwrap();
    assert!(wrong.to_string().contains("source_inspection_required"));
    std::fs::write(dir.path().join("sample.md"), "alpha\nexternal\ngamma\ndelta\n").unwrap();
    let stale = registry
        .execute_tool(tools::APPLY_PATCH, patch("sample.md", "alpha", "wrong"))
        .await
        .unwrap();
    assert_eq!(stale["conflict_detected"], true, "external conflict precedes source inspection: {stale}");
    assert_eq!(std::fs::read_to_string(dir.path().join("sample.md")).unwrap(), "alpha\nexternal\ngamma\ndelta\n");
}

#[test]
fn source_inspection_shell_exemption_is_exact_and_approval_does_not_grant_it() {
    let inspection = SourceInspection::default();
    inspection.set_mode(InspectionMode::Enforce);
    let cwd = Path::new("/fixture");
    for command in [
        "sed -i s/a/b/ sample.md",
        "python3 -c 'open(\"sample.md\",\"w\").write(\"bad\")'",
        "cat > sample.md <<EOF\nbad\nEOF",
        "cargo fmt --all",
    ] {
        assert!(inspection.check_shell(&json!({"cmd":command,"confirm":true}), cwd).is_err(), "{command}");
    }
    let args = json!({"cmd":"cargo fmt --all"});
    inspection.exempt_user_command(&args, cwd);
    assert!(inspection.check_shell(&args, cwd).is_ok());
    assert!(inspection.check_shell(&args, Path::new("/other")).is_err());
    assert!(
        inspection
            .check_shell(&json!({"cmd":"cargo fmt --all && touch sample.md"}), cwd)
            .is_err()
    );
    assert!(inspection.check_shell(&json!({"cmd":"cargo check --locked"}), cwd).is_ok());
}

#[tokio::test]
async fn source_inspection_whole_file_write_and_delete_need_complete_text() {
    let (dir, registry) = fixture().await;
    let mut history = Vec::new();
    expose(&registry, &mut history, "part", "sample.md", 2, 1).await;
    assert!(
        registry
            .file_ops_tool()
            .write_file(json!({"path":"sample.md", "content":"replacement\n", "mode":"overwrite"}))
            .await
            .is_err()
    );
    assert!(registry.file_ops_tool().delete_file(json!({"path":"sample.md"})).await.is_err());
    expose(&registry, &mut history, "full", "sample.md", 1, 4).await;
    let result = registry.file_ops_tool().delete_file(json!({"path":"sample.md"})).await.unwrap();
    assert_eq!(result["deleted"], true);
    assert!(!dir.path().join("sample.md").exists());
}

#[tokio::test]
async fn source_inspection_insertion_requires_both_adjacent_lines() {
    let (dir, registry) = fixture().await;
    let mut history = Vec::new();
    expose(&registry, &mut history, "anchor", "sample.md", 2, 1).await;
    let args = json!({"input":"*** Begin Patch\n*** Update File: sample.md\n@@ beta\n+inserted\n*** End Patch"});
    let result = registry.execute_tool(tools::APPLY_PATCH, args.clone()).await.unwrap();
    assert!(result.to_string().contains("source_inspection_required"));
    expose(&registry, &mut history, "adjacent", "sample.md", 3, 1).await;
    let result = registry.execute_tool(tools::APPLY_PATCH, args).await.unwrap();
    assert!(result.get("error").is_none(), "{result}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("sample.md")).unwrap(),
        "alpha\nbeta\ninserted\ngamma\ndelta\n"
    );
}

#[tokio::test]
async fn source_inspection_repeated_text_checks_the_selected_occurrence() {
    let (dir, registry) = fixture().await;
    std::fs::write(dir.path().join("sample.md"), "same\nseparator\nsame\n").unwrap();
    let mut history = Vec::new();
    expose(&registry, &mut history, "last", "sample.md", 3, 1).await;
    let result = registry
        .execute_tool(tools::APPLY_PATCH, patch("sample.md", "same", "changed"))
        .await
        .unwrap();
    assert!(result.to_string().contains("source_inspection_required"));
    assert_eq!(std::fs::read_to_string(dir.path().join("sample.md")).unwrap(), "same\nseparator\nsame\n");
    let args =
        json!({"input":"*** Begin Patch\n*** Update File: sample.md\n@@ -3,1 +3,1 @@\n-same\n+changed\n*** End Patch"});
    let result = registry.execute_tool(tools::APPLY_PATCH, args).await.unwrap();
    assert!(result.get("error").is_none(), "{result}");
    assert_eq!(std::fs::read_to_string(dir.path().join("sample.md")).unwrap(), "same\nseparator\nchanged\n");
}

#[tokio::test]
async fn source_inspection_truncated_and_summary_outputs_never_credit_unseen_lines() {
    let (dir, registry) = fixture().await;
    let path = vtcode_commons::canonicalize(dir.path().join("sample.md")).unwrap();
    let args = json!({"path":"sample.md", "offset":1,"limit":4});
    let output = json!({"path":"sample.md","content":"alpha\nbet...[truncated]","success":true});
    registry.observe_source_output(tools::READ_FILE, &args, &output).await;
    registry.reconcile_source_inspection(&pair("truncated", tools::READ_FILE, &args, &output));
    let inspection = registry.edited_file_monitor_ref().source_inspection();
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(inspection.require(&path, &content, std::slice::from_ref(&(0..1))).is_ok());
    assert!(inspection.require(&path, &content, std::slice::from_ref(&(1..2))).is_err());
    let summary = json!({"success":true, "content":"Read sample.md: four lines"});
    registry.reconcile_source_inspection(&pair("truncated", tools::READ_FILE, &args, &summary));
    assert!(inspection.require(&path, &content, std::slice::from_ref(&(0..1))).is_err());
}

#[tokio::test]
async fn source_inspection_new_file_is_authorized_only_by_retained_authored_patch() {
    let (dir, registry) = fixture().await;
    let args = json!({"input":"*** Begin Patch\n*** Add File: created.txt\n+authored\n*** End Patch"});
    let output = registry.execute_tool(tools::APPLY_PATCH, args.clone()).await.unwrap();
    assert!(output.get("error").is_none(), "{output}");
    registry.reconcile_source_inspection(&pair("created", tools::APPLY_PATCH, &args, &output));
    let result = registry
        .execute_tool(tools::APPLY_PATCH, patch("created.txt", "authored", "second"))
        .await
        .unwrap();
    assert!(result.get("error").is_none(), "{result}");
    assert_eq!(std::fs::read_to_string(dir.path().join("created.txt")).unwrap(), "second\n");
}

#[test]
fn source_inspection_eviction_and_modes_are_conservative() {
    let inspection = SourceInspection::default();
    inspection.set_mode(InspectionMode::Enforce);
    inspection.0.lock().active.insert("read".into());
    for index in 0..=MAX_FILES {
        inspection.insert_lines(PathBuf::from(format!("/file-{index}")), "source\n", vec![0], "read".into(), false);
    }
    assert!(inspection.require_full(Path::new("/file-0"), "source\n").is_err());
    assert!(
        inspection
            .require_full(Path::new(&format!("/file-{MAX_FILES}")), "source\n")
            .is_ok()
    );
    inspection.set_mode(InspectionMode::Warn);
    assert!(inspection.require_full(Path::new("/unseen"), "source\n").is_ok());
    inspection.set_mode(InspectionMode::Off);
    assert!(inspection.check_shell(&json!({"cmd":"unknown writer"}), Path::new("/")).is_ok());
}

#[cfg(unix)]
#[tokio::test]
async fn source_inspection_canonical_alias_reuses_coverage_without_wrong_path_credit() {
    let (dir, registry) = fixture().await;
    std::os::unix::fs::symlink(dir.path().join("sample.md"), dir.path().join("alias.md")).unwrap();
    let mut history = Vec::new();
    expose(&registry, &mut history, "alias", "alias.md", 1, 1).await;
    let output = registry
        .execute_tool(tools::APPLY_PATCH, patch("sample.md", "alpha", "aliased"))
        .await
        .unwrap();
    assert!(output.get("error").is_none(), "{output}");
    assert!(
        std::fs::read_to_string(dir.path().join("sample.md"))
            .unwrap()
            .starts_with("aliased\n")
    );
}

#[tokio::test]
async fn source_inspection_passing_build_never_authorizes_unseen_source() {
    let (dir, registry) = fixture().await;
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname='inspection_fixture'\nversion='0.0.0'\nedition='2024'\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "pub const VALUE: u8 = 7;\n").unwrap();
    let output = registry
        .execute_harness_command_session(json!({"action":"run", "command":"cargo check --quiet", "yield_time_ms":1000}))
        .await
        .unwrap();
    assert_eq!(output["exit_code"], 0, "{output}");
    let result = registry
        .execute_tool(tools::APPLY_PATCH, patch("sample.md", "alpha", "blind"))
        .await
        .unwrap();
    assert!(result.to_string().contains("source_inspection_required"), "{result}");
}

#[tokio::test]
async fn source_inspection_shell_reads_use_cwd_and_credit_verbatim_lines() {
    let (dir, registry) = fixture().await;
    std::fs::create_dir(dir.path().join("nested")).unwrap();
    std::fs::write(dir.path().join("nested/sample.md"), "different\nbody\n").unwrap();
    let args = json!({"cmd":"sed -n '1,1p' sample.md", "workdir":"nested", "yield_time_ms":1000});
    let output = registry.execute_tool(tools::EXEC_COMMAND, args.clone()).await.unwrap();
    assert_eq!(output["exit_code"], 0, "{output}");
    registry.reconcile_source_inspection(&pair("shell-read", tools::EXEC_COMMAND, &args, &output));
    let result = registry
        .execute_tool(tools::APPLY_PATCH, patch("nested/sample.md", "different", "edited"))
        .await
        .unwrap();
    assert!(result.get("error").is_none(), "{result}");
    let result = registry
        .execute_tool(tools::APPLY_PATCH, patch("sample.md", "alpha", "wrong-file"))
        .await
        .unwrap();
    assert!(result.to_string().contains("source_inspection_required"), "{result}");
}

#[tokio::test]
async fn source_inspection_all_public_and_prevalidated_patch_routes_reject_unseen_source() {
    let (_dir, registry) = fixture().await;
    let args = patch("sample.md", "alpha", "blind");
    for name in [tools::APPLY_PATCH, "applypatch"] {
        match registry.execute_tool(name, args.clone()).await {
            Ok(result) => assert!(result.get("error").is_some(), "{name}: {result}"),
            Err(error) => assert!(error.to_string().contains("policy"), "{name}: {error}"),
        }
    }
    let result = registry.execute_public_tool_ref(tools::APPLY_PATCH, &args).await.unwrap();
    assert!(result.to_string().contains("source_inspection_required"));
    let result = registry.execute_tool_ref_prevalidated(tools::APPLY_PATCH, &args).await.unwrap();
    assert!(result.to_string().contains("source_inspection_required"));
}

#[tokio::test]
async fn source_inspection_mandatory_refresh_shares_patch_quota_without_replenishment() {
    let (_dir, registry) = fixture().await;
    let output = registry
        .execute_tool(tools::APPLY_PATCH, patch("sample.md", "alpha", "blind"))
        .await
        .unwrap();
    assert!(output.to_string().contains("source_inspection_required"));
    let read = json!({"path":"sample.md","offset":1,"limit":200,"condense":false});
    assert!(registry.has_patch_recovery_read(tools::READ_FILE, &read));
    let result = registry.execute_tool(tools::READ_FILE, read.clone()).await.unwrap();
    assert!(result.get("error").is_none());
    assert!(!registry.has_patch_recovery_read(tools::READ_FILE, &read));
    // No request retained the read, so the edit is still unauthorized; this
    // rejection must not refill the quota consumed above.
    let output = registry
        .execute_tool(tools::APPLY_PATCH, patch("sample.md", "alpha", "blind-again"))
        .await
        .unwrap();
    assert!(output.to_string().contains("source_inspection_required"));
    assert!(!registry.has_patch_recovery_read(tools::READ_FILE, &read));
}

#[tokio::test]
async fn source_inspection_surviving_ranges_need_the_retained_patch_mapping() {
    let (dir, registry) = fixture().await;
    let mut history = Vec::new();
    expose(&registry, &mut history, "read", "sample.md", 1, 4).await;
    let args = patch("sample.md", "alpha", "one\n+two");
    let output = registry.execute_tool(tools::APPLY_PATCH, args.clone()).await.unwrap();
    assert!(output.get("error").is_none(), "{output}");
    history.extend(pair("edit", tools::APPLY_PATCH, &args, &output));
    registry.reconcile_source_inspection(&history);
    history.truncate(2);
    registry.reconcile_source_inspection(&history);
    let result = registry
        .execute_tool(tools::APPLY_PATCH, patch("sample.md", "delta", "bad"))
        .await
        .unwrap();
    assert!(result.to_string().contains("source_inspection_required"));
    assert!(
        std::fs::read_to_string(dir.path().join("sample.md"))
            .unwrap()
            .ends_with("delta\n")
    );
}

#[tokio::test]
async fn source_inspection_deterministic_valid_and_blind_traces() {
    for (scenario, mode, blind) in [
        ("baseline_valid", InspectionMode::Off, false),
        ("enforced_valid", InspectionMode::Enforce, false),
        ("enforced_blind", InspectionMode::Enforce, true),
    ] {
        let (_dir, registry) = fixture().await;
        registry.set_inspection_mode(mode);
        let started = std::time::Instant::now();
        let mut rejected = 0;
        if blind {
            let result = registry
                .execute_tool(tools::APPLY_PATCH, patch("sample.md", "beta", "blind"))
                .await
                .unwrap();
            assert!(result.to_string().contains("source_inspection_required"));
            rejected += 1;
        }
        let mut history = Vec::new();
        expose(&registry, &mut history, "read", "sample.md", 2, 1).await;
        let source_tokens = Message::estimate_tokens(&history[1]);
        for (id, old, new) in [("first", "beta", "authored"), ("second", "authored", "finished")] {
            let args = patch("sample.md", old, new);
            let result = registry.execute_tool(tools::APPLY_PATCH, args.clone()).await.unwrap();
            assert!(result.get("error").is_none(), "{scenario}: {result}");
            history.extend(pair(id, tools::APPLY_PATCH, &args, &result));
            registry.reconcile_source_inspection(&history);
        }
        eprintln!(
            "{}",
            json!({"scenario":scenario,"reads":1,"source_result_tokens_estimate":source_tokens,"elapsed_us":started.elapsed().as_micros(),"expected_blind_rejections":rejected,"false_rejections":0})
        );
    }
}

#[test]
fn source_inspection_call_identity_normalizes_handler_routing() {
    let raw = json!({"cmd":"sed -n '2,3p' source.rs", "max_output_tokens":1000});
    let normalized = crate::tools::command_args::normalize_shell_args(&raw).unwrap();
    assert_eq!(call_key(tools::EXEC_COMMAND, &raw), call_key(tools::EXEC_COMMAND, &normalized));
    let patch = "*** Begin Patch\n*** Add File: new.txt\n+source\n*** End Patch";
    assert_eq!(
        call_key(tools::APPLY_PATCH, &json!({"input":patch})),
        call_key(tools::APPLY_PATCH, &json!({"patch":patch}))
    );
}

#[tokio::test]
async fn source_inspection_retry_projection_revokes_only_affected_tool_pairs() {
    let (dir, registry) = fixture().await;
    std::fs::write(dir.path().join("other.md"), "other\n").unwrap();
    let mut history = Vec::new();
    expose(&registry, &mut history, "first", "sample.md", 1, 1).await;
    expose(&registry, &mut history, "second", "other.md", 1, 1).await;
    let before = history[2..].to_vec();
    let after = [Message::user("Summary: the second file was read".into())];
    registry.reconcile_source_projection_change(&before, &after);
    let inspection = registry.edited_file_monitor_ref().source_inspection();
    let first = vtcode_commons::canonicalize(dir.path().join("sample.md")).unwrap();
    let second = vtcode_commons::canonicalize(dir.path().join("other.md")).unwrap();
    assert!(
        inspection
            .require(&first, "alpha\nbeta\ngamma\ndelta\n", std::slice::from_ref(&(0..1)))
            .is_ok()
    );
    assert!(inspection.require_full(&second, "other\n").is_err());
}

#[tokio::test]
async fn source_inspection_positioned_search_counts_only_complete_returned_lines() {
    let (_dir, registry) = fixture().await;
    let args = json!({"query":"gamma"});
    let output = json!({"results":[{"path":"sample.md","line":3,"snippet":"gamma"},{"path":"sample.md","line":4,"snippet":"delt..."}]});
    registry.observe_source_output(tools::CODE_SEARCH, &args, &output).await;
    registry.reconcile_source_inspection(&pair("search", tools::CODE_SEARCH, &args, &output));
    let allowed = registry
        .execute_tool(tools::APPLY_PATCH, patch("sample.md", "gamma", "found"))
        .await
        .unwrap();
    assert!(allowed.get("error").is_none(), "{allowed}");
    let denied = registry
        .execute_tool(tools::APPLY_PATCH, patch("sample.md", "delta", "unseen"))
        .await
        .unwrap();
    assert!(denied.to_string().contains("source_inspection_required"), "{denied}");
}

#[tokio::test]
async fn source_inspection_pure_move_maps_partial_coverage_without_crediting_unseen_source() {
    let (_dir, registry) = fixture().await;
    let mut history = Vec::new();
    expose(&registry, &mut history, "read", "sample.md", 1, 1).await;
    let args = json!({"path":"sample.md","destination":"moved.md"});
    let output = registry.file_ops_tool().move_file(args.clone()).await.unwrap();
    assert!(output.get("error").is_none(), "{output}");
    history.extend(pair("move", tools::MOVE_FILE, &args, &output));
    registry.reconcile_source_inspection(&history);
    let denied = registry
        .execute_tool(tools::APPLY_PATCH, patch("moved.md", "beta", "unseen"))
        .await
        .unwrap();
    assert!(denied.to_string().contains("source_inspection_required"), "{denied}");
    let allowed = registry
        .execute_tool(tools::APPLY_PATCH, patch("moved.md", "alpha", "seen"))
        .await
        .unwrap();
    assert!(allowed.get("error").is_none(), "{allowed}");
}

#[tokio::test]
async fn source_inspection_trusted_user_command_uses_normal_dispatch_and_cannot_be_reused() {
    let (dir, registry) = fixture().await;
    let args = json!({"cmd":"printf generated > generated.txt", "yield_time_ms":1000});
    let denied = registry.execute_tool(tools::EXEC_COMMAND, args.clone()).await.unwrap();
    assert!(denied.to_string().contains("source_inspection_shell_write"), "{denied}");
    assert!(!dir.path().join("generated.txt").exists());
    {
        let _grant = registry.trusted_user_command_exemption(&args).unwrap();
        let output = registry.execute_tool(tools::EXEC_COMMAND, args.clone()).await.unwrap();
        assert_eq!(output["exit_code"], 0, "{output}");
    }
    assert_eq!(std::fs::read_to_string(dir.path().join("generated.txt")).unwrap(), "generated");
    let denied = registry.execute_tool(tools::EXEC_COMMAND, args).await.unwrap();
    assert!(denied.to_string().contains("source_inspection_shell_write"), "{denied}");
}

#[tokio::test]
async fn source_inspection_empty_existing_file_needs_an_actual_empty_read() {
    let (dir, registry) = fixture().await;
    std::fs::write(dir.path().join("empty.md"), "").unwrap();
    let args = json!({"input":"*** Begin Patch\n*** Update File: empty.md\n@@\n+authored\n*** End Patch"});
    let denied = registry.execute_tool(tools::APPLY_PATCH, args.clone()).await.unwrap();
    assert!(denied.to_string().contains("source_inspection_required"), "{denied}");
    let mut history = Vec::new();
    expose(&registry, &mut history, "empty", "empty.md", 1, 1).await;
    let output = registry.execute_tool(tools::APPLY_PATCH, args).await.unwrap();
    assert!(output.get("error").is_none(), "{output}");
    // Empty originals have no trailing newline; a hunk without an EOF marker preserves that policy.
    assert_eq!(std::fs::read_to_string(dir.path().join("empty.md")).unwrap(), "authored");
}

#[test]
fn source_inspection_transfer_metadata_remains_bounded() {
    let inspection = SourceInspection::default();
    inspection.set_mode(InspectionMode::Enforce);
    let mut source = PathBuf::from("/file-0");
    inspection.insert_lines(source.clone(), "source\n", vec![0], "read".into(), false);
    for index in 1..=MAX_FILES * 2 {
        let destination = PathBuf::from(format!("/file-{index}"));
        inspection.record_transfer(&source, &destination, "source\n", format!("move-{index}"), true);
        source = destination;
    }
    let ledger = inspection.0.lock();
    assert_eq!(ledger.files.len(), 1);
    assert_eq!(ledger.order.len(), 1);
    assert_eq!(ledger.order.front(), Some(&source));
}

#[tokio::test]
async fn source_inspection_legacy_paging_uses_zero_based_positions_with_repeated_text() {
    let (dir, registry) = fixture().await;
    std::fs::write(dir.path().join("sample.md"), "same\nsame\nend\n").unwrap();
    let args = json!({"path":"sample.md","offset_lines":1,"page_size_lines":1});
    let output = registry.execute_tool(tools::READ_FILE, args.clone()).await.unwrap();
    assert!(output.get("error").is_none(), "{output}");
    registry.reconcile_source_inspection(&pair("legacy", tools::READ_FILE, &args, &output));
    let denied = registry
        .execute_tool(tools::APPLY_PATCH, patch("sample.md", "same", "blind-first"))
        .await
        .unwrap();
    assert!(denied.to_string().contains("source_inspection_required"), "{denied}");
    let args = json!({"input":"*** Begin Patch\n*** Update File: sample.md\n@@ -2,1 +2,1 @@\n-same\n+inspected-second\n*** End Patch"});
    let output = registry.execute_tool(tools::APPLY_PATCH, args).await.unwrap();
    assert!(output.get("error").is_none(), "{output}");
    assert_eq!(std::fs::read_to_string(dir.path().join("sample.md")).unwrap(), "same\ninspected-second\nend\n");
}

#[tokio::test]
async fn source_inspection_append_needs_only_the_tail_and_never_credits_unseen_prefix() {
    for trailing_newline in [true, false] {
        let (dir, registry) = fixture().await;
        let before = if trailing_newline {
            "unseen\ntail\n"
        } else {
            "unseen\ntail"
        };
        std::fs::write(dir.path().join("sample.md"), before).unwrap();
        let mut history = Vec::new();
        expose(&registry, &mut history, "tail", "sample.md", 2, 1).await;
        let args = json!({"path":"sample.md","mode":"append","content":"authored\n"});
        let output = registry.execute_tool(tools::WRITE_FILE, args.clone()).await.unwrap();
        assert!(output.get("error").is_none(), "{output}");
        assert_eq!(std::fs::read_to_string(dir.path().join("sample.md")).unwrap(), format!("{before}authored\n"));
        history.extend(pair("append", tools::WRITE_FILE, &args, &output));
        registry.reconcile_source_inspection(&history);
        let denied = registry
            .execute_tool(tools::APPLY_PATCH, patch("sample.md", "unseen", "bad"))
            .await
            .unwrap();
        assert!(denied.to_string().contains("source_inspection_required"), "{denied}");
        let authored = if trailing_newline { "authored" } else { "tailauthored" };
        let allowed = registry
            .execute_tool(tools::APPLY_PATCH, patch("sample.md", authored, "edited"))
            .await
            .unwrap();
        assert!(allowed.get("error").is_none(), "{allowed}");
    }
}

#[tokio::test]
async fn source_inspection_copy_requires_destination_coverage_and_preserves_source_coverage() {
    let (dir, registry) = fixture().await;
    std::fs::write(dir.path().join("destination.md"), "destination-only\n").unwrap();
    let overwrite = json!({"path":"sample.md","destination":"destination.md"});
    assert!(registry.file_ops_tool().copy_file(overwrite.clone()).await.is_err());
    assert_eq!(std::fs::read_to_string(dir.path().join("destination.md")).unwrap(), "destination-only\n");
    let mut history = Vec::new();
    expose(&registry, &mut history, "destination", "destination.md", 1, 1).await;
    let copied = registry.file_ops_tool().copy_file(overwrite.clone()).await.unwrap();
    history.extend(pair("overwrite", tools::COPY_FILE, &overwrite, &copied));
    registry.reconcile_source_inspection(&history);
    let denied = registry
        .execute_tool(tools::APPLY_PATCH, patch("destination.md", "alpha", "unknown-copy"))
        .await
        .unwrap();
    assert!(denied.to_string().contains("source_inspection_required"), "{denied}");
    expose(&registry, &mut history, "source", "sample.md", 1, 1).await;
    let copy = json!({"path":"sample.md","destination":"copied.md"});
    let copied = registry.file_ops_tool().copy_file(copy.clone()).await.unwrap();
    history.extend(pair("copy", tools::COPY_FILE, &copy, &copied));
    registry.reconcile_source_inspection(&history);
    let denied = registry
        .execute_tool(tools::APPLY_PATCH, patch("copied.md", "beta", "unknown"))
        .await
        .unwrap();
    assert!(denied.to_string().contains("source_inspection_required"), "{denied}");
    for path in ["sample.md", "copied.md"] {
        let allowed = registry
            .execute_tool(tools::APPLY_PATCH, patch(path, "alpha", "known"))
            .await
            .unwrap();
        assert!(allowed.get("error").is_none(), "{path}: {allowed}");
    }
    assert!(
        registry
            .file_ops_tool()
            .copy_file(json!({"path":"sample.md","destination":"sample.md"}))
            .await
            .is_err()
    );
    assert!(
        std::fs::read_to_string(dir.path().join("sample.md"))
            .unwrap()
            .starts_with("known\n")
    );
}

#[tokio::test]
async fn source_inspection_literal_whitespace_edit_needs_only_selected_lines() {
    let (dir, registry) = fixture().await;
    std::fs::write(dir.path().join("sample.md"), "unseen prefix\r\n  β old \r\nunseen tail").unwrap();
    let mut history = Vec::new();
    expose(&registry, &mut history, "selected", "sample.md", 2, 1).await;
    let args = json!({"path":"sample.md", "old_str":"β old\t", "new_str":"β new"});
    let result = registry.execute_tool(tools::EDIT_FILE, args.clone()).await.unwrap();
    assert!(result.get("error").is_none(), "{result}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("sample.md")).unwrap(),
        "unseen prefix\r\nβ new\r\nunseen tail"
    );
    history.extend(pair("edit", tools::EDIT_FILE, &args, &result));
    registry.reconcile_source_inspection(&history);
    let followup = registry
        .edit_file(json!({"path":"sample.md", "old_str":"β new", "new_str":"done"}))
        .await
        .unwrap();
    assert!(followup.get("error").is_none(), "{followup}");
    let blind = registry
        .edit_file(json!({"path":"sample.md", "old_str":"unseen tail", "new_str":"bad"}))
        .await
        .unwrap_err();
    assert!(blind.to_string().contains("source_inspection_required"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("sample.md")).unwrap(),
        "unseen prefix\r\ndone\r\nunseen tail"
    );
}

#[tokio::test]
async fn source_inspection_default_warn_allows_edits_after_clearing_and_resume() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("doc.md"), "alpha\nbeta\n").unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    registry.allow_all_tools().await.unwrap();
    assert_eq!(registry.inspection_mode(), InspectionMode::Warn);
    let mut history = Vec::new();
    expose(&registry, &mut history, "read", "doc.md", 1, 1).await;
    registry.reconcile_source_inspection(&[Message::user("Compacted summary".into())]);
    let result = registry
        .execute_tool(tools::APPLY_PATCH, patch("doc.md", "beta", "updated"))
        .await
        .unwrap();
    assert!(result.get("error").is_none(), "{result}");
    let resumed = ToolRegistry::new(dir.path().to_path_buf()).await;
    resumed.allow_all_tools().await.unwrap();
    let result = resumed
        .execute_tool(tools::EDIT_FILE, json!({"path":"doc.md", "old_str":"alpha", "new_str":"first"}))
        .await
        .unwrap();
    assert!(result.get("error").is_none(), "{result}");
    assert_eq!(std::fs::read_to_string(dir.path().join("doc.md")).unwrap(), "first\nupdated\n");
    let inspection = SourceInspection::default();
    assert!(inspection.check_shell(&json!({"cmd":"cargo fmt --all"}), dir.path()).is_ok());
}

#[tokio::test]
async fn source_inspection_literal_policy_replay_reports_bounded_trace_metrics() {
    // Replay the pre-change fallback's full-file requirement, rather than claiming
    // that this fixture is a live provider or historical release-binary run.
    let source = (1..=500)
        .map(|line| {
            if line == 257 {
                "  selected source \n".to_owned()
            } else {
                format!("unrelated-{line:03}\n")
            }
        })
        .collect::<String>();
    for (scenario, mode, before_policy) in [
        ("before_full_file_fallback_policy_replay", InspectionMode::Enforce, true),
        ("after_localized_enforce", InspectionMode::Enforce, false),
        ("after_default_warn", InspectionMode::Warn, false),
    ] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("sample.md"), &source).unwrap();
        let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
        registry.allow_all_tools().await.unwrap();
        registry.set_inspection_mode(mode);
        let canonical = vtcode_commons::canonicalize(dir.path().join("sample.md")).unwrap();
        let start = std::time::Instant::now();
        let mut history = Vec::new();
        expose(&registry, &mut history, "selected", "sample.md", 257, 1).await;
        let mut false_rejections = 0;
        let mut refresh_calls = 0;
        if before_policy {
            let error = registry
                .edited_file_monitor_ref()
                .source_inspection()
                .require_full(&canonical, &source)
                .unwrap_err();
            assert!(error.to_string().contains("source_inspection_required"));
            false_rejections += 1;
            for (id, offset) in [("page1", 1), ("page2", 201), ("page3", 401)] {
                expose(&registry, &mut history, id, "sample.md", offset, 200).await;
                refresh_calls += 1;
            }
            assert!(
                registry
                    .edited_file_monitor_ref()
                    .source_inspection()
                    .require_full(&canonical, &source)
                    .is_ok()
            );
        }
        let result = registry
            .execute_tool(
                tools::EDIT_FILE,
                json!({"path":"sample.md", "old_str":"selected source\t", "new_str":"corrected source"}),
            )
            .await
            .unwrap();
        assert!(result.get("error").is_none(), "{scenario}: {result}");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("sample.md")).unwrap(),
            source.replace("  selected source ", "corrected source")
        );
        let returned_source_bytes = history
            .iter()
            .filter_map(|message| {
                if message.role != crate::llm::provider::MessageRole::Tool {
                    return None;
                }
                let output: Value = serde_json::from_str(&message.content.as_text()).unwrap();
                Some(output["content"].as_str().unwrap().len())
            })
            .sum::<usize>();
        let estimated_request_tokens = history.iter().map(Message::estimate_tokens).sum::<usize>();
        eprintln!(
            "{}",
            json!({"scenario":scenario, "provenance":"deterministic local policy replay; no provider calls",
            "reads":history.len()/2, "returned_source_bytes":returned_source_bytes, "estimated_request_tokens":estimated_request_tokens,
            "refresh_calls":refresh_calls, "false_rejections":false_rejections, "elapsed_us":start.elapsed().as_micros()})
        );
        if before_policy {
            assert_eq!(history.len() / 2, 4);
        } else {
            assert_eq!(history.len() / 2, 1);
        }
    }
}

#[tokio::test]
async fn source_inspection_literal_join_requires_suffix_before_crediting_joined_text() {
    let (dir, registry) = fixture().await;
    let mut history = Vec::new();
    expose(&registry, &mut history, "selected", "sample.md", 2, 1).await;
    let args = json!({"path":"sample.md", "old_str":"beta\n", "new_str":"joined-"});
    let denied = registry.edit_file(args.clone()).await.unwrap_err();
    assert!(denied.to_string().contains("source_inspection_required"));
    assert_eq!(std::fs::read_to_string(dir.path().join("sample.md")).unwrap(), "alpha\nbeta\ngamma\ndelta\n");
    expose(&registry, &mut history, "suffix", "sample.md", 3, 1).await;
    let result = registry.execute_tool(tools::EDIT_FILE, args.clone()).await.unwrap();
    assert!(result.get("error").is_none(), "{result}");
    assert_eq!(std::fs::read_to_string(dir.path().join("sample.md")).unwrap(), "alpha\njoined-gamma\ndelta\n");
    history.extend(pair("join", tools::EDIT_FILE, &args, &result));
    registry.reconcile_source_inspection(&history);
    let corrected = registry
        .edit_file(json!({"path":"sample.md", "old_str":"joined-gamma", "new_str":"finished"}))
        .await
        .unwrap();
    assert!(corrected.get("error").is_none(), "{corrected}");
    assert_eq!(std::fs::read_to_string(dir.path().join("sample.md")).unwrap(), "alpha\nfinished\ndelta\n");
    assert!(
        registry
            .edit_file(json!({"path":"sample.md", "old_str":"delta", "new_str":"unseen"}))
            .await
            .is_err()
    );
}

mod regressions;
