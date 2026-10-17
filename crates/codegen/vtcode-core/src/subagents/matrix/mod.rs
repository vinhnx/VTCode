//! Local matrix orchestration beside the subagent runtime. Canonical snapshots
//! are acknowledged before launch and before exposing completion.
mod fingerprint;
mod projection;
mod worker;

use super::SubagentController;
use crate::exec::events::matrix::*;
use anyhow::{Context, Result, bail, ensure};
use futures::future::BoxFuture;
use parking_lot::RwLock;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use vtcode_memory::matrix::MatrixState;

/// Access to the existing authoritative drain, not a second persisted store.
#[derive(Clone)]
pub struct MatrixPersistence {
    pub persist: Arc<dyn Fn(MatrixSnapshot) -> BoxFuture<'static, Result<()>> + Send + Sync>,
    pub load: Arc<dyn Fn() -> BoxFuture<'static, Result<Vec<MatrixSnapshot>>> + Send + Sync>,
}

#[derive(Default)]
pub(super) struct MatrixRuntime {
    #[cfg(test)]
    executor_override: RwLock<Option<TestExecutor>>,
    persistence: RwLock<Option<MatrixPersistence>>,
    updated_at: RwLock<chrono::DateTime<chrono::Utc>>,
    state: Mutex<Option<MatrixState>>,
    driver_active: AtomicBool,
    pub(super) executing: AtomicBool,
    notify: Notify,
    pub(super) cancellation: RwLock<CancellationToken>,
    error: RwLock<Option<String>>,
    completion: parking_lot::Mutex<Option<MatrixSnapshot>>,
}

#[cfg(test)]
type TestExecutor = Arc<
    dyn Fn(MatrixAssignment, MatrixTaskSpec, CancellationToken) -> BoxFuture<'static, worker::WorkerResult>
        + Send
        + Sync,
>;

/// Only the runtime can bind this capability to a worker registry.
#[derive(Clone)]
pub(crate) struct MatrixWorkerContext {
    pub assignment: MatrixAssignment,
    report: Arc<parking_lot::Mutex<Option<Value>>>,
}
impl MatrixWorkerContext {
    fn new(assignment: MatrixAssignment) -> Self {
        Self {
            assignment,
            report: Arc::new(parking_lot::Mutex::new(None)),
        }
    }
    pub(crate) fn reported_outcome(&self) -> Option<MatrixOutcome> {
        self.report
            .lock()
            .as_ref()
            .and_then(|report| report.get("outcome").and_then(Value::as_str))
            .and_then(|outcome| match outcome {
                "executed" => Some(MatrixOutcome::Success),
                "failed" => Some(MatrixOutcome::Failed),
                "permission_denied" => Some(MatrixOutcome::PermissionDenied),
                "budget_exhausted" => Some(MatrixOutcome::BudgetExhausted),
                "interrupted" => Some(MatrixOutcome::Interrupted),
                "timed_out" => Some(MatrixOutcome::TimedOut),
                _ => None,
            })
    }
    pub(crate) fn report(&self, args: Value) -> Result<Value> {
        ensure!(args.get("action").and_then(Value::as_str) == Some("report"), "matrix workers may only report");
        ensure!(
            args.get("matrix_id").is_none()
                && args.get("task_id").is_none()
                && args.get("attempt_id").is_none()
                && args.get("worker_id").is_none(),
            "worker report identity is runtime-owned"
        );
        let outcome = args
            .get("outcome")
            .and_then(Value::as_str)
            .context("matrix report requires outcome")?;
        ensure!(
            [
                "executed",
                "failed",
                "interrupted",
                "timed_out",
                "permission_denied",
                "budget_exhausted"
            ]
            .contains(&outcome),
            "invalid matrix report outcome"
        );
        let mut report = self.report.lock();
        ensure!(report.is_none(), "duplicate matrix report");
        *report = Some(args);
        Ok(
            json!({"accepted":true,"task_id":self.assignment.task_id,"attempt_id":self.assignment.attempt_id,"final_success":false}),
        )
    }
}

