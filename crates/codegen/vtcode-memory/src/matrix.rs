//! Deterministic matrix projection and atomic local resource admission.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};
use thiserror::Error;
use vtcode_exec_events::{ThreadEvent, matrix::*};

#[derive(Debug, Error)]
#[error("{0}")]
/// Specification, transition, or recovery validation failure.
pub struct MatrixError(pub String);
/// Result of a matrix state transition.
pub type MatrixResult<T> = Result<T, MatrixError>;

fn require(condition: bool, message: impl Into<String>) -> MatrixResult<()> {
    if condition {
        Ok(())
    } else {
        Err(MatrixError(message.into()))
    }
}

/// Validate source-independent specification structure before persistence or replay.
pub fn validate_spec(spec: &MatrixSpec) -> MatrixResult<()> {
    require(!spec.id.trim().is_empty() && !spec.tasks.is_empty(), "matrix needs an ID and tasks")?;
    require(
        spec.resources.iter().all(|(id, n)| !id.trim().is_empty() && *n > 0),
        "resource capacities must be positive",
    )?;
    let ids: BTreeSet<_> = spec.tasks.iter().map(|task| task.id.as_str()).collect();
    require(ids.len() == spec.tasks.len(), "duplicate task IDs")?;
    for task in &spec.tasks {
        require(
            !task.id.trim().is_empty() && !task.instructions.trim().is_empty(),
            "task needs an ID and instructions",
        )?;
        require(task.timeout_secs > 0, "task timeout must be positive")?;
        require(
            !task.checks.is_empty() && task.checks.iter().all(|check| !check.trim().is_empty()),
            "task needs nonempty verification commands",
        )?;
        validate_relative(&task.workspace)?;
        for input in &task.inputs {
            validate_relative(input)?;
        }
        require(task.dependencies.iter().all(|id| ids.contains(id.as_str())), "unknown dependency")?;
        require(
            task.dependencies.iter().collect::<BTreeSet<_>>().len() == task.dependencies.len(),
            "duplicate dependency",
        )?;
        for (resource, count) in &task.resources {
            require(
                *count > 0 && spec.resources.get(resource).is_some_and(|capacity| count <= capacity),
                format!("invalid resource requirement: {resource}"),
            )?;
        }
    }
    let mut visited = BTreeSet::new();
    loop {
        let previous = visited.len();
        for task in &spec.tasks {
            if task.dependencies.iter().all(|id| visited.contains(id.as_str())) {
                visited.insert(task.id.as_str());
            }
        }
        if visited.len() == spec.tasks.len() {
            return Ok(());
        }
        require(visited.len() > previous, "dependency cycle")?;
    }
}

fn validate_relative(path: &str) -> MatrixResult<()> {
    require(
        !path.is_empty()
            && !Path::new(path).is_absolute()
            && Path::new(path)
                .components()
                .all(|component| matches!(component, Component::Normal(_) | Component::CurDir)),
        "workspace and input paths must stay relative to the workspace",
    )
}

/// Reject workspace symlink escapes and unavailable declared inputs before dispatch.
pub fn validate_workspace(spec: &MatrixSpec, root: &Path) -> MatrixResult<()> {
    let canonical_root =
        vtcode_commons::canonicalize(root).map_err(|error| MatrixError(format!("resolve workspace: {error}")))?;
    for task in &spec.tasks {
        let workspace = vtcode_commons::canonicalize(root.join(&task.workspace))
            .map_err(|error| MatrixError(format!("resolve task workspace {}: {error}", task.id)))?;
        require(
            workspace.starts_with(&canonical_root) && workspace.is_dir(),
            "task workspace escapes root or is not a directory",
        )?;
        for input in &task.inputs {
            let path = vtcode_commons::canonicalize(workspace.join(input))
                .map_err(|error| MatrixError(format!("resolve input {input}: {error}")))?;
            require(
                path.starts_with(&canonical_root) && path.is_file(),
                "declared input escapes root or is not a file",
            )?;
        }
    }
    Ok(())
}

