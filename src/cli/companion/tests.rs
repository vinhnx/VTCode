use super::*;
use serde_json::json;
use tempfile::TempDir;

#[cfg(unix)]
#[path = "tests/process_cleanup.rs"]
mod process_cleanup;

fn input(prompt: &str) -> StartTaskInput {
    StartTaskInput {
        prompt: prompt.into(),
        mode: TaskMode::Plan,
        plan_task_id: None,
    }
}

fn snapshot() -> TaskSnapshot {
    TaskSnapshot {
        task_id: "test-task".into(),
        mode: TaskMode::Plan,
        state: TaskState::Running,
        session_id: None,
        final_response: None,
        changes: Vec::new(),
        unified_diff: String::new(),
        evidence_truncated: false,
        command_results: Vec::new(),
        verification: TaskVerification::Unverified,
        verification_command: None,
        verification_exit_code: None,
        progress: String::new(),
    }
}

fn event(value: serde_json::Value) -> ThreadEvent {
    serde_json::from_value(value).expect("canonical fixture")
}

fn completed() -> serde_json::Value {
    json!({"type":"thread.completed", "thread_id":"thread-1", "session_id":"session-1",
        "subtype":"success", "outcome_code":"completed", "result":"Plan: change the greeting and run its test",
        "usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":2},"num_turns":1})
}

fn verification(kind: &str, exit_code: Option<i32>) -> ThreadEvent {
    event(json!({"type":"item.completed", "item":{"id":"verify", "type":"harness",
        "event":kind,"command":"cargo check --locked","exit_code":exit_code}}))
}

fn command_invocation(id: &str, command: &str) -> ThreadEvent {
    event(json!({"type":"item.completed","item":{
        "id":id,"type":"tool_invocation","tool_name":"exec_command",
        "arguments":{"cmd":command},"tool_call_id":format!("native-{id}"),"status":"completed"
    }}))
}

fn command_output(id: &str, exit: Option<i32>) -> ThreadEvent {
    event(json!({"type":"item.completed","item":{
        "id":format!("{id}:output"),"type":"tool_output","call_id":id,
        "tool_call_id":format!("native-{id}"),"status":"completed","exit_code":exit,
        "output":"private stdout/stderr marker; VERIFY_OK; exit 0"
    }}))
}

#[test]
fn command_results_retain_smoke_checks_separately_from_harness_verification() {
    let mut projection = EventProjection::default();
    let mut task = snapshot();
    let atomic = "python3 -c 'import os; p=\"target/smoke.txt\"; expected=b\"smoke\\n\"\ntry:\n assert open(p,\"rb\").read()==expected\nfinally:\n os.unlink(p)'";
    for (id, command) in [
        ("atomic", atomic),
        ("absence", "test ! -e target/smoke.txt && test ! -L target/smoke.txt"),
    ] {
        projection.observe(&mut task, command_invocation(id, command));
        projection.observe(&mut task, command_output(id, Some(0)));
    }
    assert_eq!(task.verification, TaskVerification::Unverified);
    projection.observe(
        &mut task,
        event(json!({"type":"item.completed","item":{
            "id":"cargo","type":"command_execution","command":"cargo check --locked",
            "status":"completed","exit_code":0,"aggregated_output":"private stderr marker"
        }})),
    );
    projection.observe(&mut task, verification("verification_passed", Some(0)));
    assert_eq!(task.verification, TaskVerification::Verified);
    assert_eq!(task.verification_command.as_deref(), Some("cargo check --locked"));
    assert_eq!(task.command_results.len(), 3);
    assert_eq!(task.command_results[0].command.as_deref(), Some(atomic));
    assert_eq!(task.command_results[0].event_ref.item_id, "atomic:output");
    assert_eq!(task.command_results[0].exit_code, Some(0));
    assert_eq!(
        task.command_results[1].command.as_deref(),
        Some("test ! -e target/smoke.txt && test ! -L target/smoke.txt")
    );
    assert_eq!(task.command_results[2].source, "command_execution");
    assert!(!task.evidence_truncated);
    let serialized = serde_json::to_string(&task).unwrap();
    assert!(!serialized.contains("private stdout/stderr"));
    assert!(!serialized.contains("private stderr"));
    projection.observe(&mut task, command_invocation("later", "printf changed > source.rs"));
    assert_eq!(task.verification, TaskVerification::Unverified);
    assert_eq!(task.command_results.len(), 3);
    task.mark_incomplete();
    assert_eq!(task.command_results[0].exit_code, Some(0));
    assert!(task.evidence_truncated);
}

