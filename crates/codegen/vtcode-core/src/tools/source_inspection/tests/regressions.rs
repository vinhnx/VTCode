use super::*;

#[tokio::test]
async fn advisory_binary_operations_preserve_bytes_and_strict_rejection() {
    for mode in [InspectionMode::Warn, InspectionMode::Off, InspectionMode::Enforce] {
        let (dir, registry) = fixture().await;
        registry.set_inspection_mode(mode);
        let old_bytes = [0xff, 0x00, 0x82, 0x13];
        let new_bytes = [0xfe, 0x01, 0x7f];
        std::fs::write(dir.path().join("asset.bin"), old_bytes).unwrap();
        let result = registry.file_ops_tool().delete_file(json!({"path":"asset.bin"})).await;
        if mode == InspectionMode::Enforce {
            assert!(result.is_err());
            assert_eq!(std::fs::read(dir.path().join("asset.bin")).unwrap(), old_bytes);
        } else {
            assert_eq!(result.unwrap()["deleted"], true);
            assert!(!dir.path().join("asset.bin").exists());
        }
        for (operation, source, destination) in [("move", "move.bin", "moved.bin"), ("copy", "copy.bin", "copied.bin")]
        {
            std::fs::write(dir.path().join(source), new_bytes).unwrap();
            std::fs::write(dir.path().join(destination), old_bytes).unwrap();
            let args = json!({"path":source,"destination":destination,"force":true});
            let result = if operation == "move" {
                registry.file_ops_tool().move_file(args).await
            } else {
                registry.file_ops_tool().copy_file(args).await
            };
            if mode == InspectionMode::Enforce {
                assert!(result.is_err());
                assert_eq!(std::fs::read(dir.path().join(destination)).unwrap(), old_bytes);
                assert_eq!(std::fs::read(dir.path().join(source)).unwrap(), new_bytes);
            } else {
                assert_eq!(result.unwrap()["success"], true);
                assert_eq!(std::fs::read(dir.path().join(destination)).unwrap(), new_bytes);
                assert_eq!(dir.path().join(source).exists(), operation == "copy");
            }
        }
        std::fs::create_dir(dir.path().join("assets")).unwrap();
        std::fs::write(dir.path().join("assets/image.bin"), old_bytes).unwrap();
        let result = registry
            .file_ops_tool()
            .delete_file(json!({"path":"assets","recursive":true}))
            .await;
        if mode == InspectionMode::Enforce {
            assert!(result.is_err());
            assert_eq!(std::fs::read(dir.path().join("assets/image.bin")).unwrap(), old_bytes);
        } else {
            assert_eq!(result.unwrap()["deleted"], true);
            assert!(!dir.path().join("assets").exists());
        }
    }
}

#[tokio::test]
async fn absent_patch_deletion_is_skipped_without_rejecting_other_operations() {
    for mode in [InspectionMode::Warn, InspectionMode::Off, InspectionMode::Enforce] {
        let (dir, registry) = fixture().await;
        registry.set_inspection_mode(mode);
        let args = json!({"input":"*** Begin Patch\n*** Delete File: absent.bin\n*** Add File: new.txt\n+created\n*** End Patch"});
        let result = registry.execute_tool(tools::APPLY_PATCH, args).await.unwrap();
        assert!(result.get("error").is_none(), "{mode:?}: {result}");
        assert!(result["applied"].to_string().contains("skipped deletion"));
        assert_eq!(std::fs::read_to_string(dir.path().join("new.txt")).unwrap(), "created\n");
        assert!(!dir.path().join("absent.bin").exists());
    }
}

#[tokio::test]
async fn advisory_patch_delete_accepts_binary_but_strict_stays_closed() {
    for mode in [InspectionMode::Warn, InspectionMode::Enforce] {
        let (dir, registry) = fixture().await;
        registry.set_inspection_mode(mode);
        let bytes = [0xff, 0x01];
        std::fs::write(dir.path().join("binary.bin"), bytes).unwrap();
        let args = json!({"input":"*** Begin Patch\n*** Delete File: binary.bin\n*** End Patch"});
        let result = registry.execute_tool(tools::APPLY_PATCH, args).await.unwrap();
        if mode == InspectionMode::Warn {
            assert!(result.get("error").is_none(), "{result}");
            assert!(!dir.path().join("binary.bin").exists());
        } else {
            assert!(result.get("error").is_some(), "{result}");
            assert_eq!(std::fs::read(dir.path().join("binary.bin")).unwrap(), bytes);
        }
    }
}

