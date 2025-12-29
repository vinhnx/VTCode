//! Agent2Agent (A2A) Protocol support for VT Code
//!
//! This module implements the [A2A Protocol](https://a2a-protocol.org), an open standard
//! enabling communication and interoperability between AI agents.
//!
//! ## Features
//!
//! - **Agent Discovery**: Via Agent Cards at `/.well-known/agent-card.json`
//! - **Task Lifecycle Management**: States like `submitted`, `working`, `completed`
//! - **Real-time Streaming**: Via Server-Sent Events (SSE)
//! - **Rich Content Types**: Text, file, and structured data parts
//!
//! ## Usage
//!
//! ```rust,ignore
//! use vtcode_core::a2a::{AgentCard, TaskManager, Message, Part};
//!
//! // Create an agent card
//! let card = AgentCard::new("vtcode-agent", "VT Code AI Agent", "1.0.0");
//!
//! // Create a task manager
//! let manager = TaskManager::new();
//! let task = manager.create_task(None).await;
//! ```

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