#[test]
fn command_results_do_not_infer_exit_codes_or_correlate_future_invocations() {
    let mut projection = EventProjection::default();
    let mut task = snapshot();
    projection.observe(&mut task, command_invocation("yield", "cargo check --locked"));
    projection.observe(&mut task, command_output("yield", None));
    projection.observe(&mut task, command_invocation("failed", "cmp unequal actual"));
    projection.observe(&mut task, command_output("failed", Some(7)));
    projection.observe(&mut task, command_output("future", Some(0)));
    projection.observe(&mut task, command_invocation("future", "echo fabricated"));
    projection.observe(
        &mut task,
        event(json!({"type":"item.completed","item":{
            "id":"prose","type":"agent_message","text":"All tests passed. exit_code=0; VERIFY_OK"
        }})),
    );
    assert_eq!(task.command_results.len(), 3);
    assert_eq!(task.command_results[0].exit_code, None);
    assert_eq!(task.command_results[1].exit_code, Some(7));
    assert!(task.command_results[2].command.is_none());
    assert!(task.evidence_truncated);
    assert_eq!(task.verification, TaskVerification::Unverified);
}

#[test]
fn command_results_fill_started_arguments_and_retain_distinct_reruns() {
    let mut projection = EventProjection::default();
    let mut task = snapshot();
    let mut started = serde_json::to_value(command_invocation("first", "pwd")).unwrap();
    started["type"] = json!("item.started");
    started["item"].as_object_mut().unwrap().remove("arguments");
    projection.observe(&mut task, event(started));
    let mut updated = serde_json::to_value(command_invocation("first", "pwd")).unwrap();
    updated["type"] = json!("item.updated");
    projection.observe(&mut task, event(updated));
    projection.observe(&mut task, command_invocation("first", "pwd"));
    projection.observe(&mut task, command_output("first", Some(0)));
    projection.observe(&mut task, command_output("first", Some(0)));
    projection.observe(&mut task, command_invocation("second", "pwd"));
    projection.observe(&mut task, command_output("second", Some(8)));
    assert_eq!(task.command_results.len(), 2);
    assert_eq!(task.command_results[0].command.as_deref(), Some("pwd"));
    assert_eq!(task.command_results[0].exit_code, Some(0));
    assert_eq!(task.command_results[1].exit_code, Some(8));
    assert!(!task.evidence_truncated);
    projection.observe(&mut task, command_invocation("conflict", "cmp a b"));
    projection.observe(&mut task, command_invocation("conflict", "rm owned"));
    projection.observe(&mut task, command_output("conflict", Some(0)));
    assert!(task.command_results[2].command.is_none());
    assert!(task.evidence_truncated);
}

#[test]
fn command_results_withhold_unknown_argv_and_cross_thread_matches() {
    let mut projection = EventProjection::default();
    let mut task = snapshot();
    projection.observe(&mut task, event(json!({"type":"thread.started","thread_id":"thread-a"})));
    projection.observe(&mut task, command_invocation("before", "pwd"));
    projection.observe(&mut task, event(json!({"type":"thread.started","thread_id":"thread-b"})));
    projection.observe(&mut task, command_output("before", Some(0)));
    assert!(task.command_results[0].command.is_none());
    assert_eq!(task.command_results[0].event_ref.thread_id.as_deref(), Some("thread-b"));
    let mut invocation = serde_json::to_value(command_invocation("argv", "check")).unwrap();
    invocation["item"]["arguments"]["args"] = json!(["--different-scope"]);
    projection.observe(&mut task, event(invocation));
    projection.observe(&mut task, command_output("argv", Some(0)));
    assert!(task.command_results[1].command_truncated);
    assert!(task.evidence_truncated);
}

