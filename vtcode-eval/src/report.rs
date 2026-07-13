use crate::{EvalMetric, task::EvalCategory};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TaskReport {
    pub task_id: String,
    pub category: String,
    pub metric: EvalMetric,
}

#[derive(Debug, Clone, Serialize)]
pub struct SuiteReport {
    pub suite_id: String,
    pub suite_name: String,
    pub task_reports: Vec<TaskReport>,
    pub aggregate: EvalMetric,
    pub capability_metrics: EvalMetric,
    pub regression_metrics: EvalMetric,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvalReport {
    pub generated_at: String,
    pub suites: Vec<SuiteReport>,
}

impl EvalReport {
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# Eval Report\n\n");
        for s in &self.suites {
            out.push_str(&format!("## {}\n\n", s.suite_name));
            out.push_str(&format!(
                "- Aggregate: pass@k={:.1}%\n",
                s.aggregate.pass_at_k * 100.0
            ));
            out.push_str("| Task | Category | pass@k | passed/total |\n");
            out.push_str("|------|----------|--------|-------------|\n");
            for t in &s.task_reports {
                out.push_str(&format!(
                    "| {} | {} | {:.1}% | {}/{} |\n",
                    t.task_id,
                    t.category.as_str(),
                    t.metric.pass_at_k * 100.0,
                    t.metric.passed_runs,
                    t.metric.total_runs
                ));
            }
            out.push('\n');
        }
        out
    }
}

pub fn build_task_report(
    task_id: &str,
    _name: &str,
    category: EvalCategory,
    metric: EvalMetric,
) -> TaskReport {
    TaskReport {
        task_id: task_id.into(),
        category: category.label().into(),
        metric,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metric::EvalMetric;
    use crate::task::EvalCategory;

    #[test]
    fn to_markdown_renders_tasks_and_aggregate() {
        let report = EvalReport {
            generated_at: "2026-01-01".into(),
            suites: vec![SuiteReport {
                suite_id: "s1".into(),
                suite_name: "demo".into(),
                task_reports: vec![TaskReport {
                    task_id: "t1".into(),
                    category: "Capability".into(),
                    metric: EvalMetric {
                        pass_at_k: 0.5,
                        pass_all_k: 0.0,
                        total_runs: 2,
                        passed_runs: 1,
                        task_id: "t1".into(),
                    },
                }],
                aggregate: EvalMetric {
                    pass_at_k: 0.5,
                    pass_all_k: 0.0,
                    total_runs: 2,
                    passed_runs: 1,
                    task_id: "aggregate".into(),
                },
                capability_metrics: EvalMetric {
                    pass_at_k: 0.5,
                    pass_all_k: 0.0,
                    total_runs: 2,
                    passed_runs: 1,
                    task_id: "cap".into(),
                },
                regression_metrics: EvalMetric {
                    pass_at_k: 0.0,
                    pass_all_k: 0.0,
                    total_runs: 0,
                    passed_runs: 0,
                    task_id: "reg".into(),
                },
            }],
        };
        let md = report.to_markdown();
        assert!(md.contains("# Eval Report"));
        assert!(md.contains("demo"));
        assert!(md.contains("t1"));
        assert!(md.contains("Capability"));
    }

    #[test]
    fn build_task_report_maps_category() {
        let tr = build_task_report(
            "t1",
            "name",
            EvalCategory::Regression,
            EvalMetric {
                pass_at_k: 1.0,
                pass_all_k: 1.0,
                total_runs: 1,
                passed_runs: 1,
                task_id: "t1".into(),
            },
        );
        assert_eq!(tr.task_id, "t1");
        assert_eq!(tr.category, "Regression");
    }
}
