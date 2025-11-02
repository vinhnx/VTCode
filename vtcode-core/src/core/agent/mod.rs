//! Agent system for intelligent conversation management

pub mod bootstrap;
pub mod chat;
pub mod compaction; // Legacy - will be replaced by new modules
pub mod config;
pub mod conversation;
pub mod core;
pub mod engine;
pub mod events;
pub mod examples;
pub mod intelligence;
pub mod runner;
pub mod semantic;
pub mod snapshots;
pub mod stats;
pub mod task;
pub mod types;

// Re-export main types for convenience
pub use bootstrap::{AgentComponentBuilder, AgentComponentSet};
pub use config::CompactionConfig;
pub use engine::CompactionEngine;
pub use semantic::SemanticAnalyzer;
pub use types::*;