#[test]
fn command_results_keep_native_item_correlation_scoped_to_actor_and_turn() {
    let mut projection = EventProjection::default();
    let mut task = snapshot();
    let context =
        |actor: &str, turn: &str| json!({"task_id":"runtime","actor_id":actor,"turn_id":turn,"timestamp":"now"});
    let mut invocation = serde_json::to_value(command_invocation("same", "cmp expected actual")).unwrap();
    invocation["item"]["context"] = context("actor-a", "turn-a");
    projection.observe(&mut task, event(invocation.clone()));
    let mut wrong = serde_json::to_value(command_output("same", Some(0))).unwrap();
    wrong["item"]["context"] = context("actor-b", "turn-b");
    projection.observe(&mut task, event(wrong));
    assert!(task.command_results[0].command.is_none());
    let mut correct = serde_json::to_value(command_output("same", Some(4))).unwrap();
    correct["item"]["context"] = context("actor-a", "turn-a");
    projection.observe(&mut task, event(correct.clone()));
    projection.observe(&mut task, event(correct));
    assert_eq!(task.command_results.len(), 2);
    assert_eq!(task.command_results[1].command.as_deref(), Some("cmp expected actual"));
    assert_eq!(task.command_results[1].exit_code, Some(4));
    assert_eq!(task.command_results[1].event_ref.actor_id.as_deref(), Some("actor-a"));
    invocation["item"]["context"] = context("actor-c", "turn-c");
    projection.observe(&mut task, event(invocation));
    let mut mismatch = serde_json::to_value(command_output("same", Some(0))).unwrap();
    mismatch["item"]["context"] = context("actor-c", "turn-c");
    mismatch["item"]["tool_call_id"] = json!("different-native-id");
    projection.observe(&mut task, event(mismatch));
    assert!(task.command_results[2].command.is_none());
}

#[test]
fn command_results_redact_and_bound_commands_records_and_pending_invocations() {
    let mut projection = EventProjection::default();
    let mut task = snapshot();
    for index in 0..40 {
        let id = format!("record-{index}");
        projection.observe(&mut task, command_invocation(&id, "pwd"));
        projection.observe(&mut task, command_output(&id, Some(0)));
    }
    assert_eq!(task.command_results.len(), 32);
    assert!(task.evidence_truncated);
    let mut projection = EventProjection::default();
    let mut task = snapshot();
    for index in 0..12 {
        let id = format!("large-{index}");
        projection
            .observe(&mut task, command_invocation(&id, &format!("password=1234567890123456 {}", "é".repeat(5000))));
        projection.observe(&mut task, command_output(&id, Some(0)));
    }
    assert!(!task.command_results.is_empty());
    assert!(task.command_results.len() < 12);
    assert!(
        task.command_results
            .iter()
            .all(|r| r.command_truncated && r.command.as_ref().unwrap().len() <= 4096)
    );
    let records = serde_json::to_string(&task.command_results).unwrap();
    assert!(records.len() <= 16 * 1024);
    assert!(!records.contains("1234567890123456"));
    assert!(task.evidence_truncated);
    let mut projection = EventProjection::default();
    let mut task = snapshot();
    for index in 0..33 {
        projection.observe(&mut task, command_invocation(&format!("pending-{index}"), "pwd"));
    }
    projection.observe(&mut task, command_output("pending-32", Some(0)));
    assert!(task.command_results[0].command.is_none());
    assert!(task.evidence_truncated);
}

#[test]
fn command_results_preserve_canonical_task_uuids_without_exempting_secret_values() {
    let mut projection = EventProjection::default();
    let mut task = snapshot();
    for (index, id) in ["task-fb236495-6968-44f1-9654-d3239a16fa50", "task-sk-1234567890123456"]
        .into_iter()
        .enumerate()
    {
        let call = format!("call-{index}");
        for input in [command_invocation(&call, "pwd"), command_output(&call, Some(0))] {
            let mut input = serde_json::to_value(input).unwrap();
            input["item"]["context"] =
                json!({"task_id":id,"actor_id":"actor","turn_id":"turn","timestamp":"2026-10-17T00:00:00Z"});
            projection.observe(&mut task, event(input));
        }
        assert_eq!(task.evidence_truncated, index != 0);
    }
    assert_eq!(
        task.command_results[0].event_ref.runtime_task_id.as_deref(),
        Some("task-fb236495-6968-44f1-9654-d3239a16fa50")
    );
    assert!(!serde_json::to_string(&task).unwrap().contains("1234567890123456"));
}