#[tokio::test]
async fn cached_read_reexposes_only_current_source_under_a_new_retained_call() {
    for stale in [false, true] {
        let (dir, registry) = fixture().await;
        let args = json!({"path":"sample.md", "offset":1, "limit":1,"max_tokens":4096,"condense":false});
        let initial = registry.execute_tool(tools::READ_FILE, args.clone()).await.unwrap();
        registry.reconcile_source_inspection(&pair("original", tools::READ_FILE, &args, &initial));
        registry.reconcile_source_inspection(&[]);
        if stale {
            std::fs::write(dir.path().join("sample.md"), "external\nbeta\ngamma\ndelta\n").unwrap();
            registry
                .edited_file_monitor_ref()
                .accept_disk_version(&dir.path().join("sample.md"))
                .await
                .unwrap();
        }
        let reused = registry.execute_tool(tools::READ_FILE, args.clone()).await.unwrap();
        assert_eq!(reused["reused_recent_result"], true, "{reused}");
        assert_eq!(reused["content"], initial["content"]);
        registry.reconcile_source_inspection(&pair("reread", tools::READ_FILE, &args, &reused));
        let old = if stale { "external" } else { "alpha" };
        let result = registry
            .edit_file(json!({"path":"sample.md","old_str":old,"new_str":"edited"}))
            .await;
        if stale {
            assert!(result.unwrap_err().to_string().contains("source_inspection_required"));
            assert_eq!(
                std::fs::read_to_string(dir.path().join("sample.md")).unwrap(),
                "external\nbeta\ngamma\ndelta\n"
            );
        } else {
            assert!(result.unwrap().get("error").is_none());
            assert_eq!(std::fs::read_to_string(dir.path().join("sample.md")).unwrap(), "edited\nbeta\ngamma\ndelta\n");
        }
    }
}

#[tokio::test]
async fn clearing_mutation_results_revokes_authored_coverage_with_inputs_retained() {
    use crate::core::agent::state::clear_old_tool_results;
    for clear_inputs in [false, true] {
        let (dir, registry) = fixture().await;
        let create = json!({"input":"*** Begin Patch\n*** Add File: created.txt\n+authored\n*** End Patch"});
        let output = registry.execute_tool(tools::APPLY_PATCH, create.clone()).await.unwrap();
        assert!(output.get("error").is_none(), "{output}");
        let mut history = pair("create", tools::APPLY_PATCH, &create, &output).to_vec();
        expose(&registry, &mut history, "last-read", "sample.md", 1, 1).await;
        let projected = clear_old_tool_results(&history, 1, 1, 1, clear_inputs);
        assert!(projected[1].content.as_text().contains("\"cleared\":\"tool_result\""));
        if !clear_inputs {
            assert_eq!(projected[0].tool_calls, history[0].tool_calls);
        }
        registry.reconcile_source_inspection(&projected);
        let rejected = registry
            .edit_file(json!({"path":"created.txt","old_str":"authored","new_str":"bad"}))
            .await
            .unwrap_err();
        assert!(rejected.to_string().contains("source_inspection_required"));
        assert_eq!(std::fs::read_to_string(dir.path().join("created.txt")).unwrap(), "authored\n");
        let permitted = registry
            .edit_file(json!({"path":"sample.md","old_str":"alpha","new_str":"inspected"}))
            .await
            .unwrap();
        assert!(permitted.get("error").is_none(), "{permitted}");
        assert_eq!(std::fs::read_to_string(dir.path().join("sample.md")).unwrap(), "inspected\nbeta\ngamma\ndelta\n");
    }
}