/// State derives only from canonical snapshots; callers persist each mutation before effects.
#[derive(Debug, Clone)]
pub struct MatrixState {
    snapshot: MatrixSnapshot,
}

impl MatrixState {
    /// Create an idle matrix after structural and path validation.
    pub fn create(spec: MatrixSpec, root: &Path) -> MatrixResult<Self> {
        validate_spec(&spec)?;
        validate_workspace(&spec, root)?;
        let tasks = spec
            .tasks
            .iter()
            .map(|task| MatrixTaskState {
                id: task.id.clone(),
                status: MatrixTaskStatus::Queued,
                attempts: Vec::new(),
                automatic_retries: 0,
            })
            .collect();
        Ok(Self {
            snapshot: MatrixSnapshot {
                spec,
                lifecycle: MatrixLifecycle::Created,
                tasks,
                generation: None,
                revision: 0,
            },
        })
    }

    /// Validate and restore a retained canonical checkpoint.
    pub fn from_snapshot(snapshot: MatrixSnapshot) -> MatrixResult<Self> {
        validate_spec(&snapshot.spec)?;
        require(
            snapshot.tasks.len() == snapshot.spec.tasks.len()
                && snapshot
                    .tasks
                    .iter()
                    .zip(&snapshot.spec.tasks)
                    .all(|(state, spec)| state.id == spec.id),
            "snapshot task identities disagree with specification",
        )?;
        let mut attempt_ids = BTreeSet::new();
        let mut worker_ids = BTreeSet::new();
        let mut active_count = 0;
        let mut active_writer = false;
        let mut resource_usage = BTreeMap::<&str, u64>::new();
        for (task, spec) in snapshot.tasks.iter().zip(&snapshot.spec.tasks) {
            require(task.automatic_retries <= 1, "snapshot exceeds automatic retry limit")?;
            let active = task.attempts.iter().filter(|attempt| !attempt.cleanup_confirmed).count();
            require(active <= 1, "snapshot has overlapping attempts for one task")?;
            if active == 1 {
                require(
                    matches!(task.status, MatrixTaskStatus::Assigned | MatrixTaskStatus::CleanupUncertain),
                    "snapshot active attempt has inconsistent task status",
                )?;
                active_count += 1;
                active_writer |= spec.access == WorkspaceAccess::Write;
                for (name, quantity) in &spec.resources {
                    *resource_usage.entry(name).or_default() += u64::from(*quantity);
                }
            } else {
                require(
                    !matches!(task.status, MatrixTaskStatus::Assigned | MatrixTaskStatus::CleanupUncertain),
                    "snapshot assigned task has no owned attempt",
                )?;
            }
            for attempt in &task.attempts {
                require(
                    !attempt.id.is_empty()
                        && !attempt.worker_id.is_empty()
                        && attempt_ids.insert(&attempt.id)
                        && worker_ids.insert(&attempt.worker_id),
                    "snapshot has duplicate or empty attempt identities",
                )?;
            }
            if task.status == MatrixTaskStatus::Verified {
                let attempt = task
                    .attempts
                    .last()
                    .ok_or_else(|| MatrixError("verified task has no attempt".into()))?;
                require(
                    attempt.phase == MatrixPhase::Verify
                        && attempt.outcome == Some(MatrixOutcome::Success)
                        && attempt.cleanup_confirmed
                        && attempt.generation == snapshot.generation,
                    "verified task has no completed current-generation attempt",
                )?;
                validate_evidence(&spec.checks, attempt, &attempt.evidence)?;
            }
        }
        require(
            active_count <= 5 && (!active_writer || active_count <= 1),
            "snapshot overcommits worker or workspace leases",
        )?;
        require(
            resource_usage.iter().all(|(name, used)| {
                snapshot
                    .spec
                    .resources
                    .get(*name)
                    .is_some_and(|capacity| *used <= u64::from(*capacity))
            }),
            "snapshot overcommits named resources",
        )?;
        if snapshot.lifecycle == MatrixLifecycle::Succeeded {
            require(
                snapshot.tasks.iter().all(|task| {
                    task.status == MatrixTaskStatus::Verified
                        && task.attempts.iter().all(|attempt| attempt.cleanup_confirmed)
                }),
                "successful matrix has incomplete or unstopped work",
            )?;
        }
        Ok(Self { snapshot })
    }