#[test]
fn command_results_flag_redaction_and_respect_the_exact_command_byte_boundary() {
    let mut projection = EventProjection::default();
    let mut task = snapshot();
    let boundary = "pwd\n".repeat(1024);
    projection.observe(&mut task, command_invocation("boundary", &boundary));
    projection.observe(&mut task, command_output("boundary", Some(0)));
    assert_eq!(task.command_results[0].command.as_deref(), Some(boundary.as_str()));
    assert!(!task.command_results[0].command_truncated);
    assert!(!task.evidence_truncated);
    projection.observe(&mut task, command_invocation("beyond", &format!("{boundary}x")));
    projection.observe(&mut task, command_output("beyond", Some(0)));
    assert_eq!(task.command_results[1].command.as_ref().unwrap().len(), 4096);
    assert!(task.command_results[1].command_truncated);
    let mut projection = EventProjection::default();
    let mut task = snapshot();
    projection.observe(&mut task, command_invocation("secret", "check password=1234567890123456"));
    projection.observe(&mut task, command_output("secret", Some(0)));
    assert!(task.command_results[0].command_truncated);
    assert!(task.evidence_truncated);
    assert!(!serde_json::to_string(&task).unwrap().contains("1234567890123456"));
}

#[test]
fn verification_requires_exit_zero_and_later_mutations_invalidate_it() {
    let mut projection = EventProjection::default();
    let mut task = snapshot();
    projection.observe(&mut task, verification("verification_passed", None));
    assert_eq!(task.verification, TaskVerification::Unverified);
    projection.observe(&mut task, verification("verification_passed", Some(7)));
    assert_eq!(task.verification, TaskVerification::Unverified);
    projection.observe(&mut task, verification("verification_failed", Some(7)));
    assert_eq!(task.verification, TaskVerification::Failed);
    projection.observe(&mut task, verification("verification_passed", Some(0)));
    assert_eq!(task.verification, TaskVerification::Verified);
    projection.observe(
        &mut task,
        event(json!({"type":"item.started", "item":{
        "id":"shell", "type":"command_execution", "command":"touch later.rs", "status":"in_progress"}})),
    );
    assert_eq!(task.verification, TaskVerification::Unverified);
    assert_eq!(task.verification_exit_code, None);
}

#[test]
fn canonical_projection_excludes_reasoning_and_bounds_redacted_evidence() {
    let mut projection = EventProjection::default();
    let mut task = snapshot();
    projection.observe(&mut task, event(completed()));
    assert_eq!(task.session_id.as_deref(), Some("session-1"));
    assert_eq!(task.verification, TaskVerification::Unverified);
    projection.observe(
        &mut task,
        event(json!({"type":"item.completed", "item":{
        "id":"change", "type":"file_change", "changes":[{"path":"src/greeting.rs","kind":"update"}],
        "status":"completed","unified_diff": format!("+password=1234567890123456\n{}", "é".repeat(MAX_TEXT_BYTES))}})),
    );
    assert_eq!(task.changes, ["src/greeting.rs"]);
    assert!(task.unified_diff.len() <= MAX_TEXT_BYTES);
    assert!(!task.unified_diff.contains("1234567890123456"));
    assert!(task.evidence_truncated);
    projection.observe(&mut task, event(json!({"type":"item.completed", "item":{
        "id":"failed-change", "type":"file_change", "changes":[{"path":"unrelated.rs","kind":"add"}], "status":"failed"}})));
    assert_eq!(task.changes, ["src/greeting.rs"]);
}

