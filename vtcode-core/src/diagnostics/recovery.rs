use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

/// Recovery action executed when a degradation is detected.
#[async_trait]
pub trait RecoveryAction: Send + Sync {
    async fn execute(&self) -> Result<()>;
    fn name(&self) -> &'static str;
}

/// Composite set of recovery actions.
#[derive(Default, Debug, Clone)]
pub struct RecoveryPlaybook {
    actions: Vec<Arc<dyn RecoveryAction>>,
}

impl RecoveryPlaybook {
    pub fn new(actions: Vec<Arc<dyn RecoveryAction>>) -> Self {
        Self { actions }
    }

    pub fn with_action(mut self, action: Arc<dyn RecoveryAction>) -> Self {
        self.actions.push(action);
        self
    }

    pub async fn execute_all(&self) -> Result<Vec<String>> {
        let mut results = Vec::new();
        for action in &self.actions {
            action.execute().await?;
            results.push(action.name().to_string());
        }
        Ok(results)
    }
}

/// Simple recovery action that records a label (for testing/demo).
pub struct LabeledAction {
    label: &'static str,
}

impl LabeledAction {
    pub fn new(label: &'static str) -> Self {
        Self { label }
    }
}

#[async_trait]
impl RecoveryAction for LabeledAction {
    async fn execute(&self) -> Result<()> {
        Ok(())
    }

    fn name(&self) -> &'static str {
        self.label
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn executes_all_actions() {
        let playbook = RecoveryPlaybook::default().with_action(Arc::new(LabeledAction::new("reset")));
        let executed = playbook.execute_all().await.expect("playbook should run");
        assert_eq!(executed, vec!["reset".to_string()]);
    }
}