    /// Inspect the complete canonical state.
    pub fn snapshot(&self) -> &MatrixSnapshot {
        &self.snapshot
    }
    /// Build the canonical event for an acknowledged persistence barrier.
    pub fn event(&self) -> ThreadEvent {
        ThreadEvent::MatrixUpdated(Box::new(self.snapshot.clone()))
    }
    fn changed(&mut self) {
        self.snapshot.revision = self.snapshot.revision.saturating_add(1);
    }
    fn terminal(&self) -> bool {
        matches!(self.snapshot.lifecycle, MatrixLifecycle::Cancelled | MatrixLifecycle::Succeeded)
    }
    /// Assignments whose owned work has not been confirmed stopped.
    pub fn active_assignments(&self) -> Vec<MatrixAssignment> {
        self.snapshot
            .tasks
            .iter()
            .flat_map(|task| {
                task.attempts
                    .iter()
                    .filter(|attempt| !attempt.cleanup_confirmed)
                    .map(|attempt| MatrixAssignment {
                        task_id: task.id.clone(),
                        attempt_id: attempt.id.clone(),
                        worker_id: attempt.worker_id.clone(),
                        phase: attempt.phase,
                        generation: attempt.generation.clone(),
                    })
            })
            .collect()
    }
    /// Freeze creation and permit scheduler dispatch.
    pub fn start(&mut self) -> MatrixResult<()> {
        require(self.snapshot.lifecycle == MatrixLifecycle::Created, "only a created matrix can start")?;
        self.snapshot.lifecycle = MatrixLifecycle::Running;
        self.changed();
        Ok(())
    }
    /// Stop new dispatch while admitted workers finish.
    pub fn pause(&mut self) -> MatrixResult<()> {
        require(
            matches!(self.snapshot.lifecycle, MatrixLifecycle::Running | MatrixLifecycle::Verifying),
            "matrix is not running",
        )?;
        self.snapshot.lifecycle = MatrixLifecycle::Paused;
        self.changed();
        Ok(())
    }
    /// Resume dispatch from a paused phase.
    pub fn resume(&mut self) -> MatrixResult<()> {
        require(self.snapshot.lifecycle == MatrixLifecycle::Paused, "matrix is not paused")?;
        self.snapshot.lifecycle = if self.snapshot.generation.is_some() {
            MatrixLifecycle::Verifying
        } else {
            MatrixLifecycle::Running
        };
        self.changed();
        Ok(())
    }
    /// Permanently stop new dispatch; owned workers still require cleanup.
    pub fn cancel(&mut self) -> MatrixResult<()> {
        require(!self.terminal(), "matrix is already terminal")?;
        self.snapshot.lifecycle = MatrixLifecycle::Cancelled;
        self.changed();
        Ok(())
    }

