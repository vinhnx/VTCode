use super::AgentRunner;
use serde_json::Value;

impl AgentRunner {
    pub(super) async fn optimize_tool_result(&self, name: &str, result: Value) -> Value {
        let mut optimizer = {
            let mut opt_ref = self.context_optimizer.borrow_mut();
            std::mem::take(&mut *opt_ref)
        };

        let optimized = optimizer.optimize_result(name, result).await;

        let mut opt_ref = self.context_optimizer.borrow_mut();
        *opt_ref = optimizer;

        optimized
    }
}
