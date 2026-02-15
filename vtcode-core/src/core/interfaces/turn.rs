use anyhow::Result;
use async_trait::async_trait;

use crate::config::loader::VTCodeConfig;
use crate::config::types::AgentConfig as CoreAgentConfig;
use crate::core::agent::steering::SteeringMessage;

/// Parameters passed to a [`TurnDriver`] implementation for executing a single turn.
#[derive(Debug)]
pub struct TurnDriverParams<'a, Resume> {
    pub agent_config: &'a CoreAgentConfig,
    pub vt_config: Option<VTCodeConfig>,
    pub skip_confirmations: bool,
    pub full_auto: bool,
    pub plan_mode: bool,
    pub team_context: Option<crate::agent_teams::TeamContext>,
    pub resume: Option<Resume>,
    pub steering_receiver: &'a mut Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
}

impl<'a, Resume> TurnDriverParams<'a, Resume> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        agent_config: &'a CoreAgentConfig,
        vt_config: Option<VTCodeConfig>,
        skip_confirmations: bool,
        full_auto: bool,
        plan_mode: bool,
        team_context: Option<crate::agent_teams::TeamContext>,
        resume: Option<Resume>,
        steering_receiver: &'a mut Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
    ) -> Self {
        Self {
            agent_config,
            vt_config,
            skip_confirmations,
            full_auto,
            plan_mode,
            team_context,
            resume,
            steering_receiver,
        }
    }

    pub fn map_resume<F, NextResume>(self, map: F) -> TurnDriverParams<'a, NextResume>
    where
        F: FnOnce(Option<Resume>) -> Option<NextResume>,
    {
        TurnDriverParams {
            agent_config: self.agent_config,
            vt_config: self.vt_config,
            skip_confirmations: self.skip_confirmations,
            full_auto: self.full_auto,
            plan_mode: self.plan_mode,
            team_context: self.team_context,
            resume: map(self.resume),
            steering_receiver: self.steering_receiver,
        }
    }
}

/// Abstraction over the core turn-driving loop used by the CLI and ACP bridges.
#[async_trait]
pub trait TurnDriver<Resume>: Send + Sync {
    async fn drive_turn(&self, params: TurnDriverParams<'_, Resume>) -> Result<()>;
}