    /// Reserve slot, shared workspace lease, and all named resources as one state mutation.
    pub fn reserve_ready(&mut self, configured_cap: usize) -> MatrixResult<Vec<MatrixAssignment>> {
        if !matches!(self.snapshot.lifecycle, MatrixLifecycle::Running | MatrixLifecycle::Verifying) {
            return Ok(Vec::new());
        }
        let cap = configured_cap.min(5);
        let phase = if self.snapshot.lifecycle == MatrixLifecycle::Verifying {
            MatrixPhase::Verify
        } else {
            MatrixPhase::Execute
        };
        let mut assignments = Vec::new();
        for index in 0..self.snapshot.tasks.len() {
            let active = self.active_assignments();
            if active.len() >= cap {
                break;
            }
            let task = self
                .snapshot
                .spec
                .tasks
                .get(index)
                .ok_or_else(|| MatrixError("unknown task specification".into()))?;
            let state = self
                .snapshot
                .tasks
                .get(index)
                .ok_or_else(|| MatrixError("unknown task index".into()))?;
            let ready = match phase {
                MatrixPhase::Execute => {
                    state.status == MatrixTaskStatus::Queued
                        && task.dependencies.iter().all(|id| {
                            self.snapshot.tasks.iter().any(|state| {
                                &state.id == id
                                    && matches!(state.status, MatrixTaskStatus::Executed | MatrixTaskStatus::Verified)
                            })
                        })
                }
                MatrixPhase::Verify => state.status == MatrixTaskStatus::Executed,
            };
            if !ready {
                continue;
            }
            let mut usage = BTreeMap::<&str, u64>::new();
            let mut blocked = false;
            for assignment in &active {
                let Some(owner) = self.snapshot.spec.tasks.iter().find(|task| task.id == assignment.task_id) else {
                    return Err(MatrixError("unknown active task".into()));
                };
                if owner.access == WorkspaceAccess::Write || task.access == WorkspaceAccess::Write {
                    blocked = true;
                }
                for (resource, count) in &owner.resources {
                    let used = usage.entry(resource).or_default();
                    *used += u64::from(*count);
                }
            }
            if blocked
                || task.resources.iter().any(|(resource, count)| {
                    self.snapshot.spec.resources.get(resource).is_none_or(|capacity| {
                        usage.get(resource.as_str()).copied().unwrap_or(0) + u64::from(*count) > u64::from(*capacity)
                    })
                })
            {
                continue;
            }
            let assignment = MatrixAssignment {
                task_id: task.id.clone(),
                attempt_id: uuid::Uuid::new_v4().to_string(),
                worker_id: uuid::Uuid::new_v4().to_string(),
                phase,
                generation: self.snapshot.generation.clone(),
            };
            let state = self
                .snapshot
                .tasks
                .get_mut(index)
                .ok_or_else(|| MatrixError("unknown task index".into()))?;
            state.status = MatrixTaskStatus::Assigned;
            state.attempts.push(MatrixAttempt {
                id: assignment.attempt_id.clone(),
                worker_id: assignment.worker_id.clone(),
                phase,
                generation: assignment.generation.clone(),
                launch_requested: false,
                outcome: None,
                cleanup_confirmed: false,
                evidence: Vec::new(),
            });
            assignments.push(assignment);
        }
        if !assignments.is_empty() {
            self.changed();
        }
        Ok(assignments)
    }

    /// Persist this marker before invoking a process or worker launch.
    pub fn mark_launch_requested(&mut self, attempt_id: &str) -> MatrixResult<()> {
        let attempt = self
            .snapshot
            .tasks
            .iter_mut()
            .flat_map(|task| task.attempts.iter_mut())
            .find(|attempt| attempt.id == attempt_id)
            .ok_or_else(|| MatrixError("unknown launch attempt".into()))?;
        require(attempt.outcome.is_none() && !attempt.launch_requested, "launch was already requested")?;
        attempt.launch_requested = true;
        self.changed();
        Ok(())
    }

    /// Release a launch reservation after launch failure and confirmed cleanup.
    pub fn rollback_launch(&mut self, attempt_id: &str) -> MatrixResult<()> {
        let task = self
            .snapshot
            .tasks
            .iter_mut()
            .find(|task| task.attempts.last().is_some_and(|attempt| attempt.id == attempt_id))
            .ok_or_else(|| MatrixError("unknown launch attempt".into()))?;
        let attempt = task
            .attempts
            .last_mut()
            .ok_or_else(|| MatrixError("missing launch attempt".into()))?;
        require(attempt.outcome.is_none(), "attempt already reported")?;
        attempt.cleanup_confirmed = true;
        attempt.outcome = Some(MatrixOutcome::Interrupted);
        task.status = if attempt.phase == MatrixPhase::Execute {
            MatrixTaskStatus::Queued
        } else {
            MatrixTaskStatus::Executed
        };
        self.changed();
        Ok(())
    }

