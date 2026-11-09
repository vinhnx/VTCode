use serde_json::json;

use super::response_meta::attach_spool_metadata;
use super::{ResolvedExecSession, update_exec_capture_output};
use crate::tools::exec_session::PipeOutputStats;
use crate::tools::registry::ExecSettlementMode;
use crate::tools::types::VTCodeExecSession;

#[tokio::test]
#[cfg(unix)]
async fn pipe_environment_uses_configured_search_paths_and_overrides() -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let workspace = tempfile::tempdir()?;
    let bin = workspace.path().join("bin");
    std::fs::create_dir(&bin)?;
    let executable = bin.join("python3");
    std::fs::write(&executable, "#!/bin/sh\nprintf path-probe-ok\n")?;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))?;
    let override_bin = workspace.path().join("override-bin");
    std::fs::create_dir(&override_bin)?;
    let override_executable = override_bin.join("python3");
    std::fs::write(&override_executable, "#!/bin/sh\nprintf override-probe-ok\n")?;
    std::fs::set_permissions(&override_executable, std::fs::Permissions::from_mode(0o755))?;
    let registry = super::ToolRegistry::new(workspace.path().to_path_buf()).await;
    let mut config = registry.commands_config();
    config.extra_path_entries = vec![bin.to_string_lossy().into_owned()];
    registry.apply_commands_config(&config);

    let env = registry.build_pipe_session_env("/bin/sh", hashbrown::HashMap::new());
    let path = env.get("PATH").expect("pipe PATH");
    let paths: Vec<_> = std::env::split_paths(path).collect();
    assert_eq!(paths.first(), Some(&bin));
    let mut overrides = hashbrown::HashMap::new();
    overrides.insert("PATH".to_string(), override_bin.to_string_lossy().into_owned());
    assert_eq!(registry.build_pipe_session_env("/bin/sh", overrides)["PATH"], override_bin.to_string_lossy());
    registry.allow_all_tools().await?;
    let result = registry
        .execute_tool(
            crate::config::constants::tools::EXEC_COMMAND,
            json!({"cmd": "python3", "shell": "/bin/sh", "yield_time_ms": 1000}),
        )
        .await?;
    assert_eq!(result["exit_code"], 0, "{result}");
    assert_eq!(result["output"], "path-probe-ok");
    let overridden = registry
        .execute_tool(
            crate::config::constants::tools::EXEC_COMMAND,
            json!({"cmd": "python3", "shell": "/bin/sh", "env": {"PATH": override_bin}, "yield_time_ms": 1000}),
        )
        .await?;
    assert_eq!(overridden["exit_code"], 0, "{overridden}");
    assert_eq!(overridden["output"], "override-probe-ok");
    Ok(())
}

#[cfg(unix)]
struct PipePeekFixture {
    registry: super::ToolRegistry,
    _workspace: tempfile::TempDir,
    expected_bytes: u64,
}

#[cfg(unix)]
impl PipePeekFixture {
    async fn new(initial: &str) -> anyhow::Result<Self> {
        let workspace = tempfile::tempdir()?;
        let registry = super::ToolRegistry::new(workspace.path().to_path_buf()).await;
        registry
            .exec_sessions
            .create_pipe_session(
                "run-peek".into(),
                vec![
                    "/bin/sh".into(),
                    "-c".into(),
                    "printf '%s' \"$1\"; while IFS= read -r line; do printf '%s' \"$line\"; done".into(),
                    "peek-fixture".into(),
                    initial.into(),
                ],
                workspace.path().to_path_buf(),
                Default::default(),
            )
            .await?;
        let fixture = Self {
            registry,
            _workspace: workspace,
            expected_bytes: initial.len() as u64,
        };
        fixture.wait_for_bytes().await?;
        Ok(fixture)
    }

    async fn append(&mut self, text: &str) -> anyhow::Result<()> {
        self.registry
            .exec_sessions
            .send_input_to_session("run-peek", text.as_bytes(), true)
            .await?;
        self.expected_bytes += text.len() as u64;
        self.wait_for_bytes().await
    }