#[tokio::test]
async fn patch_conflicts_precede_disk_matching_in_all_inspection_modes() {
    for mode in [InspectionMode::Warn, InspectionMode::Off, InspectionMode::Enforce] {
        let (dir, registry) = fixture().await;
        registry.set_inspection_mode(mode);
        let mut history = Vec::new();
        expose(&registry, &mut history, "read", "sample.md", 1, 1).await;
        std::fs::write(dir.path().join("sample.md"), "external-owner\nbeta\ngamma\ndelta\n").unwrap();
        let args = patch("sample.md", "alpha", "agent-change");
        let conflict = registry.execute_tool(tools::APPLY_PATCH, args).await.unwrap();
        assert_eq!(conflict["conflict_detected"], true, "{mode:?}: {conflict}");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("sample.md")).unwrap(),
            "external-owner\nbeta\ngamma\ndelta\n"
        );
        assert!(!conflict["intended_content"].is_null(), "tracked source provides a conflict preview: {conflict}");
        // Resolution still requires a fresh read and a corrected patch.
        expose(&registry, &mut history, "refresh", "sample.md", 1, 2).await;
        let corrected = registry
            .execute_tool(tools::APPLY_PATCH, patch("sample.md", "external-owner", "resolved"))
            .await
            .unwrap();
        assert!(corrected.get("error").is_none(), "{corrected}");
        assert_eq!(std::fs::read_to_string(dir.path().join("sample.md")).unwrap(), "resolved\nbeta\ngamma\ndelta\n");
    }
}

#[test]
fn strict_shell_inspection_rejects_mutation_hidden_behind_verifier_labels() {
    let inspection = SourceInspection::default();
    inspection.set_mode(InspectionMode::Enforce);
    for args in [
        json!({"cmd":"cargo check --locked > sample.md"}),
        json!({"cmd":"cargo check --locked 2> sample.md"}),
        json!({"cmd":"rustc --print=cfg=sample.md"}),
        json!({"cmd":"rustc --print cfg=sample.md"}),
        json!({"cmd":"rustc --emit=dep-info=sample.md input.rs"}),
        json!({"cmd":"rustc -osample.md input.rs"}),
        json!({"cmd":"python3 -m pytest --junitxml=sample.md"}),
        json!({"cmd":"uv run pytest --log-file sample.md"}),
        json!({"cmd":"python3 -c 'open(\"sample.md\",\"w\").write(\"blind\")' -m pytest"}),
        json!({"cmd":"python3 -c", "args":["open('sample.md','w').write('blind')", "-m", "pytest"]}),
        json!({"cmd":"python3 '-copen(\"sample.md\",\"w\").write(\"blind\")' -m pytest"}),
        json!({"cmd":"python3 script.py -m pytest"}),
        json!({"cmd":"uv run python3 -c 'open(\"sample.md\",\"w\").write(\"blind\")' pytest"}),
        json!({"cmd":"uv run sh -c 'printf blind > sample.md' pytest"}),
        json!({"cmd":"cargo check --locked && python3 -c 'open(\"sample.md\",\"w\").write(\"blind\")' -m pytest"}),
    ] {
        assert!(inspection.check_shell(&args, Path::new("/fixture")).is_err(), "{args}");
        inspection.set_mode(InspectionMode::Warn);
        assert!(inspection.check_shell(&args, Path::new("/fixture")).is_ok(), "advisory is not admission: {args}");
        inspection.set_mode(InspectionMode::Enforce);
    }
    for command in [
        "uv run python -m pytest",
        "uv run python3 -I -m pytest",
        "uv run pytest",
        "uv run --frozen ruff check",
        "uv run --locked pytest",
        "rustc --print=cfg",
        "python3 -m pytest -q",
        "ruff check .",
        "cargo check --locked",
        "cargo check --locked 2>&1",
        "python3 -m pytest",
        "python3 -I -B -m pytest",
        "rg -n 'literal > data' sample.md",
        "cargo check --locked && cargo nextest run --locked",
    ] {
        assert!(inspection.check_shell(&json!({"cmd":command}), Path::new("/fixture")).is_ok(), "{command}");
    }
}

#[tokio::test]
async fn strict_shell_redirect_is_rejected_before_the_file_is_opened() {
    let (dir, registry) = fixture().await;
    for command in ["cargo check --locked > sample.md", "rustc --print=cfg=sample.md"] {
        let result = registry
            .execute_tool(tools::EXEC_COMMAND, json!({"cmd":command, "yield_time_ms":1}))
            .await
            .unwrap();
        assert!(result.to_string().contains("source_inspection_shell_write"), "{result}");
        assert_eq!(std::fs::read_to_string(dir.path().join("sample.md")).unwrap(), "alpha\nbeta\ngamma\ndelta\n");
    }
}