    /// Runtime must resolve evidence IDs to owned completed canonical commands before calling.
    pub fn report(
        &mut self,
        attempt_id: &str,
        worker_id: &str,
        outcome: MatrixOutcome,
        evidence: Vec<MatrixCommandEvidence>,
        cleanup_confirmed: bool,
    ) -> MatrixResult<()> {
        let index = self
            .snapshot
            .tasks
            .iter()
            .position(|task| task.attempts.last().is_some_and(|attempt| attempt.id == attempt_id))
            .ok_or_else(|| MatrixError("stale or unknown report attempt".into()))?;
        let task = self
            .snapshot
            .tasks
            .get(index)
            .ok_or_else(|| MatrixError("unknown task index".into()))?;
        let attempt = task
            .attempts
            .last()
            .ok_or_else(|| MatrixError("missing report attempt".into()))?;
        require(
            attempt.worker_id == worker_id && attempt.outcome.is_none(),
            "duplicate report or wrong worker identity",
        )?;
        require(task.status == MatrixTaskStatus::Assigned, "task is not assigned")?;
        if outcome == MatrixOutcome::Success && attempt.phase == MatrixPhase::Verify {
            let generation = attempt
                .generation
                .as_deref()
                .ok_or_else(|| MatrixError("verification has no generation".into()))?;
            require(self.snapshot.generation.as_deref() == Some(generation), "stale verification generation")?;
            let checks = &self
                .snapshot
                .spec
                .tasks
                .get(index)
                .ok_or_else(|| MatrixError("unknown task specification".into()))?
                .checks;
            validate_evidence(checks, attempt, &evidence)?;
        }
        let phase = attempt.phase;
        let terminal = self.terminal();
        let task = self
            .snapshot
            .tasks
            .get_mut(index)
            .ok_or_else(|| MatrixError("unknown task index".into()))?;
        let attempt = task
            .attempts
            .last_mut()
            .ok_or_else(|| MatrixError("missing report attempt".into()))?;
        attempt.outcome = Some(outcome);
        attempt.cleanup_confirmed = cleanup_confirmed;
        attempt.evidence = evidence;
        task.status = if !cleanup_confirmed {
            MatrixTaskStatus::CleanupUncertain
        } else {
            match outcome {
                MatrixOutcome::Success => {
                    if phase == MatrixPhase::Execute {
                        MatrixTaskStatus::Executed
                    } else {
                        MatrixTaskStatus::Verified
                    }
                }
                MatrixOutcome::Interrupted => MatrixTaskStatus::Interrupted,
                MatrixOutcome::TimedOut => MatrixTaskStatus::TimedOut,
                _ => MatrixTaskStatus::Failed,
            }
        };
        if !terminal {
            if cleanup_confirmed
                && matches!(outcome, MatrixOutcome::Interrupted | MatrixOutcome::TimedOut)
                && self
                    .snapshot
                    .spec
                    .tasks
                    .get(index)
                    .ok_or_else(|| MatrixError("unknown task specification".into()))?
                    .replay_safe
                && task.automatic_retries == 0
            {
                task.automatic_retries += 1;
                task.status = if phase == MatrixPhase::Execute {
                    MatrixTaskStatus::Queued
                } else {
                    MatrixTaskStatus::Executed
                };
            } else if outcome != MatrixOutcome::Success || !cleanup_confirmed {
                self.snapshot.lifecycle = MatrixLifecycle::Blocked;
            }
            if self.has_blockers() {
                self.snapshot.lifecycle = MatrixLifecycle::Blocked;
            }
        }
        self.changed();
        Ok(())
    }

