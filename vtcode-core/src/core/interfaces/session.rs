use anyhow::Result;
use async_trait::async_trait;

use crate::config::loader::VTCodeConfig;
use crate::config::types::AgentConfig as CoreAgentConfig;
use crate::core::agent::steering::SteeringMessage;

/// Source that triggered planning workflow entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlanningEntrySource {
    #[default]
    None,
    CliFlag,
    ConfigDefault,
    UserRequest,
}

impl PlanningEntrySource {
    pub const fn should_auto_enter(self) -> bool {
        matches!(self, Self::CliFlag)
    }

    pub const fn requires_startup_prompt(self) -> bool {
        matches!(self, Self::ConfigDefault)
    }
}

/// Parameters passed to a [`SessionRuntime`] implementation for executing an interactive session.
#[derive(Debug)]
pub struct SessionRuntimeParams<'a, Resume> {
    pub agent_config: &'a CoreAgentConfig,
    pub vt_config: Option<VTCodeConfig>,
    pub skip_confirmations: bool,
    pub full_auto: bool,
    pub primary_agent_explicitly_configured: bool,
    pub planning_entry_source: PlanningEntrySource,
    pub resume: Option<Resume>,
    pub steering_receiver: &'a mut Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
}

impl<'a, Resume> SessionRuntimeParams<'a, Resume> {
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        agent_config: &'a CoreAgentConfig,
        vt_config: Option<VTCodeConfig>,
        skip_confirmations: bool,
        full_auto: bool,
        primary_agent_explicitly_configured: bool,
        planning_entry_source: PlanningEntrySource,
        resume: Option<Resume>,
        steering_receiver: &'a mut Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
    ) -> Self {
        Self {
            agent_config,
            vt_config,
            skip_confirmations,
            full_auto,
            primary_agent_explicitly_configured,
            planning_entry_source,
            resume,
            steering_receiver,
        }
    }
}

/// Abstraction over an interactive session runtime used by the CLI and adapters.
#[async_trait]
pub trait SessionRuntime<Resume>: Send + Sync {
    async fn run_session(&self, params: SessionRuntimeParams<'_, Resume>) -> Result<()>;
}
