//! Executor trait boundary and pure eval orchestration.
//!
//! The [`EvalExecutor`] trait isolates the harness orchestration from the
//! concrete agent runner. This is the "interface guard rail": `run_suite`
//! depends only on the trait, so it can be unit-tested with a fake executor
//! and the production executor implementation can be swapped
//! without touching the orchestration logic.

use anyhow::Result;
use async_trait::async_trait;

use crate::task::EvalTask;
use crate::{
    EvalReport, EvalRunResult, EvalSuite, SuiteReport, aggregate_metrics, build_task_report,
    compute_metric,
};

/// Executes a single eval task attempt and returns the verified outcome.
///
/// Implementations own the full "execute this task" semantics — running the
/// agent and applying any environment verification (probes). Keeping this
/// behind a trait decouples the orchestration in [`run_suite`] from the
/// concrete runner, which makes the harness independently testable.
#[async_trait]
pub trait EvalExecutor: Send + Sync {
    /// Run one attempt of `task` and return the outcome.
    async fn execute_task(&self, task: &EvalTask) -> Result<EvalRunResult>;
}

/// Pure orchestration core: loop tasks × attempts through the executor,
/// compute per-task metrics, and assemble the report.
///
/// This function performs no file I/O, no config reads, and no trust checks —
/// those belong to the caller. It depends only on [`EvalExecutor`], which makes
/// it fully unit-testable with an in-memory fake (see `executor::tests`).
pub async fn run_suite(executor: &dyn EvalExecutor, suite: &EvalSuite) -> Result<EvalReport> {
    let mut all_task_reports = Vec::new();

    for task in &suite.tasks {
        let mut run_results = Vec::new();
        for _attempt in 1..=suite.attempts {
            let result = executor.execute_task(task).await?;
            run_results.push(result);
        }
        let metric = compute_metric(&task.id, &run_results);
        all_task_reports.push(build_task_report(&task.id, &task.name, task.category, metric));
    }

    let all_metrics: Vec<_> = all_task_reports.iter().map(|r| r.metric.clone()).collect();
    let cap_metrics: Vec<_> = all_task_reports
        .iter()
        .filter(|r| r.category == "Capability")
        .map(|r| r.metric.clone())
        .collect();
    let reg_metrics: Vec<_> = all_task_reports
        .iter()
        .filter(|r| r.category == "Regression")
        .map(|r| r.metric.clone())
        .collect();

    Ok(EvalReport {
        generated_at: chrono::Utc::now().to_rfc3339(),
        suites: vec![SuiteReport {
            suite_id: suite.id.clone(),
            suite_name: suite.name.clone(),
            task_reports: all_task_reports,
            aggregate: aggregate_metrics(&all_metrics),
            capability_metrics: aggregate_metrics(&cap_metrics),
            regression_metrics: aggregate_metrics(&reg_metrics),
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{EvalCategory, EvalRunResult, EvalTask, RunOutcome};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// In-memory fake executor for isolating `run_suite`.
    struct FakeExecutor {
        outcomes: Vec<RunOutcome>,
        calls: AtomicUsize,
    }

    #[async_trait]
    impl EvalExecutor for FakeExecutor {
        async fn execute_task(&self, _task: &EvalTask) -> Result<EvalRunResult> {
            let i = self.calls.fetch_add(1, Ordering::SeqCst);
            let outcome = self.outcomes[i % self.outcomes.len()];
            Ok(EvalRunResult {
                task_id: _task.id.clone(),
                outcome,
                error_message: None,
                duration_secs: 0.0,
                attempt: (i + 1) as u32,
                cost_usd: None,
                transcript_path: None,
            })
        }
    }

    fn suite(attempts: u32) -> EvalSuite {
        EvalSuite {
            id: "s1".into(),
            name: "demo".into(),
            tasks: vec![
                EvalTask {
                    id: "t1".into(),
                    name: "t1".into(),
                    category: EvalCategory::Capability,
                    prompt: "p".into(),
                    verify_commands: vec![],
                    timeout_secs: None,
                },
                EvalTask {
                    id: "t2".into(),
                    name: "t2".into(),
                    category: EvalCategory::Regression,
                    prompt: "p".into(),
                    verify_commands: vec![],
                    timeout_secs: None,
                },
            ],
            attempts,
        }
    }

    #[tokio::test]
    async fn run_suite_aggregates_capability_and_regression() {
        // t1 gets [Pass, Fail]; t2 gets [Pass, Pass]
        let exec = FakeExecutor {
            outcomes: vec![
                RunOutcome::Pass,
                RunOutcome::Fail,
                RunOutcome::Pass,
                RunOutcome::Pass,
            ],
            calls: AtomicUsize::new(0),
        };
        let report = run_suite(&exec, &suite(2)).await.unwrap();
        let s = &report.suites[0];
        // t1: 1/2 pass, t2: 2/2 pass
        assert_eq!(s.aggregate.passed_runs, 3);
        assert_eq!(s.aggregate.total_runs, 4);
        assert!((s.capability_metrics.pass_at_k - 0.5).abs() < 1e-9);
        assert!((s.regression_metrics.pass_at_k - 1.0).abs() < 1e-9);
        assert!(s.capability_metrics.pass_all_k < 1.0);
        assert_eq!(s.regression_metrics.pass_all_k, 1.0);
    }
}