#[test]
fn companion_withholds_sensitive_and_escaping_change_evidence() {
    for path in [
        ".env",
        "config/.SSH/id_ed25519",
        "../outside.rs",
        "/absolute.rs",
        ".vtcode/sessions/private.json",
    ] {
        let mut projection = EventProjection::default();
        let mut task = snapshot();
        projection.observe(
            &mut task,
            event(json!({"type":"item.completed", "item":{
            "id":"change", "type":"file_change", "changes":[{"path":path,"kind":"update"}],
            "status":"completed","unified_diff":"+private material"}})),
        );
        assert!(task.changes.is_empty(), "{path}");
        assert!(task.unified_diff.is_empty(), "{path}");
        assert!(task.evidence_truncated);
    }
}

#[test]
fn companion_does_not_project_private_reasoning() {
    let mut projection = EventProjection::default();
    let mut task = snapshot();
    projection.observe(
        &mut task,
        event(json!({"type":"item.completed", "item":{
        "id":"reasoning", "type":"reasoning", "text":"private reasoning marker"}})),
    );
    assert!(!serde_json::to_string(&task).unwrap().contains("private reasoning marker"));
}

#[tokio::test]
async fn event_reader_rejects_oversized_and_partial_records() {
    let bytes = vec![b'x'; MAX_EVENT_BYTES + 1];
    assert!(read_event_line(&mut BufReader::new(bytes.as_slice())).await.is_err());
    assert!(read_event_line(&mut BufReader::new(&b"{}"[..])).await.is_err());
    let mut reader = BufReader::new(&b"{}\n[]\n"[..]);
    assert_eq!(read_event_line(&mut reader).await.unwrap().unwrap(), b"{}\n");
    assert_eq!(read_event_line(&mut reader).await.unwrap().unwrap(), b"[]\n");
    assert!(read_event_line(&mut reader).await.unwrap().is_none());
}

#[test]
fn launch_uses_fixed_argv_and_never_grants_trust_or_passes_proxy_credentials() {
    let service =
        CompanionService::new("/workspace".into(), "/bin/vtcode".into(), "TEST_PROXY_SECRET".into(), Vec::new());
    let malicious = "--workspace /tmp/escape $(touch injected); resume --last";
    let mut request = input(malicious);
    let command = service.command(&request, None);
    let command = command.as_std();
    let args: Vec<_> = command.get_args().map(|s| s.to_string_lossy().into_owned()).collect();
    assert_eq!(args, ["--workspace", "/workspace", "exec", "--json", "--dry-run",]);
    let env: BTreeMap<_, _> = command.get_envs().collect();
    assert_eq!(env.get(std::ffi::OsStr::new("TEST_PROXY_SECRET")), Some(&None));
    assert_eq!(env.get(std::ffi::OsStr::new("VTCODE_TRUST_WORKSPACE")), Some(&Some(std::ffi::OsStr::new("deny"))));
    request.mode = TaskMode::Implement;
    assert!(!service.command(&request, None).as_std().get_args().any(|s| s == "--dry-run"));
}

#[test]
fn companion_cli_requires_mcp_and_prompt_cannot_override_read_only_mode() {
    use clap::Parser;
    use vtcode_core::cli::args::{Cli, Commands};
    assert!(Cli::try_parse_from(["vtcode", "webmcp", "serve", "--companion"]).is_err());
    assert!(Cli::try_parse_from(["vtcode", "webmcp", "serve", "--mcp", "--companion"]).is_ok());
    let parsed = Cli::try_parse_from(["vtcode", "exec", "--json", "--dry-run"]).unwrap();
    assert!(matches!(parsed.command, Some(Commands::Exec { dry_run: true, prompt: None, .. })));
}