    /// Start complete final verification after every execution task finishes.
    pub fn begin_verification(&mut self, generation: String) -> MatrixResult<()> {
        require(
            self.snapshot.lifecycle == MatrixLifecycle::Running
                && self.active_assignments().is_empty()
                && self.snapshot.tasks.iter().all(|task| task.status == MatrixTaskStatus::Executed)
                && !generation.is_empty(),
            "execution must finish before final verification",
        )?;
        self.snapshot.generation = Some(generation);
        self.snapshot.lifecycle = MatrixLifecycle::Verifying;
        self.changed();
        Ok(())
    }
    /// The runtime fingerprints again after cleanup before committing success.
    pub fn finalize_verification(&mut self, generation: &str) -> MatrixResult<()> {
        require(
            self.snapshot.lifecycle == MatrixLifecycle::Verifying
                && self.snapshot.generation.as_deref() == Some(generation)
                && self.active_assignments().is_empty()
                && self.snapshot.tasks.iter().all(|task| task.status == MatrixTaskStatus::Verified),
            "verification is incomplete, paused, or its generation changed",
        )?;
        self.snapshot.lifecycle = MatrixLifecycle::Succeeded;
        self.changed();
        Ok(())
    }
    /// Explicit resume reconciles prepared assignments and blocks ambiguous survivors.
    pub fn recover(&mut self) -> MatrixResult<()> {
        let assignments = self.active_assignments();
        for assignment in assignments {
            let attempt = self
                .snapshot
                .tasks
                .iter()
                .flat_map(|task| &task.attempts)
                .find(|attempt| attempt.id == assignment.attempt_id)
                .ok_or_else(|| MatrixError("missing recovery attempt".into()))?;
            if !attempt.launch_requested {
                self.rollback_launch(&assignment.attempt_id)?;
            } else if attempt.outcome.is_none() {
                self.report(
                    &assignment.attempt_id,
                    &assignment.worker_id,
                    MatrixOutcome::Interrupted,
                    Vec::new(),
                    false,
                )?;
            } else if !self.terminal() {
                self.snapshot.lifecycle = MatrixLifecycle::Blocked;
                self.changed();
            }
        }
        Ok(())
    }
    /// Invalidate every verification result after a source generation change.
    pub fn invalidate_verification(&mut self, generation: String) -> MatrixResult<()> {
        require(
            !self.terminal()
                && !self.has_blockers()
                && self.snapshot.generation.is_some()
                && self.active_assignments().is_empty()
                && !generation.is_empty(),
            "stop owned work before changing verification generation",
        )?;
        for task in &mut self.snapshot.tasks {
            task.status = MatrixTaskStatus::Executed;
        }
        self.snapshot.generation = Some(generation);
        if self.snapshot.lifecycle != MatrixLifecycle::Paused {
            self.snapshot.lifecycle = MatrixLifecycle::Verifying;
        }
        self.changed();
        Ok(())
    }
    /// Retry a failed task after an explicit coordinator decision and cleanup.
    pub fn retry(&mut self, task_id: &str) -> MatrixResult<()> {
        require(!self.terminal(), "terminal matrix cannot retry")?;
        require(self.active_assignments().is_empty(), "wait for owned work to stop before retry")?;
        let task = self
            .snapshot
            .tasks
            .iter_mut()
            .find(|task| task.id == task_id)
            .ok_or_else(|| MatrixError("unknown task".into()))?;
        require(
            matches!(
                task.status,
                MatrixTaskStatus::Failed | MatrixTaskStatus::Interrupted | MatrixTaskStatus::TimedOut
            ) && task.attempts.iter().all(|attempt| attempt.cleanup_confirmed),
            "task cannot retry before confirmed cleanup",
        )?;
        task.status = MatrixTaskStatus::Queued;
        // An explicit retry delegates repairs before a fresh complete check phase.
        for task in &mut self.snapshot.tasks {
            if task.status == MatrixTaskStatus::Verified {
                task.status = MatrixTaskStatus::Executed;
            }
        }
        self.snapshot.generation = None;
        self.snapshot.lifecycle = MatrixLifecycle::Running;
        self.changed();
        Ok(())
    }
    /// Cleanup must be established by an ownership token, never a PID alone.
    pub fn confirm_cleanup(&mut self, attempt_id: &str, worker_id: &str) -> MatrixResult<()> {
        let index = self
            .snapshot
            .tasks
            .iter()
            .position(|task| task.attempts.last().is_some_and(|attempt| attempt.id == attempt_id))
            .ok_or_else(|| MatrixError("unknown cleanup attempt".into()))?;
        let terminal = self.terminal();
        let replay_safe = self
            .snapshot
            .spec
            .tasks
            .get(index)
            .ok_or_else(|| MatrixError("unknown task specification".into()))?
            .replay_safe;
        let task = self
            .snapshot
            .tasks
            .get_mut(index)
            .ok_or_else(|| MatrixError("unknown task index".into()))?;
        let attempt = task
            .attempts
            .last_mut()
            .ok_or_else(|| MatrixError("missing cleanup attempt".into()))?;
        require(attempt.worker_id == worker_id && attempt.outcome.is_some(), "cleanup identity or outcome missing")?;
        attempt.cleanup_confirmed = true;
        task.status = match attempt.outcome {
            Some(MatrixOutcome::Success) => {
                if attempt.phase == MatrixPhase::Execute {
                    MatrixTaskStatus::Executed
                } else {
                    MatrixTaskStatus::Verified
                }
            }
            Some(MatrixOutcome::Interrupted) => MatrixTaskStatus::Interrupted,
            Some(MatrixOutcome::TimedOut) => MatrixTaskStatus::TimedOut,
            _ => MatrixTaskStatus::Failed,
        };
        if !terminal
            && replay_safe
            && task.automatic_retries == 0
            && matches!(attempt.outcome, Some(MatrixOutcome::Interrupted | MatrixOutcome::TimedOut))
        {
            task.automatic_retries += 1;
            task.status = if attempt.phase == MatrixPhase::Execute {
                MatrixTaskStatus::Queued
            } else {
                MatrixTaskStatus::Executed
            };
            if !self.has_blockers() {
                self.snapshot.lifecycle = if self.snapshot.generation.is_some() {
                    MatrixLifecycle::Verifying
                } else {
                    MatrixLifecycle::Running
                };
            }
        }
        self.changed();
        Ok(())
    }

