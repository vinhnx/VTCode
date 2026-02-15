//! Agent system for intelligent conversation management

pub mod bootstrap;
pub mod chat;
pub mod completion;

pub mod config;
pub mod conversation;
pub mod core;
pub mod display;
pub mod error_recovery;
pub mod events;
pub mod runner;
pub mod snapshots;
pub mod state;
pub mod stats;
pub mod steering;
pub mod task;
pub mod types;

// Re-export main types for convenience
pub use bootstrap::{AgentComponentBuilder, AgentComponentSet};

pub use types::*;