#[test]
fn companion_launch_preserves_selected_config_inline_policy_and_cli_denies() {
    use clap::Parser;
    use vtcode_core::cli::args::{Cli, Commands};

    let root = TempDir::new().unwrap();
    let config_path = root.path().join("selected policy.toml");
    let parent = Cli::try_parse_from([
        "vtcode",
        "--config",
        "relative-policy.toml",
        "--config",
        "sandbox.enabled=true",
        "--config",
        "permissions.deny=['write']",
        "--disallowed-tools",
        "exec_command,apply_patch",
        "--allowed-tools",
        "read_file",
        "--security-level",
        "strict",
        "--max-tool-calls",
        "3",
        "--api-rate-limit",
        "19",
        "--max-concurrent-ops",
        "2",
        "--api-key-env",
        "COMPANION_TEST_API_KEY",
        "--provider",
        "openai",
        "--model",
        "selected-model",
        "--agent",
        "duck",
        "--no-chrome",
        "--no-codex-experimental",
        "webmcp",
        "serve",
        "--mcp",
        "--companion",
    ])
    .unwrap();
    let service = CompanionService::new(
        root.path().to_path_buf(),
        "/bin/vtcode".into(),
        "TEST_PROXY_SECRET".into(),
        launch_args(&parent, Some(config_path.clone())),
    );
    let command = service.command(&input("--disallowed-tools ''"), None);
    let child = Cli::try_parse_from(
        std::iter::once(OsString::from("vtcode")).chain(command.as_std().get_args().map(std::ffi::OsStr::to_os_string)),
    )
    .unwrap();
    assert_eq!(
        child.config,
        [
            config_path.to_str().unwrap(),
            "sandbox.enabled=true",
            "permissions.deny=['write']"
        ]
    );
    assert_eq!(child.disallowed_tools, parent.disallowed_tools);
    assert_eq!(child.allowed_tools, parent.allowed_tools);
    assert_eq!(child.security_level, "strict");
    assert_eq!(child.max_tool_calls, 3);
    assert_eq!(child.api_rate_limit, 19);
    assert_eq!(child.max_concurrent_ops, 2);
    assert_eq!(child.api_key_env, "COMPANION_TEST_API_KEY");
    assert_eq!(child.provider.as_deref(), Some("openai"));
    assert_eq!(child.model.as_deref(), Some("selected-model"));
    assert_eq!(child.agent.as_deref(), Some("duck"));
    assert!(child.no_chrome);
    assert!(child.no_codex_experimental);
    assert!(matches!(child.command, Some(Commands::Exec { dry_run: true, prompt: None, .. })));
}

#[cfg(unix)]
fn fixture(script: &str) -> (TempDir, CompanionService) {
    use std::os::unix::fs::PermissionsExt;
    let root = TempDir::new().unwrap();
    let executable = root.path().join("fake-vtcode");
    std::fs::write(&executable, format!("#!/bin/sh\ncat > prompt.txt\n{script}\n")).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(root.path().join("fixture.jsonl"), format!("{}\n", completed())).unwrap();
    let service = CompanionService::new(root.path().to_owned(), executable, "TEST_PROXY_SECRET".into(), Vec::new());
    (root, service)
}

