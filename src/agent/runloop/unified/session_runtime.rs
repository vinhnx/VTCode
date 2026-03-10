use anyhow::Result;
use async_trait::async_trait;

use vtcode_core::core::interfaces::session::{SessionRuntime, SessionRuntimeParams};

use crate::agent::runloop::ResumeSession;

use super::turn::run_single_agent_loop_unified;

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct UnifiedSessionRuntime;

#[async_trait]
impl SessionRuntime<ResumeSession> for UnifiedSessionRuntime {
    async fn run_session(&self, params: SessionRuntimeParams<'_, ResumeSession>) -> Result<()> {
        let runner = run_single_agent_loop_unified(
            params.agent_config,
            params.vt_config,
            params.skip_confirmations,
            params.full_auto,
            params.plan_mode,
            params.resume,
            params.steering_receiver.take(),
        );
        runner.await
    }
}
