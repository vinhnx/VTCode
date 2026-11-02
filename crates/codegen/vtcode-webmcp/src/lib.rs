//! Authenticated WebMCP bridge primitives for VT Code.
//!
//! The crate separates transport, pairing, workspace policy, and runtime
//! integration. This lets the interactive TUI provide an adapter that routes
//! through its existing permission pipeline while the standalone command uses
//! [`FilesystemWorkspace`].

#![deny(unsafe_code)]

/// Opt-in companion task interface; execution remains binary-owned.
pub mod companion;
/// Error types shared by the bridge and adapters.
pub mod error;
/// Bounded canonical runtime event replay and subscription hub.
pub mod event_hub;
/// Safe headless filesystem runtime adapter.
pub mod filesystem;
/// One-time pairing and revocation state.
pub mod pairing;
/// Browser/server WebSocket protocol types.
pub mod protocol;
/// Local stdio and authenticated remote MCP transports, read-only by default.
pub mod remote_mcp;
/// Runtime adapter traits and bridge result types.
pub mod runtime;
/// Axum WebSocket server and request dispatcher.
pub mod server;

pub use error::{Result, WebmcpError};
pub use event_hub::{EventHubConfig, EventHubSubscription, SequencedThreadEvent, WebmcpEventHub};
pub use filesystem::{FilesystemLimits, FilesystemWorkspace};
pub use pairing::{PairingDisplay, PairingManager, PairingSession};
pub use protocol::{BridgeRequest, BridgeResponse, BridgeSettings, FileChange, PROTOCOL_VERSION};
pub use remote_mcp::{
    FetchInput, FetchOutput, RemoteMcpEndpoint, RemoteMcpHandler, RemoteMcpServerConfig, SearchInput, SearchOutput,
    SearchResult,
};
pub use runtime::{RuntimeAdapter, RuntimeStatus};
pub use server::{WebmcpServer, WebmcpServerConfig};