async fn terminal(service: &CompanionService, id: &str) -> TaskSnapshot {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let task = service.get(id).await.unwrap();
            if task.state != TaskState::Running {
                return task;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("supervisor completed")
}

#[tokio::test]
async fn companion_shutdown_reports_a_panicked_supervisor_without_polling_it_twice() {
    let service =
        CompanionService::new("/unused".into(), "/unused/vtcode".into(), "TEST_PROXY_SECRET".into(), Vec::new());
    let task = snapshot();
    let id = task.task_id.clone();
    let worker = tokio::spawn(async { panic!("supervisor failure fixture") });
    service.tasks.lock().await.insert(
        id.clone(),
        TaskRecord {
            snapshot: task,
            cancellation: CancellationToken::new(),
            worker: Some(worker),
        },
    );
    assert!(service.shutdown().await.is_err());
    assert_eq!(service.get(&id).await.unwrap().state, TaskState::Failed);
    assert!(service.start(input("plan")).await.unwrap_err().contains("shutting down"));
}

#[cfg(unix)]
#[tokio::test]
async fn companion_plan_then_implement_uses_real_child_and_keeps_verification_unconfirmed() {
    let (root, service) = fixture("printf '%s\\n' \"$@\" > argv.txt\ncat fixture.jsonl");
    let mut implement = input("implement the plan");
    implement.mode = TaskMode::Implement;
    assert!(service.start(implement.clone()).await.is_err());
    implement.plan_task_id = Some("../unowned".into());
    assert!(service.start(implement.clone()).await.is_err());
    let started = service.start(input("--workspace /tmp/outside; touch injected")).await.unwrap();
    let planned = terminal(&service, &started.task_id).await;
    assert_eq!(planned.state, TaskState::Completed);
    assert_eq!(planned.verification, TaskVerification::Unverified);
    let args = std::fs::read_to_string(root.path().join("argv.txt")).unwrap();
    assert!(args.contains("--dry-run\n"));
    assert!(!args.contains("/tmp/outside"));
    assert_eq!(
        std::fs::read_to_string(root.path().join("prompt.txt")).unwrap(),
        "--workspace /tmp/outside; touch injected"
    );
    assert!(!root.path().join("injected").exists());
    implement.plan_task_id = Some(started.task_id);
    let running = service.start(implement).await.unwrap();
    assert_eq!(terminal(&service, &running.task_id).await.state, TaskState::Completed);
    assert!(
        !std::fs::read_to_string(root.path().join("argv.txt"))
            .unwrap()
            .contains("--dry-run")
    );
    service.shutdown().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn companion_rejects_overlap_cancels_and_recovers_after_invalid_calls() {
    let (_root, service) = fixture("sleep 30\ncat fixture.jsonl");
    assert!(service.start(input("")).await.is_err());
    assert!(service.get("unknown").await.is_err());
    assert!(service.cancel("unknown").await.is_err());
    let running = service.start(input("plan")).await.unwrap();
    assert!(
        service
            .start(input("second task"))
            .await
            .unwrap_err()
            .contains("already running")
    );
    assert!(service.cancel(&running.task_id).await.unwrap());
    assert_eq!(terminal(&service, &running.task_id).await.state, TaskState::Cancelled);
    assert!(!service.cancel(&running.task_id).await.unwrap());
    let next = service.start(input("another plan")).await.unwrap();
    service.shutdown().await.unwrap();
    assert_eq!(service.get(&next.task_id).await.unwrap().state, TaskState::Cancelled);
}

#[cfg(unix)]
#[tokio::test]
async fn companion_exit_zero_without_canonical_completion_and_nonzero_with_completion_fail() {
    for (script, expected_reason) in [
        ("exit 0", "missing canonical thread completion"),
        ("cat fixture.jsonl\nexit 7", "exec exited with exit status: 7"),
        ("printf 'not json private payload\\n'", "invalid exec event JSON"),
    ] {
        let (_root, service) = fixture(script);
        let started = service.start(input("plan")).await.unwrap();
        let task = terminal(&service, &started.task_id).await;
        assert_eq!(task.state, TaskState::Failed, "{script}");
        assert_eq!(task.verification, TaskVerification::Unverified);
        assert!(task.evidence_truncated);
        assert!(task.progress.contains(expected_reason), "{}", task.progress);
        assert!(!task.progress.contains("private payload"));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn companion_preserves_the_first_canonical_failure_or_terminal_stop_reason() {
    for has_failure_event in [true, false] {
        let (root, service) = fixture("printf 'private stderr marker' >&2\ncat fixture.jsonl\nexit 7");
        let mut completion = completed();
        completion["subtype"] = json!("error_during_execution");
        completion["stop_reason"] = json!("terminal evaluator failure password=1234567890123456");
        let mut events = Vec::new();
        if has_failure_event {
            events.push(json!({"type":"turn.failed", "message":"parse evaluator response: decode json payload"}));
        }
        events.push(completion);
        std::fs::write(
            root.path().join("fixture.jsonl"),
            events.iter().map(|event| format!("{event}\n")).collect::<String>(),
        )
        .unwrap();
        let started = service.start(input("plan")).await.unwrap();
        let result = terminal(&service, &started.task_id).await;
        let expected = if has_failure_event {
            "parse evaluator response: decode json payload"
        } else {
            "terminal evaluator failure"
        };
        assert_eq!(result.state, TaskState::Failed);
        assert!(result.progress.contains(expected), "{}", result.progress);
        assert!(result.progress.contains("exit status: 7"), "{}", result.progress);
        assert!(!result.progress.contains("missing canonical thread completion"));
        assert!(!serde_json::to_string(&result).unwrap().contains("private stderr marker"));
        assert!(!result.progress.contains("1234567890123456"));
        assert!(result.progress.len() <= 2048);
        assert_eq!(result.verification, TaskVerification::Unverified);
        assert!(result.evidence_truncated);
    }
}

#[test]
fn companion_bounds_canonical_failure_details_and_distinguishes_supervisor_failures() {
    let mut projection = EventProjection::default();
    let mut task = snapshot();
    projection.observe(&mut task, event(json!({"type":"error", "message":"é".repeat(2048)})));
    assert!(task.progress.len() <= 1050);
    assert!(task.evidence_truncated);
    let outcome = Some(Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "task deadline exceeded")));
    let deadline = projection.failure_progress(outcome.as_ref(), true, true);
    assert!(deadline.contains("task deadline exceeded"));
    let cleanup = EventProjection::default().failure_progress(None, false, false);
    assert!(cleanup.contains("owned process cleanup failed"));
    assert!(cleanup.contains("child reap could not be confirmed"));
    let mut completed_projection = EventProjection::default();
    completed_projection.observe(&mut snapshot(), event(completed()));
    assert_eq!(completed_projection.failure_progress(None, false, true), "owned process cleanup failed");
}

#[cfg(unix)]
#[tokio::test]
async fn companion_task_failure_preserves_negative_verification_and_drops_positive_proof() {
    for (kind, code, expected) in [
        ("verification_failed", 7, TaskVerification::Failed),
        ("verification_passed", 0, TaskVerification::Unverified),
    ] {
        let (root, service) = fixture("cat fixture.jsonl\nexit 7");
        let mut completion = completed();
        completion["subtype"] = json!("error_during_execution");
        std::fs::write(
            root.path().join("fixture.jsonl"),
            format!("{}\n{}\n", serde_json::to_string(&verification(kind, Some(code))).unwrap(), completion),
        )
        .unwrap();
        let started = service.start(input("plan")).await.unwrap();
        let result = terminal(&service, &started.task_id).await;
        assert_eq!(result.state, TaskState::Failed);
        assert_eq!(result.verification, expected);
        assert!(result.evidence_truncated);
        if expected == TaskVerification::Failed {
            assert_eq!(result.verification_exit_code, Some(7));
            assert_eq!(result.verification_command.as_deref(), Some("cargo check --locked"));
        } else {
            assert!(result.verification_command.is_none());
            assert!(result.verification_exit_code.is_none());
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn companion_cancellation_preserves_observed_failed_check_and_marks_missing_tail() {
    let (root, service) = fixture("cat fixture.jsonl\nsleep 30");
    std::fs::write(
        root.path().join("fixture.jsonl"),
        format!("{}\n", serde_json::to_string(&verification("verification_failed", Some(9))).unwrap()),
    )
    .unwrap();
    let started = service.start(input("plan")).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if service.get(&started.task_id).await.unwrap().verification == TaskVerification::Failed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(service.cancel(&started.task_id).await.unwrap());
    let result = terminal(&service, &started.task_id).await;
    assert_eq!(result.state, TaskState::Cancelled);
    assert_eq!(result.verification, TaskVerification::Failed);
    assert_eq!(result.verification_exit_code, Some(9));
    assert!(result.evidence_truncated);
}

#[cfg(unix)]
#[tokio::test]
async fn companion_bounds_post_exit_drain_and_terminates_descendants() {
    let (root, service) = fixture("sleep 30 &\nprintf '%s' \"$!\" > descendant.pid\ncat fixture.jsonl");
    let started = service.start(input("plan")).await.unwrap();
    assert_eq!(terminal(&service, &started.task_id).await.state, TaskState::Failed);
    let descendant = std::fs::read_to_string(root.path().join("descendant.pid")).unwrap();
    let status = std::process::Command::new("ps")
        .args(["-p", &descendant, "-o", "stat="])
        .output()
        .unwrap();
    let status = String::from_utf8(status.stdout).unwrap();
    assert!(
        status.trim().is_empty() || status.trim_start().starts_with('Z'),
        "descendant still executing: {status}"
    );
}
