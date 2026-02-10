use super::AgentRunner;
use serde_json::Value;

impl AgentRunner {
    pub(super) async fn optimize_tool_result(&self, name: &str, result: Value) -> Value {
        let mut optimizer = self.context_optimizer.lock().await;
        optimizer.optimize_result(name, result).await
    }
}