    async fn wait_for_bytes(&self) -> anyhow::Result<()> {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let stats = self.registry.exec_sessions.output_stats("run-peek").await?;
                if stats.is_some_and(|stats| stats.total_bytes >= self.expected_bytes) {
                    return Ok::<(), anyhow::Error>(());
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await??;
        Ok(())
    }
}

#[test]
fn exec_capture_replaces_peeks_and_appends_drains() {
    let mut output = "previous snapshot".to_string();
    update_exec_capture_output(&mut output, "latest snapshot", false);
    assert_eq!(output, "latest snapshot");
    update_exec_capture_output(&mut output, " and drained output", true);
    assert_eq!(output, "latest snapshot and drained output");
    update_exec_capture_output(&mut output, "", false);
    assert!(output.is_empty(), "a reset snapshot clears stale output");
}

#[tokio::test]
#[cfg(unix)]
async fn pipe_peek_preserves_sliding_unicode_snapshots() -> anyhow::Result<()> {
    let mut fixture = PipePeekFixture::new(&format!("{}{}", "h".repeat(8192), "€".repeat(2731))).await?;
    let mut previous = None;
    let first = fixture
        .registry
        .next_exec_session_output("run-peek", false, &mut previous)
        .await?
        .unwrap();
    assert!(
        fixture
            .registry
            .next_exec_session_output("run-peek", false, &mut previous)
            .await?
            .is_none()
    );
    fixture.append("aa€").await?;
    let updated = fixture
        .registry
        .next_exec_session_output("run-peek", false, &mut previous)
        .await?
        .unwrap();
    assert!(!updated.is_char_boundary(first.len()), "old byte cursor would split the final character");
    assert!(updated.ends_with("aa€"));
    let mut captured = first;
    update_exec_capture_output(&mut captured, &updated, false);
    assert_eq!(captured, updated, "peek capture must replace its previous snapshot");
    fixture.registry.close_exec_session("run-peek").await?;
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn pipe_peek_preserves_same_length_and_reset_snapshots() -> anyhow::Result<()> {
    let mut fixture = PipePeekFixture::new(&format!("{}{}", "x".repeat(8192), "y".repeat(8192))).await?;
    let mut previous = None;
    let first = fixture
        .registry
        .next_exec_session_output("run-peek", false, &mut previous)
        .await?
        .unwrap();
    fixture.append("FRESH").await?;
    let updated = fixture
        .registry
        .next_exec_session_output("run-peek", false, &mut previous)
        .await?
        .unwrap();
    assert_eq!(first.len(), updated.len());
    assert!(updated.ends_with("FRESH"));
    fixture.registry.read_exec_session_output("run-peek", true).await?;
    let cleared = fixture
        .registry
        .next_exec_session_output("run-peek", false, &mut previous)
        .await?
        .unwrap();
    assert!(cleared.is_empty(), "an empty snapshot after draining is still a change");
    fixture.append("RESET").await?;
    let reset = fixture
        .registry
        .next_exec_session_output("run-peek", false, &mut previous)
        .await?
        .unwrap();
    assert_eq!(reset, "RESET");
    let mut captured = updated;
    update_exec_capture_output(&mut captured, &cleared, false);
    assert!(captured.is_empty());
    update_exec_capture_output(&mut captured, &reset, false);
    assert_eq!(captured, "RESET");
    update_exec_capture_output(&mut captured, "-drained", true);
    assert_eq!(captured, "RESET-drained", "drains remain incremental");
    fixture.registry.close_exec_session("run-peek").await?;
    let missing = fixture
        .registry
        .next_exec_session_output("run-peek", false, &mut previous)
        .await
        .unwrap_err();
    let error = crate::tools::registry::ToolExecutionError::from_anyhow("write_stdin", &missing, 0, false, false, None);
    assert!(error.is_exec_session_not_found());
    Ok(())
}

#[test]
fn incomplete_spool_is_not_advertised_as_a_reusable_reference() {
    let stats = PipeOutputStats {
        total_bytes: 42,
        truncated: true,
        spool_path: ".vtcode/context/tool_outputs/run-1.txt".to_string(),
        spool_available: true,
        spool_complete: false,
        spool_integrity: None,
    };
    let mut response = json!({});

    attach_spool_metadata(&mut response, &stats, true, true, 32);

    assert_eq!(response["total_output_bytes"], 42);
    assert_eq!(response["output_truncated"], true);
    assert_eq!(response["spool_complete"], false);
    assert_eq!(response["spool_pending"], true);
    assert!(response.get("spool_path").is_none());
    assert!(response.get("output_spooled").is_none());
}

#[test]
fn active_partial_spool_is_advertised_as_a_readable_snapshot() {
    let stats = PipeOutputStats {
        total_bytes: 42,
        truncated: true,
        spool_path: ".vtcode/context/tool_outputs/run-1.txt".to_string(),
        spool_available: true,
        spool_complete: false,
        spool_integrity: None,
    };
    let mut response = json!({});

    attach_spool_metadata(&mut response, &stats, false, true, 32);

    assert_eq!(response["spool_path"], stats.spool_path);
    assert_eq!(response["output_spooled"], true);
    assert_eq!(response["spool_complete"], false);
    assert_eq!(response["spool_pending"], true);
    assert_eq!(response["spool_state"], "pending");
    assert!(response.get("spool_line_count").is_none());
}

#[test]
fn complete_spool_is_advertised_with_a_reference() {
    let stats = PipeOutputStats {
        total_bytes: 42,
        truncated: false,
        spool_path: ".vtcode/context/tool_outputs/run-1.txt".to_string(),
        spool_available: true,
        spool_complete: true,
        spool_integrity: Some(crate::tools::output_spooler::SpoolIntegrity {
            byte_count: 42,
            line_count: 3,
            sha256: "digest".to_string(),
        }),
    };
    let mut response = json!({});

    attach_spool_metadata(&mut response, &stats, true, true, 32);

    assert_eq!(response["spool_path"], stats.spool_path);
    assert_eq!(response["output_spooled"], true);
    assert_eq!(response["spool_complete"], true);
    assert_eq!(response["spool_state"], "completed");
    assert_eq!(response["spooled_bytes"], 42);
    assert_eq!(response["spool_line_count"], 3);
    assert_eq!(response["spool_sha256"], "digest");
    assert!(response.get("spool_pending").is_none());
}

#[test]
fn small_output_keeps_lossless_spool_internal() {
    let stats = PipeOutputStats {
        total_bytes: 42,
        truncated: false,
        spool_path: ".vtcode/context/tool_outputs/run-1.txt".to_string(),
        spool_available: true,
        spool_complete: true,
        spool_integrity: None,
    };
    let mut response = json!({});

    attach_spool_metadata(&mut response, &stats, true, true, 64);

    assert_eq!(response["total_output_bytes"], 42);
    assert_eq!(response["output_truncated"], false);
    assert!(response.get("spool_path").is_none());
    assert!(response.get("output_spooled").is_none());
    assert!(response.get("spool_complete").is_none());
    assert!(response.get("spool_pending").is_none());
}

#[test]
fn disabled_dynamic_spooling_keeps_diagnostics_without_spool_metadata() {
    let stats = PipeOutputStats {
        total_bytes: 8_192,
        truncated: true,
        spool_path: ".vtcode/context/tool_outputs/run-1.txt".to_string(),
        spool_available: true,
        spool_complete: true,
        spool_integrity: None,
    };
    let mut response = json!({});

    attach_spool_metadata(&mut response, &stats, true, false, 64);

    assert_eq!(response["total_output_bytes"], 8_192);
    assert_eq!(response["output_truncated"], true);
    assert!(response.get("spool_path").is_none());
    assert!(response.get("output_spooled").is_none());
    assert!(response.get("spool_pending").is_none());
}

#[test]
fn background_poll_does_not_settle_until_exit() {
    let session = ResolvedExecSession::new(VTCodeExecSession {
        id: "run-background".to_string().into(),
        backend: "pipe".to_string(),
        command: "sleep".to_string(),
        args: vec!["5".to_string()],
        working_dir: None,
        background: true,
        rows: None,
        cols: None,
        child_pid: None,
        started_at: None,
        lifecycle_state: None,
        exit_code: None,
    });

    assert!(!session.should_settle_noninteractive(ExecSettlementMode::SettleNonInteractive));
}

#[test]
fn response_cap_advertises_spool_even_below_threshold() {
    let stats = PipeOutputStats {
        total_bytes: 42,
        truncated: false,
        spool_path: ".vtcode/context/tool_outputs/run-1.txt".to_string(),
        spool_available: true,
        spool_complete: true,
        spool_integrity: Some(crate::tools::output_spooler::SpoolIntegrity {
            byte_count: 42,
            line_count: 3,
            sha256: "digest".to_string(),
        }),
    };
    let mut response = json!({"truncated": true});

    attach_spool_metadata(&mut response, &stats, true, true, 64);

    assert_eq!(response["spool_path"], stats.spool_path);
    assert_eq!(response["output_spooled"], true);
    assert_eq!(response["spool_complete"], true);
}

#[test]
fn query_cap_advertises_spool_even_below_threshold() {
    let stats = PipeOutputStats {
        total_bytes: 42,
        truncated: false,
        spool_path: ".vtcode/context/tool_outputs/run-1.txt".to_string(),
        spool_available: true,
        spool_complete: true,
        spool_integrity: Some(crate::tools::output_spooler::SpoolIntegrity {
            byte_count: 42,
            line_count: 3,
            sha256: "digest".to_string(),
        }),
    };
    let mut response = json!({"query_truncated": true});

    attach_spool_metadata(&mut response, &stats, true, true, 64);

    assert_eq!(response["spool_path"], stats.spool_path);
    assert_eq!(response["output_spooled"], true);
    assert_eq!(response["spool_complete"], true);
}
