//! Bridge from eval-report JSON to RL reward signals.

use super::signal::RewardSignal;

/// Parse a [`RewardSignal`] from a single eval-report case object
/// (`evals/reports/*.json`). Returns `None` when the shape is unrecognized.
///
/// Eval reports (see `evals/README.md`) carry `latency` (seconds) and `grade`
/// (e.g. `"pass"` / `"fail"`), which map directly onto the RL reward signal so
/// benchmark runs can improve action selection without bespoke wiring.
#[must_use]
pub fn reward_from_eval_report(case: &serde_json::Value) -> Option<RewardSignal> {
    let success = match case.get("grade").and_then(serde_json::Value::as_str) {
        Some(g) => {
            let g = g.trim().to_ascii_lowercase();
            g == "pass" || g == "true" || g == "success" || g == "1"
        }
        None => case.get("success").and_then(serde_json::Value::as_bool).unwrap_or(false),
    };
    let latency_secs = case.get("latency").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
    let cost_usd = case
        .get("cost_usd")
        .and_then(serde_json::Value::as_f64)
        .or_else(|| {
            case.get("usage")
                .and_then(|u| u.get("cost_usd"))
                .and_then(serde_json::Value::as_f64)
        })
        .unwrap_or(0.0);
    Some(RewardSignal { success, latency_secs, cost_usd })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_grade_and_latency() {
        let case = json!({ "grade": "pass", "latency": 1.5, "cost_usd": 0.02 });
        let sig = reward_from_eval_report(&case).expect("signal");
        assert!(sig.success);
        assert!((sig.latency_secs - 1.5).abs() < f64::EPSILON);
        assert!(sig.score(0.5) > 0.0);
    }

    #[test]
    fn failure_shape() {
        let case = json!({ "grade": "fail", "latency": 10.0 });
        let sig = reward_from_eval_report(&case).expect("signal");
        assert!(!sig.success);
        assert!(sig.score(0.5) < 0.0);
    }

    #[test]
    fn nested_usage_cost() {
        let case = json!({ "success": true, "usage": { "cost_usd": 0.5 } });
        let sig = reward_from_eval_report(&case).expect("signal");
        assert!(sig.success);
        assert!((sig.cost_usd - 0.5).abs() < f64::EPSILON);
    }
}
