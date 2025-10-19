use anyhow::Result;
use async_trait::async_trait;

use crate::config::loader::VTCodeConfig;
use crate::config::types::AgentConfig as CoreAgentConfig;

/// Parameters passed to a [`TurnDriver`] implementation for executing a single turn.
#[derive(Debug)]
pub struct TurnDriverParams<'a, Resume> {
    pub agent_config: &'a CoreAgentConfig,
    pub vt_config: Option<VTCodeConfig>,
    pub skip_confirmations: bool,
    pub full_auto: bool,
    pub resume: Option<Resume>,
}

impl<'a, Resume> TurnDriverParams<'a, Resume> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        agent_config: &'a CoreAgentConfig,
        vt_config: Option<VTCodeConfig>,
        skip_confirmations: bool,
        full_auto: bool,
        resume: Option<Resume>,
    ) -> Self {
        Self {
            agent_config,
            vt_config,
            skip_confirmations,
            full_auto,
            resume,
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
            resume: map(self.resume),
        }
    }
}

/// Abstraction over the core turn-driving loop used by the CLI and ACP bridges.
#[async_trait]
pub trait TurnDriver<Resume>: Send + Sync {
    async fn drive_turn(&self, params: TurnDriverParams<'_, Resume>) -> Result<()>;
}
