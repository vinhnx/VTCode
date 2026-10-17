use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::runtime::AgentRuntime;
use crate::core::agent::session::AgentSessionState;
use crate::core::agent::task::TaskOutcome;

#[tokio::test]
async fn headless_verification_waits_for_yielded_terminal_pass_and_failure() {
    use super::super::tests::make_runner;
    use crate::config::loader::VTCodeConfig;
    use tempfile::TempDir;
    for exit_code in [0, 7] {
        let temp = TempDir::new().unwrap();
        let mut cfg = VTCodeConfig::default();
        cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::Single;
        cfg.agent.harness.inspection.mode = vtcode_config::core::agent::InspectionMode::Warn;
        cfg.commands.allow_list.push("./rustc".into());
        let script = temp.path().join("rustc");
        std::fs::write(&script, format!("#!/bin/sh\nsleep 1.2\nprintf 'terminal-evidence'\nexit {exit_code}\n"))
            .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let runner = Box::pin(make_runner(&temp, cfg, "yielded-verifier")).await;
        let command = "./rustc --version".to_string();
        let mut recorder = ExecEventRecorder::new("verification", None, None);
        let mut runtime = AgentRuntime::new(AgentSessionState::new("verification".into(), 3, 3, 10000), None, None);
        let results = runner
            .run_verification_commands(&[command], &mut recorder, &mut runtime)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].success, exit_code == 0, "{results:?}");
        assert_eq!(results[0].exit_code, Some(exit_code));
        assert!(results[0].output.contains("terminal-evidence"), "{:?}", results[0]);
        assert!(runner.tool_registry.in_progress_exec_sessions(8).await.is_empty());
    }
}
#[tokio::test]
async fn headless_verification_resumes_current_model_check_but_not_stale_checks() {
    use super::super::tests::make_runner;
    use crate::config::loader::VTCodeConfig;
    use std::os::unix::fs::PermissionsExt;
    for (exit_code, later_edit, chained) in [
        (0, false, false),
        (7, false, false),
        (0, true, false),
        (0, false, true),
        (7, false, true),
        (0, true, true),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut cfg = VTCodeConfig::default();
        cfg.commands.allow_list.push("./rustc".into());
        let script = temp.path().join("rustc");
        std::fs::write(&script, format!(
            "#!/bin/sh\nprintf 'launched\\n' >> launches\nwhile [ ! -f release ]; do sleep 0.02; done\nprintf terminal-evidence\nexit {exit_code}\n"
        )).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let runner = Box::pin(make_runner(&temp, cfg, "resumed-verifier")).await;
        runner.tool_registry.allow_all_tools().await.unwrap();
        runner.tool_registry.restore_completion_verification_debt();
        let command = "./rustc --version";
        let launched_command = if chained {
            "./rustc --version && printf 'chain-tail'"
        } else {
            command
        };
        let launched = runner
            .tool_registry
            .execute_tool(
                crate::config::constants::tools::EXEC_COMMAND,
                serde_json::json!({"cmd":launched_command,"background":true,"yield_time_ms":50}),
            )
            .await
            .unwrap();
        let session_id = crate::tools::command_args::session_id_text(&launched).expect("yielded check");
        assert_eq!(runner.tool_registry.pending_completion_verifier_session(command).as_deref(), Some(session_id));
        if later_edit {
            let edit = runner
                .tool_registry
                .execute_tool(
                    crate::config::constants::tools::APPLY_PATCH,
                    serde_json::json!({
                        "input":"*** Begin Patch\n*** Add File: later.rs\n+fn changed() {}\n*** End Patch"
                    }),
                )
                .await
                .unwrap();
            assert!(edit.get("error").is_none(), "{edit}");
            assert!(runner.tool_registry.pending_completion_verifier_session(command).is_none());
        }
        std::fs::write(temp.path().join("release"), "ready").unwrap();
        let mut runtime = AgentRuntime::new(AgentSessionState::new("verification".into(), 3, 3, 10000), None, None);
        let mut recorder = ExecEventRecorder::new("verification", None, None);
        let results = runner
            .run_verification_commands(&[command.into()], &mut recorder, &mut runtime)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].exit_code, Some(exit_code));
        assert_eq!(results[0].success, exit_code == 0);
        assert!(results[0].output.contains("terminal-evidence"), "{results:?}");
        let launch_count = std::fs::read_to_string(temp.path().join("launches")).unwrap().lines().count();
        assert_eq!(launch_count, if later_edit { 2 } else { 1 }, "only stale checks need a new launch");
        assert_eq!(runner.tool_registry.completion_verification_pending(), exit_code != 0);
        if later_edit {
            runner.tool_registry.terminate_harness_exec_session(session_id).await.unwrap();
            runner.tool_registry.close_harness_exec_session(session_id).await.unwrap();
        }
        assert!(runner.tool_registry.in_progress_exec_sessions(8).await.is_empty());
    }
}

#[tokio::test]
async fn headless_verification_cancellation_and_execution_limit_remain_unverified_and_cleanup() {
    use super::super::tests::make_runner;
    use crate::config::loader::VTCodeConfig;
    use crate::core::agent::steering::SteeringMessage;
    use std::os::unix::fs::PermissionsExt;
    for cancel in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let mut cfg = VTCodeConfig::default();
        cfg.commands.allow_list.push("./rustc".into());
        cfg.timeouts.long_running_command_ceiling_seconds = 15;
        let script = temp.path().join("rustc");
        std::fs::write(&script, "#!/bin/sh\nsleep 30\nprintf unexpected\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let runner = Box::pin(make_runner(&temp, cfg, "cancelled-verifier")).await;
        if cancel {
            let launched = runner
                .tool_registry
                .execute_harness_command_session(serde_json::json!({
                    "action":"run", "command":"./rustc --version", "background":true, "yield_time_ms":50
                }))
                .await
                .unwrap();
            assert!(crate::tools::command_args::session_id_text(&launched).is_some(), "{launched}");
            assert!(
                runner
                    .tool_registry
                    .pending_completion_verifier_session("./rustc --version")
                    .is_some()
            );
        }
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let registry = runner.tool_registry.clone();
        let stop = tokio::spawn(async move {
            if cancel {
                while registry.in_progress_exec_sessions(1).await.is_empty() {
                    tokio::task::yield_now().await;
                }
                sender.send(SteeringMessage::SteerStop).unwrap();
            }
        });
        let mut runtime =
            AgentRuntime::new(AgentSessionState::new("verification".into(), 3, 3, 10000), None, Some(receiver));
        let mut recorder = ExecEventRecorder::new("verification", None, None);
        let results = runner
            .run_verification_commands(&["./rustc --version".into(), "pwd".into()], &mut recorder, &mut runtime)
            .await
            .unwrap();
        stop.await.unwrap();
        assert_eq!(results.len(), 1, "stop on the first unsuccessful verifier");
        assert!(!results[0].success);
        assert_eq!(results[0].exit_code, None);
        assert!(results[0].output.contains(if cancel { "cancelled" } else { "execution limit" }), "{results:?}");
        assert!(runner.tool_registry.in_progress_exec_sessions(8).await.is_empty());
        if cancel {
            assert_eq!(runtime.state.outcome, TaskOutcome::Cancelled);
        }
    }
}
