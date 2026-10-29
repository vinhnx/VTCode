pub(super) use super::*;
pub(super) use crate::config::VTCodeConfig;
pub(super) use crate::core::agent::events::SessionStoreSink;
pub(super) use crate::subagents::tests::test_controller_config;
pub(super) use crate::tools::registry::ToolRegistry;
pub(super) use std::time::Duration;

fn task(id: &str, access: WorkspaceAccess) -> MatrixTaskSpec {
    MatrixTaskSpec {
        id: id.into(),
        instructions: format!("perform {id}"),
        dependencies: vec![],
        workspace: ".".into(),
        access,
        checks: vec![format!("check-{id}"), format!("second-{id}")],
        resources: Default::default(),
        timeout_secs: 5,
        replay_safe: false,
        inputs: vec![],
    }
}

fn rejected(result: Result<Value>) -> bool {
    match result {
        Err(_) => true,
        Ok(value) => value.get("success") == Some(&json!(false)) || value.get("error").is_some(),
    }
}

async fn controller(root: &std::path::Path, sink: &SessionStoreSink) -> SubagentController {
    let controller = SubagentController::new(test_controller_config(root.to_path_buf(), VTCodeConfig::default()))
        .await
        .unwrap();
    controller.set_matrix_persistence(sink.matrix_persistence()).await.unwrap();
    controller
}

fn success(assignment: MatrixAssignment, task: MatrixTaskSpec) -> worker::WorkerResult {
    let evidence = if assignment.phase == MatrixPhase::Verify {
        task.checks
            .into_iter()
            .enumerate()
            .map(|(index, command)| MatrixCommandEvidence {
                command,
                attempt_id: assignment.attempt_id.clone(),
                worker_id: assignment.worker_id.clone(),
                generation: assignment.generation.clone().unwrap(),
                event_id: format!("{}-{index}", assignment.attempt_id),
                exit_code: Some(0),
                cancelled: false,
            })
            .collect()
    } else {
        vec![]
    };
    worker::WorkerResult {
        assignment,
        outcome: MatrixOutcome::Success,
        evidence,
        cleanup_confirmed: true,
    }
}

async fn wait_terminal(controller: &SubagentController) -> MatrixSnapshot {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let snapshot = controller.matrix.state.lock().await.as_ref().unwrap().snapshot().clone();
            if matches!(
                snapshot.lifecycle,
                MatrixLifecycle::Succeeded | MatrixLifecycle::Blocked | MatrixLifecycle::Cancelled
            ) && !controller.matrix.driver_active.load(Ordering::Acquire)
            {
                return snapshot;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}

fn init_git(root: &std::path::Path) {
    assert!(
        std::process::Command::new("git")
            .arg("init")
            .current_dir(root)
            .output()
            .unwrap()
            .status
            .success()
    );
    std::fs::write(root.join("source.rs"), "before").unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["add", "source.rs"])
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );
}

mod admission;
mod lifecycle;
mod pause_persistence;
mod resume_bootstrap;
mod verification;