#[tokio::test]
async fn native_transfer_snapshots_distinguish_agent_writes_from_external_changes() {
    for mode in [InspectionMode::Warn, InspectionMode::Off] {
        for moved in [false, true] {
            let (dir, registry) = fixture().await;
            registry.set_inspection_mode(mode);
            let target = dir.path().join("destination.md");
            std::fs::write(&target, "previous destination\n").unwrap();
            registry
                .file_ops_tool()
                .read_file(json!({"path":"destination.md","limit":1}))
                .await
                .unwrap();
            let args = json!({"path":"sample.md","destination":"destination.md","force":true});
            let result = if moved {
                registry.file_ops_tool().move_file(args).await
            } else {
                registry.file_ops_tool().copy_file(args).await
            };
            assert_eq!(result.unwrap()["success"], true);
            let edited = registry
                .execute_tool(tools::APPLY_PATCH, patch("destination.md", "alpha", "after-native-write"))
                .await
                .unwrap();
            assert!(edited.get("error").is_none() && edited.get("conflict_detected").is_none(), "{edited}");
            assert_eq!(std::fs::read_to_string(&target).unwrap(), "after-native-write\nbeta\ngamma\ndelta\n");
            std::fs::write(&target, "external-owner\nbeta\ngamma\ndelta\n").unwrap();
            let conflict = registry
                .execute_tool(tools::APPLY_PATCH, patch("destination.md", "after-native-write", "bad"))
                .await
                .unwrap();
            assert_eq!(conflict["conflict_detected"], true, "{conflict}");
            assert_eq!(std::fs::read_to_string(&target).unwrap(), "external-owner\nbeta\ngamma\ndelta\n");
            registry.edited_file_monitor_ref().accept_disk_version(&target).await.unwrap();
            let deleted = registry
                .file_ops_tool()
                .delete_file(json!({"path":"destination.md"}))
                .await
                .unwrap();
            assert_eq!(deleted["deleted"], true);
            let cleanup = registry
                .execute_tool(
                    tools::APPLY_PATCH,
                    json!({"input":"*** Begin Patch\n*** Delete File: destination.md\n*** End Patch"}),
                )
                .await
                .unwrap();
            assert!(cleanup.get("error").is_none() && cleanup.get("conflict_detected").is_none(), "{cleanup}");
            if moved {
                let cleanup = registry
                    .execute_tool(
                        tools::APPLY_PATCH,
                        json!({"input":"*** Begin Patch\n*** Delete File: sample.md\n*** End Patch"}),
                    )
                    .await
                    .unwrap();
                assert!(cleanup.get("error").is_none() && cleanup.get("conflict_detected").is_none(), "{cleanup}");
            }
        }
    }
}

