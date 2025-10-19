use anyhow::Result;
use async_trait::async_trait;

use vtcode_core::core::interfaces::turn::{TurnDriver, TurnDriverParams};

use crate::agent::runloop::ResumeSession;

use super::turn::run_single_agent_loop_unified;

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct UnifiedTurnDriver;

#[async_trait]
impl TurnDriver<ResumeSession> for UnifiedTurnDriver {
    async fn drive_turn(&self, params: TurnDriverParams<'_, ResumeSession>) -> Result<()> {
        run_single_agent_loop_unified(
            params.agent_config,
            params.vt_config,
            params.skip_confirmations,
            params.full_auto,
            params.resume,
        )
        .await
    }
}
