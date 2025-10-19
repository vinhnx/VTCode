use anyhow::Result;
use async_trait::async_trait;

use crate::config::loader::VTCodeConfig;
use crate::config::types::AgentConfig as CoreAgentConfig;

/// Parameters required to launch an Agent Client Protocol adapter.
#[derive(Debug)]
pub struct AcpLaunchParams<'a> {
    pub agent_config: &'a CoreAgentConfig,
    pub runtime_config: &'a VTCodeConfig,
}

impl<'a> AcpLaunchParams<'a> {
    pub fn new(agent_config: &'a CoreAgentConfig, runtime_config: &'a VTCodeConfig) -> Self {
        Self {
            agent_config,
            runtime_config,
        }
    }
}

/// Interface for components that expose VTCode over the Agent Client Protocol.
#[async_trait(?Send)]
pub trait AcpClientAdapter: Send + Sync {
    async fn serve(&self, params: AcpLaunchParams<'_>) -> Result<()>;
}
