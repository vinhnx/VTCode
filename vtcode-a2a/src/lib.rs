#![cfg_attr(test, allow(missing_docs))]
//! Agent2Agent (A2A) Protocol support for VT Code.

pub mod agent_card;
pub mod cli;
pub mod client;
pub mod errors;
pub mod rpc;
pub mod task_manager;
pub mod types;
pub mod webhook;

#[cfg(feature = "a2a-server")]
pub mod server;

/// Wait for a process shutdown signal and log listener errors.
#[cfg(feature = "a2a-server")]
pub async fn shutdown_signal_logged(context: &'static str) {
    let result = {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            let mut terminate =
                signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");
            tokio::select! {
                ctrl_c_result = tokio::signal::ctrl_c() => ctrl_c_result,
                _ = terminate.recv() => Ok(()),
            }
        }
        #[cfg(not(unix))]
        {
            tokio::signal::ctrl_c().await
        }
    };
    if let Err(err) = result {
        tracing::warn!("Failed to listen for {context} shutdown signal: {err}");
    }
}

// Re-exports for convenience
pub use agent_card::{AgentCapabilities, AgentCard, AgentProvider, AgentSkill};
pub use client::A2aClient;
pub use errors::{A2aError, A2aErrorCode, A2aResult};
pub use rpc::{
    JsonRpcError, JsonRpcRequest, JsonRpcResponse, SendStreamingMessageResponse, StreamingEvent,
    TaskPushNotificationConfig,
};
pub use task_manager::TaskManager;
pub use types::{Artifact, FileContent, Message, MessageRole, Part, Task, TaskState, TaskStatus};
pub use webhook::{WebhookError, WebhookNotifier};
