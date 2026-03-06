use anyhow::Result;
use async_trait::async_trait;

use crate::config::loader::VTCodeConfig;
use crate::config::types::AgentConfig as CoreAgentConfig;
use crate::core::agent::steering::SteeringMessage;

/// High-level mode for an interactive agent session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionMode {
    Ask,
    Architect,
    #[default]
    Code,
}

impl SessionMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Architect => "architect",
            Self::Code => "code",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ask" => Some(Self::Ask),
            "architect" => Some(Self::Architect),
            "code" => Some(Self::Code),
            _ => None,
        }
    }
}

/// Parameters passed to a [`SessionRuntime`] implementation for executing an interactive session.
#[derive(Debug)]
pub struct SessionRuntimeParams<'a, Resume> {
    pub agent_config: &'a CoreAgentConfig,
    pub vt_config: Option<VTCodeConfig>,
    pub skip_confirmations: bool,
    pub full_auto: bool,
    pub plan_mode: bool,
    pub resume: Option<Resume>,
    pub steering_receiver: &'a mut Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
}

impl<'a, Resume> SessionRuntimeParams<'a, Resume> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        agent_config: &'a CoreAgentConfig,
        vt_config: Option<VTCodeConfig>,
        skip_confirmations: bool,
        full_auto: bool,
        plan_mode: bool,
        resume: Option<Resume>,
        steering_receiver: &'a mut Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
    ) -> Self {
        Self {
            agent_config,
            vt_config,
            skip_confirmations,
            full_auto,
            plan_mode,
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

#[cfg(test)]
mod tests {
    use super::SessionMode;

    #[test]
    fn session_mode_round_trip() {
        assert_eq!(
            SessionMode::parse(SessionMode::Ask.as_str()),
            Some(SessionMode::Ask)
        );
        assert_eq!(
            SessionMode::parse(SessionMode::Architect.as_str()),
            Some(SessionMode::Architect)
        );
        assert_eq!(
            SessionMode::parse(SessionMode::Code.as_str()),
            Some(SessionMode::Code)
        );
        assert_eq!(SessionMode::parse("unknown"), None);
    }
}