impl SubagentController {
    /// Coalesced notification projection; lifecycle authority stays in events.
    pub fn take_matrix_completion(&self) -> Option<MatrixSnapshot> {
        self.matrix.completion.lock().take()
    }
    /// Idle wakeups inspect readiness without consuming the outer loop's result.
    pub fn has_matrix_completion(&self) -> bool {
        self.matrix.completion.lock().is_some()
    }
    pub async fn set_matrix_persistence(&self, persistence: MatrixPersistence) -> Result<()> {
        let mut guard = self.matrix.state.lock().await;
        *self.matrix.persistence.write() = Some(persistence.clone());
        if guard.is_some() {
            return Ok(());
        }
        // Admission stays closed until replay and its reconciliation checkpoint
        // have been acknowledged. A failed load must never enable discovery.
        self.matrix.executing.store(true, Ordering::Release);
        let mut retained = (persistence.load)().await?.into_iter().filter(|snapshot| {
            !matches!(snapshot.lifecycle, MatrixLifecycle::Cancelled | MatrixLifecycle::Succeeded)
                || snapshot
                    .tasks
                    .iter()
                    .flat_map(|task| &task.attempts)
                    .any(|attempt| !attempt.cleanup_confirmed)
        });
        let snapshot = retained.next();
        ensure!(retained.next().is_none(), "multiple unreconciled matrices in one local session");
        if let Some(snapshot) = snapshot {
            let mut restored = MatrixState::from_snapshot(snapshot)?;
            self.matrix.recover(&mut restored).await?;
            self.matrix.update_execution_admission(&restored);
            *guard = Some(restored);
        } else {
            self.matrix.executing.store(false, Ordering::Release);
        }
        Ok(())
    }
    pub fn matrix_is_executing(&self) -> bool {
        self.matrix.executing.load(Ordering::Acquire)
    }
    pub(super) fn ensure_ordinary_delegation_allowed(&self) -> Result<()> {
        ensure!(!self.matrix_is_executing(), "active matrix execution must use scheduler-owned workers");
        Ok(())
    }
    pub(crate) fn matrix_is_driving(&self) -> bool {
        self.matrix.driver_active.load(Ordering::Acquire)
    }
    pub(crate) fn request_matrix_stop(&self) {
        self.matrix.cancellation.read().cancel();
        self.matrix.notify.notify_one();
    }
    pub(crate) async fn matrix_snapshot(&self) -> Option<MatrixSnapshot> {
        self.matrix.state.lock().await.as_ref().map(|state| state.snapshot().clone())
    }
    /// Headless completion keeps the canonical sink alive until admitted work stops.
    pub(crate) async fn wait_matrix_idle(&self) -> Option<MatrixSnapshot> {
        loop {
            let notified = self.background_completion_notify.notified();
            if !self.matrix.driver_active.load(Ordering::Acquire) {
                return self.matrix_snapshot().await;
            }
            tokio::select! {
                () = notified => {}
                () = tokio::time::sleep(std::time::Duration::from_millis(250)) => {}
            }
        }
    }
    pub async fn cancel_matrix(&self) -> Result<()> {
        // Stopping owned work cannot depend on a functioning persistence sink.
        self.request_matrix_stop();
        let mut guard = self.matrix.state.lock().await;
        if let Some(state) = guard.as_ref()
            && !matches!(state.snapshot().lifecycle, MatrixLifecycle::Cancelled | MatrixLifecycle::Succeeded)
        {
            let mut next = state.clone();
            next.cancel()?;
            self.matrix.persist(&next).await?;
            self.matrix.update_execution_admission(&next);
            *guard = Some(next);
        }
        Ok(())
    }
    pub async fn matrix_control(&self, args: Value) -> Result<Value> {
        let action = args.get("action").and_then(Value::as_str).context("matrix requires action")?;
        ensure!(action != "report", "matrix report is worker-only");
        if ["start", "resume", "retry"].contains(&action) {
            ensure!(
                self.config.depth == 0
                    && self.config.vt_cfg.subagents.enabled
                    && self.config.vt_cfg.subagents.max_concurrent > 0
                    && self.config.vt_cfg.subagents.max_depth > 0,
                "matrix requires root-session enabled subagents with positive capacity and depth"
            );
            ensure!(!self.matrix.cancellation.read().is_cancelled(), "user cancellation prevents matrix continuation");
        }
        let requested_id = args.get("matrix_id").and_then(Value::as_str);
        let mut guard = self.matrix.state.lock().await;
        if guard.is_none() && action != "create" {
            let persistence = self
                .matrix
                .persistence
                .read()
                .clone()
                .context("canonical matrix persistence unavailable")?;
            let snapshots = (persistence.load)().await?;
            let snapshot = snapshots
                .into_iter()
                .find(|snapshot| Some(snapshot.spec.id.as_str()) == requested_id)
                .context("unknown matrix; provide matrix_id")?;
            let mut restored = MatrixState::from_snapshot(snapshot)?;
            self.matrix.update_execution_admission(&restored);
            self.matrix.recover(&mut restored).await?;
            *guard = Some(restored);
        }
        if action == "create" {
            ensure!(!self.matrix.driver_active.load(Ordering::Acquire), "matrix driver is still cleaning up");
            if let Some(state) = guard.as_ref() {
                ensure!(
                    matches!(state.snapshot().lifecycle, MatrixLifecycle::Cancelled | MatrixLifecycle::Succeeded)
                        && state.active_assignments().is_empty(),
                    "one local matrix may be active at a time"
                );
            }
            let spec: MatrixSpec =
                serde_json::from_value(args.get("spec").cloned().context("matrix create requires spec")?)?;
            ensure!(requested_id.is_none_or(|id| id == spec.id), "matrix_id does not match specification");
            let persistence = self
                .matrix
                .persistence
                .read()
                .clone()
                .context("canonical matrix persistence unavailable")?;
            let existing = (persistence.load)().await?;
            ensure!(
                existing.iter().all(|snapshot| matches!(
                    snapshot.lifecycle,
                    MatrixLifecycle::Cancelled | MatrixLifecycle::Succeeded
                ) && snapshot
                    .tasks
                    .iter()
                    .flat_map(|task| &task.attempts)
                    .all(|attempt| attempt.cleanup_confirmed)),
                "resume the existing matrix and reconcile owned cleanup before creating another"
            );
            ensure!(
                !existing.iter().any(|snapshot| snapshot.spec.id == spec.id),
                "matrix ID already exists; use a new ID"
            );
            let state = MatrixState::create(spec, &self.config.workspace_root)?;
            self.matrix.persist(&state).await?;
            self.matrix.update_execution_admission(&state);
            *guard = Some(state);
            *self.matrix.cancellation.write() = CancellationToken::new();
            *self.matrix.error.write() = None;
        } else if action != "status" {
            let mut state = guard.as_ref().context("matrix is unavailable")?.clone();
            ensure!(requested_id == Some(state.snapshot().spec.id.as_str()), "matrix_id does not match active matrix");
            if ["start", "resume", "retry"].contains(&action) && !self.matrix_is_driving() {
                self.matrix.recover(&mut state).await?;
                self.matrix.update_execution_admission(&state);
                *guard = Some(state.clone());
            }
            let mut next = state.clone();
            match action {
                "start" => {
                    ensure!(
                        self.config.vt_cfg.subagents.enabled && self.config.vt_cfg.subagents.max_concurrent > 0,
                        "matrix requires enabled subagents with positive concurrency"
                    );
                    vtcode_memory::matrix::validate_workspace(&next.snapshot().spec, &self.config.workspace_root)?;
                    fingerprint::fingerprint(&self.config.workspace_root, &next.snapshot().spec).await?;
                    next.start()?;
                }
                "pause" => next.pause()?,
                "resume" => {
                    if next.snapshot().lifecycle == MatrixLifecycle::Paused {
                        next.resume()?;
                    } else {
                        ensure!(
                            matches!(next.snapshot().lifecycle, MatrixLifecycle::Running | MatrixLifecycle::Verifying),
                            "matrix needs a coordinator retry or confirmed owned cleanup"
                        );
                    }
                }
                "retry" => {
                    next.retry(args.get("task_id").and_then(Value::as_str).context("retry requires task_id")?)?
                }
                "cancel" => {
                    self.matrix.cancellation.read().cancel();
                    next.cancel()?;
                }
                _ => bail!("unknown matrix action {action}"),
            }
            if ["start", "resume", "retry"].contains(&action) && !self.matrix.driver_active.load(Ordering::Acquire) {
                // Block new discovery before inspecting its reserved permits,
                // including when restoring a matrix into a warm session.
                self.matrix.executing.store(true, Ordering::Release);
                if self.admission.available_permits()
                    != self
                        .config
                        .vt_cfg
                        .subagents
                        .max_concurrent
                        .min(vtcode_config::subagents::SUBAGENT_HARD_CONCURRENCY_LIMIT)
                {
                    self.matrix.update_execution_admission(&state);
                    bail!("wait for discovery workers to stop before matrix dispatch");
                }
            }
            if next.snapshot() != state.snapshot()
                && let Err(error) = self.matrix.persist(&next).await
            {
                self.matrix.update_execution_admission(&state);
                return Err(error);
            }
            self.matrix.update_execution_admission(&next);
            *guard = Some(next);
            if action == "cancel" {
                self.matrix.cancellation.read().cancel();
            }
        }
        let snapshot = guard.as_ref().context("matrix unavailable")?.snapshot().clone();
        if let Some(id) = requested_id {
            ensure!(id == snapshot.spec.id, "matrix_id does not match active matrix");
        }
        drop(guard);
        self.matrix.notify.notify_one();
        if ["start", "resume", "retry"].contains(&action) && !self.matrix.driver_active.swap(true, Ordering::AcqRel) {
            self.matrix.executing.store(true, Ordering::Release);
            *self.matrix.error.write() = None;
            let driver_cancel = self.matrix.cancellation.read().child_token();
            let controller = self.clone();
            tokio::spawn(async move {
                if let Err(error) = controller.matrix_drive(&driver_cancel).await {
                    *controller.matrix.error.write() = Some(format!("{error:#}"));
                    // Stop this driver's owned work without turning an internal
                    // failure into the user's terminal cancellation decision.
                    driver_cancel.cancel();
                }
                controller.matrix.driver_active.store(false, Ordering::Release);
                // Cleanup-uncertain state keeps the coordinator restricted.
                let state = controller.matrix.state.lock().await;
                if let Some(state) = state.as_ref()
                    && matches!(state.snapshot().lifecycle, MatrixLifecycle::Succeeded | MatrixLifecycle::Blocked)
                {
                    *controller.matrix.completion.lock() = Some(state.snapshot().clone());
                }
                if let Some(state) = state.as_ref() {
                    controller.matrix.update_execution_admission(state);
                }
                controller.background_completion_notify.notify_one();
            });
        }
        let mut result = projection::tracker_result(&snapshot);
        result["matrix"] = json!(snapshot);
        result["persistence_error"] = json!(self.matrix.error.read().clone());
        Ok(result)
    }