    fn has_blockers(&self) -> bool {
        self.snapshot.tasks.iter().any(|task| {
            matches!(
                task.status,
                MatrixTaskStatus::Failed
                    | MatrixTaskStatus::Interrupted
                    | MatrixTaskStatus::TimedOut
                    | MatrixTaskStatus::CleanupUncertain
            )
        })
    }
}

fn validate_evidence(
    checks: &[String],
    attempt: &MatrixAttempt,
    evidence: &[MatrixCommandEvidence],
) -> MatrixResult<()> {
    let generation = attempt
        .generation
        .as_deref()
        .ok_or_else(|| MatrixError("verification has no generation".into()))?;
    require(
        evidence.len() == checks.len()
            && checks.iter().zip(evidence).all(|(check, record)| {
                record.command == *check
                    && record.attempt_id == attempt.id
                    && record.worker_id == attempt.worker_id
                    && record.generation == generation
                    && !record.event_id.is_empty()
                    && record.exit_code == Some(0)
                    && !record.cancelled
            })
            && evidence.iter().map(|record| &record.event_id).collect::<BTreeSet<_>>().len() == evidence.len(),
        "missing, unrelated, duplicate, cancelled, failed, or stale verification evidence",
    )
}

/// Latest complete durable checkpoint for each matrix, in stable ID order.
pub fn replay(events: impl IntoIterator<Item = ThreadEvent>) -> MatrixResult<BTreeMap<String, MatrixState>> {
    let mut matrices = BTreeMap::<String, MatrixState>::new();
    for event in events {
        if let ThreadEvent::MatrixUpdated(snapshot) = event {
            if let Some(previous) = matrices.get(&snapshot.spec.id) {
                require(previous.snapshot.spec == snapshot.spec, "matrix specification changed after creation")?;
                require(snapshot.revision > previous.snapshot.revision, "matrix revision did not advance")?;
            }
            let state = MatrixState::from_snapshot(*snapshot)?;
            matrices.insert(state.snapshot.spec.id.clone(), state);
        }
    }
    Ok(matrices)
}

#[cfg(test)]
mod tests;
