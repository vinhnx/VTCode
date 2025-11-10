//! Agent system for intelligent conversation management

pub mod bootstrap;
pub mod chat;

pub mod config;
pub mod conversation;
pub mod core;
pub mod events;
pub mod examples;

pub mod runner;
pub mod snapshots;
pub mod stats;
pub mod task;
pub mod types;

// Re-export main types for convenience
pub use bootstrap::{AgentComponentBuilder, AgentComponentSet};

pub use types::*;
