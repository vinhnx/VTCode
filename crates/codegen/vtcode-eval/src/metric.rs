use super::task::{EvalRunResult, RunOutcome};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalMetric {
    pub(crate) pass_at_k: f64,
    pub(crate) pass_all_k: f64,
    pub(crate) total_runs: u32,
    pub(crate) passed_runs: u32,
    pub(crate) task_id: String,
}

pub fn compute_metric(task_id: &str, results: &[EvalRunResult]) -> EvalMetric {
    let total = results.len() as u32;
    let passed = results.iter().filter(|r| r.outcome == RunOutcome::Pass).count() as u32;
    let pass_at_k = if total > 0 { passed as f64 / total as f64 } else { 0.0 };
    let pass_all_k = if total > 0 && passed == total { 1.0 } else { 0.0 };
    EvalMetric {
        pass_at_k,
        pass_all_k,
        total_runs: total,
        passed_runs: passed,
        task_id: task_id.into(),
    }
}

pub fn aggregate_metrics(metrics: &[EvalMetric]) -> EvalMetric {
    if metrics.is_empty() {
        return EvalMetric {
            pass_at_k: 0.0,
            pass_all_k: 0.0,
            total_runs: 0,
            passed_runs: 0,
            task_id: "aggregate".into(),
        };
    }
    let total_runs: u32 = metrics.iter().map(|m| m.total_runs).sum();
    let passed_runs: u32 = metrics.iter().map(|m| m.passed_runs).sum();
    let pass_at_k = if total_runs > 0 {
        passed_runs as f64 / total_runs as f64
    } else {
        0.0
    };
    let pass_all_k = if metrics.iter().all(|m| m.pass_all_k > 0.0) {
        1.0
    } else {
        0.0
    };
    EvalMetric {
        pass_at_k,
        pass_all_k,
        total_runs,
        passed_runs,
        task_id: "aggregate".into(),
    }
}

pub fn pass_at_k(results: &[EvalRunResult]) -> f64 {
    let total = results.len() as f64;
    if total == 0.0 {
        return 0.0;
    }
    let passed = results.iter().filter(|r| r.outcome == RunOutcome::Pass).count() as f64;
    passed / total
}

pub fn pass_all_k(results: &[EvalRunResult]) -> f64 {
    if results.is_empty() {
        return 0.0;
    }
    if results.iter().all(|r| r.outcome == RunOutcome::Pass) {
        1.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{EvalRunResult, RunOutcome};

    fn r(outcome: RunOutcome) -> EvalRunResult {
        EvalRunResult {
            task_id: "t".into(),
            outcome,
            error_message: None,
            duration_secs: 0.0,
            attempt: 1,
            cost_usd: None,
            transcript_path: None,
        }
    }

    #[test]
    fn compute_metric_pass_at_k() {
        let results = vec![r(RunOutcome::Pass), r(RunOutcome::Fail), r(RunOutcome::Error)];
        let m = compute_metric("t", &results);
        assert_eq!(m.total_runs, 3);
        assert_eq!(m.passed_runs, 1);
        assert!((m.pass_at_k - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(m.pass_all_k, 0.0);
    }

    #[test]
    fn compute_metric_all_pass() {
        let results = vec![r(RunOutcome::Pass), r(RunOutcome::Pass)];
        let m = compute_metric("t", &results);
        assert_eq!(m.pass_all_k, 1.0);
        assert!((m.pass_at_k - 1.0).abs() < 1e-9);
    }

    #[test]
    fn compute_metric_empty() {
        let m = compute_metric("t", &[]);
        assert_eq!(m.total_runs, 0);
        assert_eq!(m.pass_at_k, 0.0);
    }

    #[test]
    fn aggregate_combines_runs() {
        let a = EvalMetric {
            pass_at_k: 1.0,
            pass_all_k: 1.0,
            total_runs: 1,
            passed_runs: 1,
            task_id: "a".into(),
        };
        let b = EvalMetric {
            pass_at_k: 0.0,
            pass_all_k: 0.0,
            total_runs: 1,
            passed_runs: 0,
            task_id: "b".into(),
        };
        let agg = aggregate_metrics(&[a, b]);
        assert_eq!(agg.total_runs, 2);
        assert_eq!(agg.passed_runs, 1);
        assert!((agg.pass_at_k - 0.5).abs() < 1e-9);
        assert_eq!(agg.pass_all_k, 0.0);
    }

    #[test]
    fn aggregate_empty() {
        let agg = aggregate_metrics(&[]);
        assert_eq!(agg.total_runs, 0);
        assert_eq!(agg.pass_at_k, 0.0);
    }
}