#[tokio::test]
async fn directory_removal_and_move_refresh_only_their_tracked_descendants() {
    for moved in [false, true] {
        let (dir, registry) = fixture().await;
        registry.set_inspection_mode(InspectionMode::Warn);
        std::fs::create_dir(dir.path().join("folder")).unwrap();
        std::fs::create_dir(dir.path().join("folder-other")).unwrap();
        std::fs::write(dir.path().join("folder/a.txt"), "owned child\n").unwrap();
        std::fs::write(dir.path().join("folder-other/a.txt"), "sibling before\n").unwrap();
        for path in ["folder/a.txt", "folder-other/a.txt"] {
            registry
                .file_ops_tool()
                .read_file(json!({"path":path,"limit":1}))
                .await
                .unwrap();
        }
        std::fs::write(dir.path().join("folder-other/a.txt"), "sibling external\n").unwrap();
        let result = if moved {
            registry
                .file_ops_tool()
                .move_file(json!({"path":"folder","destination":"moved-folder"}))
                .await
        } else {
            registry
                .file_ops_tool()
                .delete_file(json!({"path":"folder","recursive":true}))
                .await
        };
        assert_eq!(result.unwrap()["success"], true);
        let recreated = registry
            .file_ops_tool()
            .write_file(json!({"path":"folder/a.txt","content":"recreated\n","mode":"fail_if_exists"}))
            .await
            .unwrap();
        assert!(recreated.get("conflict_detected").is_none(), "{recreated}");
        assert_eq!(std::fs::read_to_string(dir.path().join("folder/a.txt")).unwrap(), "recreated\n");
        if moved {
            std::fs::write(dir.path().join("moved-folder/a.txt"), "destination external\n").unwrap();
            let moved_conflict = registry
                .execute_tool(tools::APPLY_PATCH, patch("moved-folder/a.txt", "owned child", "bad"))
                .await
                .unwrap();
            assert_eq!(moved_conflict["conflict_detected"], true, "{moved_conflict}");
            assert_eq!(
                std::fs::read_to_string(dir.path().join("moved-folder/a.txt")).unwrap(),
                "destination external\n"
            );
        }
        let sibling_conflict = registry
            .execute_tool(tools::APPLY_PATCH, patch("folder-other/a.txt", "sibling before", "bad"))
            .await
            .unwrap();
        assert_eq!(sibling_conflict["conflict_detected"], true, "{sibling_conflict}");
        assert_eq!(std::fs::read_to_string(dir.path().join("folder-other/a.txt")).unwrap(), "sibling external\n");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn advisory_recursive_delete_unlinks_symlinks_without_following_them() {
    for mode in [InspectionMode::Warn, InspectionMode::Enforce] {
        let (dir, registry) = fixture().await;
        registry.set_inspection_mode(mode);
        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("owned.bin");
        std::fs::write(&outside_file, [0xff, 0x00, 0x81]).unwrap();
        std::fs::create_dir(dir.path().join("links")).unwrap();
        std::os::unix::fs::symlink(&outside_file, dir.path().join("links/outside")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("missing"), dir.path().join("links/dangling")).unwrap();
        let result = registry
            .file_ops_tool()
            .delete_file(json!({"path":"links","recursive":true}))
            .await;
        if mode == InspectionMode::Warn {
            assert_eq!(result.unwrap()["deleted"], true);
            assert!(!dir.path().join("links").exists());
        } else {
            assert!(result.is_err());
            assert!(std::fs::symlink_metadata(dir.path().join("links/dangling")).is_ok());
        }
        assert_eq!(std::fs::read(&outside_file).unwrap(), [0xff, 0x00, 0x81]);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn advisory_directory_copy_keeps_resolved_source_and_destination_in_workspace() {
    for source_escape in [false, true] {
        let (dir, registry) = fixture().await;
        registry.set_inspection_mode(InspectionMode::Warn);
        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("owned.txt");
        std::fs::write(&outside_file, "outside-owner\n").unwrap();
        std::fs::create_dir(dir.path().join("from")).unwrap();
        std::fs::create_dir(dir.path().join("to")).unwrap();
        if source_escape {
            std::os::unix::fs::symlink(&outside_file, dir.path().join("from/a.txt")).unwrap();
        } else {
            std::fs::write(dir.path().join("from/a.txt"), "authorized-source\n").unwrap();
            std::os::unix::fs::symlink(&outside_file, dir.path().join("to/a.txt")).unwrap();
        }
        let result = registry
            .file_ops_tool()
            .copy_file(json!({"path":"from","destination":"to","recursive":true}))
            .await;
        assert!(result.is_err(), "copy cannot escape via {}", if source_escape { "source" } else { "destination" });
        assert_eq!(std::fs::read_to_string(&outside_file).unwrap(), "outside-owner\n");
        if source_escape {
            assert!(!dir.path().join("to/a.txt").exists());
        }
    }
}

#[test]
fn verification_labels_follow_python_and_uv_execution_targets() {
    use crate::tools::tool_intent::{ShellActivity, classify_shell_activity};
    for command in [
        "python3 script.py -m pytest",
        "python3 -cprint(1) -m pytest",
        "uv run python3 -cprint(1) pytest",
        "uv run sh -c 'printf blind' pytest",
    ] {
        assert_eq!(
            classify_shell_activity(tools::EXEC_COMMAND, &json!({"cmd":command})),
            ShellActivity::Mutation,
            "{command}"
        );
    }
    for command in [
        "python3 -I -m pytest",
        "uv run pytest",
        "uv run python3 -m pytest",
        "uv run --locked ruff check",
    ] {
        assert_eq!(
            classify_shell_activity(tools::EXEC_COMMAND, &json!({"cmd":command})),
            ShellActivity::Verification,
            "{command}"
        );
    }
}
