use std::sync::Arc;

use super::ToolRegistry;
use crate::subagents::SubagentController;

impl ToolRegistry {
    pub fn set_subagent_controller(&self, controller: Arc<SubagentController>) {
        if let Ok(mut slot) = self.subagent_controller.write() {
            *slot = Some(controller);
        }
    }

    pub fn subagent_controller(&self) -> Option<Arc<SubagentController>> {
        self.subagent_controller
            .read()
            .ok()
            .and_then(|slot| slot.clone())
    }

    pub fn has_subagent_controller(&self) -> bool {
        self.subagent_controller().is_some()
    }
}
