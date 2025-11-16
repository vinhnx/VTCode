use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use vtcode_core::core::decision_tracker::{DecisionOutcome, DecisionTracker};

pub(crate) async fn record_outcome_for_decision(
    ledger: &Arc<RwLock<DecisionTracker>>,
    decision_id: &str,
    success: bool,
    err_message: Option<String>,
) -> Result<()> {
    let outcome = if success {
        DecisionOutcome::Success {
            result: "OK".to_string(),
            metrics: std::collections::HashMap::new(),
        }
    } else {
        DecisionOutcome::Failure {
            error: err_message.unwrap_or_else(|| "Tool failed".to_string()),
            recovery_attempts: 0,
            context_preserved: true,
        }
    };

    let mut guard = ledger.write().await;
    guard.record_outcome(decision_id, outcome);
    Ok(())
}
