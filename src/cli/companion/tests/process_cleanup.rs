//! Service-level cleanup with a real native helper in a detached tool group.
use super::*;
use nix::errno::Errno;
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use std::os::unix::fs::PermissionsExt;

fn fixture_with_owned_child(invalid_output: bool) -> (TempDir, CompanionService) {
    let root = TempDir::new().unwrap();
    let native = assert_cmd::cargo::cargo_bin("vtcode");
    assert!(native.is_file(), "build the shipped vtcode binary before this test");
    // The shipped debug binary is hundreds of megabytes, and its first exec in a
    // test process pays a multi-second pre-`main` startup cost that has nothing to
    // do with command lifetime. Load it once here so fixture readiness below
    // measures the supervisor rather than that startup.
    let warm = std::process::Command::new(&native)
        .arg("--version")
        .output()
        .expect("load the shipped vtcode binary");
    assert!(warm.status.success(), "shipped vtcode binary failed to start");
    let executable = root.path().join("fake-vtcode");
    let script = format!(
        r#"#!/usr/bin/env python3
import os, subprocess, sys, time
sys.stdin.read()
open('runner-pid', 'w').write(str(os.getpid()))
subprocess.Popen([{native}, '--vtcode-owned-exec', '/bin/sh', '/bin/sh', '-c',
    'sleep 60 & child=$!; printf "%s\\n" "$child" > owned-pid; wait'],
    start_new_session=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
deadline = time.monotonic() + 5
while not os.path.exists('owned-pid'):
    if time.monotonic() >= deadline: raise RuntimeError('owned helper did not start')
    time.sleep(.01)
print({output}, flush=True)
time.sleep(60)
"#,
        native = serde_json::to_string(&native.to_string_lossy()).unwrap(),
        output = serde_json::to_string(if invalid_output {
            "invalid JSON"
        } else {
            "{\"type\":\"thread.started\",\"thread_id\":\"cleanup\"}"
        })
        .unwrap(),
    );
    std::fs::write(&executable, script).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let service = CompanionService::new(root.path().to_owned(), executable, "TEST_PROXY_SECRET".into(), Vec::new());
    (root, service)
}

async fn read_pid(root: &TempDir, name: &str) -> Pid {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(text) = std::fs::read_to_string(root.path().join(name))
                && let Ok(pid) = text.trim().parse::<i32>()
            {
                return Pid::from_raw(pid);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fixture PID published")
}

async fn assert_gone(pid: Pid) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while kill(pid, None) != Err(Errno::ESRCH) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("detached managed child survived cleanup");
}

#[tokio::test]
async fn companion_cancellation_closes_detached_command_ownership() {
    let (root, service) = fixture_with_owned_child(false);
    let started = service.start(input("plan")).await.unwrap();
    let child = read_pid(&root, "owned-pid").await;
    assert!(kill(child, None).is_ok());
    assert!(service.cancel(&started.task_id).await.unwrap());
    assert_eq!(terminal(&service, &started.task_id).await.state, TaskState::Cancelled);
    assert_gone(child).await;
    service.shutdown().await.unwrap();
}

#[tokio::test]
async fn companion_collector_failure_closes_detached_command_ownership() {
    let (root, service) = fixture_with_owned_child(true);
    let started = service.start(input("plan")).await.unwrap();
    let child = read_pid(&root, "owned-pid").await;
    let result = terminal(&service, &started.task_id).await;
    assert_eq!(result.state, TaskState::Failed);
    assert!(result.evidence_truncated);
    assert_gone(child).await;
    service.shutdown().await.unwrap();
}

#[tokio::test]
async fn companion_runner_sigkill_closes_detached_command_ownership() {
    let (root, service) = fixture_with_owned_child(false);
    let started = service.start(input("plan")).await.unwrap();
    let child = read_pid(&root, "owned-pid").await;
    kill(read_pid(&root, "runner-pid").await, Signal::SIGKILL).unwrap();
    assert_eq!(terminal(&service, &started.task_id).await.state, TaskState::Failed);
    assert_gone(child).await;
    service.shutdown().await.unwrap();
}

#[tokio::test]
async fn companion_supervisor_abort_closes_detached_command_ownership() {
    let (root, service) = fixture_with_owned_child(false);
    let started = service.start(input("plan")).await.unwrap();
    let child = read_pid(&root, "owned-pid").await;
    service
        .tasks
        .lock()
        .await
        .get(&started.task_id)
        .unwrap()
        .worker
        .as_ref()
        .unwrap()
        .abort();
    assert_eq!(terminal(&service, &started.task_id).await.state, TaskState::Failed);
    assert_gone(child).await;
    assert!(service.shutdown().await.is_err());
}

#[tokio::test]
async fn companion_deadline_closes_detached_command_ownership() {
    let (root, service) = fixture_with_owned_child(false);
    // Exercise the production deadline branch with a shorter private budget; the
    // public service entry point retains its thirty-minute limit. The budget must
    // still outlast fixture readiness, which launches the (warmed) native binary
    // through the ownership helper before there is a detached command to close.
    let started = service.start_with_timeout(input("plan"), Duration::from_secs(2)).await.unwrap();
    let child = read_pid(&root, "owned-pid").await;
    assert!(
        kill(child, None).is_ok(),
        "fixture published its owned command only after the deadline had already closed ownership"
    );
    let result = terminal(&service, &started.task_id).await;
    assert_eq!(result.state, TaskState::Failed);
    assert!(result.progress.contains("task deadline exceeded"), "{}", result.progress);
    assert!(result.evidence_truncated);
    assert_gone(child).await;
    service.shutdown().await.unwrap();
}
