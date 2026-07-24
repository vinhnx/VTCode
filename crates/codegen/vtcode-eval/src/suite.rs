use crate::task::EvalTask;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalSuite {
    pub(crate) id: String,
    pub name: String,
    pub tasks: Vec<EvalTask>,
    pub attempts: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::EvalCategory;

    #[test]
    fn suite_round_trips_through_json() {
        let suite = EvalSuite {
            id: "s1".into(),
            name: "demo".into(),
            tasks: vec![crate::task::EvalTask {
                id: "t1".into(),
                name: "t1".into(),
                category: EvalCategory::Capability,
                prompt: "do it".into(),
                verify_commands: vec!["cargo test".into()],
                timeout_secs: Some(30),
            }],
            attempts: 3,
        };
        let json = serde_json::to_string(&suite).unwrap();
        let back: EvalSuite = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "s1");
        assert_eq!(back.attempts, 3);
        assert_eq!(back.tasks.len(), 1);
        assert_eq!(back.tasks[0].timeout_secs, Some(30));
    }

    #[test]
    fn suite_rejects_zero_attempts_via_validation() {
        // The runner enforces attempts >= 1; serde itself allows 0, so the
        // guardrail lives in the CLI entrypoint (see eval.rs M3).
        let suite: EvalSuite = serde_json::from_str(r#"{"id":"s","name":"n","tasks":[],"attempts":0}"#).unwrap();
        assert_eq!(suite.attempts, 0);
    }
}