    async fn matrix_drive(&self, driver_cancel: &CancellationToken) -> Result<()> {
        let mut workers = JoinSet::new();
        let mut pending = BTreeMap::<String, worker::WorkerResult>::new();
        loop {
            let mut guard = self.matrix.state.lock().await;
            let state = guard.as_ref().context("matrix disappeared")?;
            let mut next = state.clone();
            if self.matrix.cancellation.read().is_cancelled()
                && !matches!(next.snapshot().lifecycle, MatrixLifecycle::Cancelled | MatrixLifecycle::Succeeded)
            {
                next.cancel()?;
            }
            if workers.is_empty() && !pending.is_empty() {
                for (_, result) in std::mem::take(&mut pending) {
                    next.report(
                        &result.assignment.attempt_id,
                        &result.assignment.worker_id,
                        result.outcome,
                        result.evidence,
                        result.cleanup_confirmed,
                    )?;
                }
                // Preserve stopped-work results even if inputs became unavailable.
                self.matrix.persist(&next).await?;
                *guard = Some(next.clone());
                if next.snapshot().lifecycle != MatrixLifecycle::Cancelled {
                    let current = fingerprint::fingerprint(&self.config.workspace_root, &next.snapshot().spec).await?;
                    let generation_changed = next
                        .snapshot()
                        .generation
                        .as_deref()
                        .is_some_and(|generation| generation != current);
                    if generation_changed
                        && next.active_assignments().is_empty()
                        && matches!(next.snapshot().lifecycle, MatrixLifecycle::Verifying | MatrixLifecycle::Paused)
                    {
                        next.invalidate_verification(current)?;
                    }
                }
            }
            if workers.is_empty()
                && next.snapshot().lifecycle == MatrixLifecycle::Running
                && next
                    .snapshot()
                    .tasks
                    .iter()
                    .all(|task| task.status == MatrixTaskStatus::Executed)
            {
                let generation = fingerprint::fingerprint(&self.config.workspace_root, &next.snapshot().spec).await?;
                next.begin_verification(generation)?;
            }
            if workers.is_empty()
                && next.snapshot().lifecycle == MatrixLifecycle::Verifying
                && next
                    .snapshot()
                    .tasks
                    .iter()
                    .all(|task| task.status == MatrixTaskStatus::Verified)
            {
                let generation = fingerprint::fingerprint(&self.config.workspace_root, &next.snapshot().spec).await?;
                if next.snapshot().generation.as_deref() != Some(generation.as_str()) {
                    next.invalidate_verification(generation)?;
                } else {
                    next.finalize_verification(&generation)?;
                }
            }
            let available = self.admission.available_permits();
            let active = next.active_assignments().len();
            if matches!(next.snapshot().lifecycle, MatrixLifecycle::Running | MatrixLifecycle::Verifying) {
                vtcode_memory::matrix::validate_workspace(&next.snapshot().spec, &self.config.workspace_root)?;
            }
            let assignments =
                next.reserve_ready((active + available).min(self.config.vt_cfg.subagents.max_concurrent).min(5))?;
            let mut launches = Vec::new();
            for assignment in assignments {
                match Arc::clone(&self.admission).try_acquire_owned() {
                    Ok(permit) => {
                        let task = next
                            .snapshot()
                            .spec
                            .tasks
                            .iter()
                            .find(|task| task.id == assignment.task_id)
                            .context("unknown assigned task")?
                            .clone();
                        launches.push((assignment, task, permit));
                    }
                    Err(_) => next.rollback_launch(&assignment.attempt_id)?,
                }
            }
            if next.snapshot() != guard.as_ref().context("matrix unavailable")?.snapshot() {
                self.matrix.persist(&next).await?;
                *guard = Some(next);
                self.background_completion_notify.notify_one();
            }
            if !launches.is_empty() {
                let mut launching = guard.as_ref().context("matrix unavailable")?.clone();
                for (assignment, _, _) in &launches {
                    launching.mark_launch_requested(&assignment.attempt_id)?;
                }
                self.matrix.persist(&launching).await?;
                *guard = Some(launching);
            }
            let lifecycle = guard.as_ref().context("matrix unavailable")?.snapshot().lifecycle;
            drop(guard);
            for (assignment, task, permit) in launches {
                let controller = self.clone();
                let cancel = driver_cancel.child_token();
                #[cfg(test)]
                let executor = self.matrix.executor_override.read().clone();
                workers.spawn(async move {
                    let _permit = permit;
                    #[cfg(test)]
                    if let Some(executor) = executor {
                        return executor(assignment, task, cancel).await;
                    }
                    Box::pin(worker::execute(&controller, assignment, task, cancel)).await
                });
            }
            if workers.is_empty() && !matches!(lifecycle, MatrixLifecycle::Running | MatrixLifecycle::Verifying) {
                return Ok(());
            }
            if workers.is_empty() && lifecycle == MatrixLifecycle::Verifying {
                continue;
            }
            tokio::select! {
                result = workers.join_next(), if !workers.is_empty() => {
                    let result = result.context("matrix worker missing")?.context("matrix worker panicked; cleanup ownership uncertain")?;
                    if result.assignment.phase == MatrixPhase::Verify {
                        pending.insert(result.assignment.attempt_id.clone(), result);
                    } else {
                        let mut guard = self.matrix.state.lock().await;
                        let mut next = guard.as_ref().context("matrix unavailable")?.clone();
                        next.report(&result.assignment.attempt_id, &result.assignment.worker_id, result.outcome, result.evidence, result.cleanup_confirmed)?;
                        self.matrix.persist(&next).await?;
                        *guard = Some(next);
                        self.background_completion_notify.notify_one();
                    }
                }
                () = self.matrix.notify.notified() => {}
                () = tokio::time::sleep(std::time::Duration::from_millis(250)) => {}
            }
        }
    }
}
impl MatrixRuntime {
    async fn recover(&self, state: &mut MatrixState) -> Result<()> {
        let previous = state.snapshot().clone();
        state.recover()?;
        if state.snapshot() != &previous {
            self.persist(state).await?;
        }
        Ok(())
    }

    fn update_execution_admission(&self, state: &MatrixState) {
        let held = !state.active_assignments().is_empty()
            || !matches!(
                state.snapshot().lifecycle,
                MatrixLifecycle::Created | MatrixLifecycle::Succeeded | MatrixLifecycle::Cancelled
            );
        self.executing.store(held, Ordering::Release);
    }

    async fn persist(&self, state: &MatrixState) -> Result<()> {
        let persistence = self
            .persistence
            .read()
            .clone()
            .context("canonical matrix persistence unavailable")?;
        (persistence.persist)(state.snapshot().clone()).await?;
        *self.updated_at.write() = chrono::Utc::now();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
