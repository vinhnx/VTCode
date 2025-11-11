//! # Core Agent Architecture
//!
//! This module contains the core components of the VTCode agent system,
//! implementing the main agent loop, context management, and supporting infrastructure.
//!
//! ## Architecture Overview
//!
//! The core system is built around several key components:
//!
//! - **Agent**: Main agent implementation with conversation management

//! - **Prompt Caching**: Strategic caching for improved response times
//! - **Decision Tracking**: Audit trail of agent decisions and actions
//! - **Error Recovery**: Intelligent error handling with context preservation
//! - **Timeout Detection**: Prevents runaway operations
//! - **Trajectory Management**: Session state and history tracking
//!
//! ## Key Components
//!
//! ### Agent System
//! ```rust,no_run
//! use vtcode_core::core::agent::core::Agent;
//! use vtcode_core::VTCodeConfig;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = VTCodeConfig::load()?;
//!     let agent = Agent::new(config).await?;
//!     agent.run().await?;
//!     Ok(())
//! }
//! ```
//!

//!
pub mod agent;

pub mod context_pruner;
pub mod decision_tracker;
pub mod error_recovery;
pub mod interfaces;
pub mod orchestrator_retry;
pub mod prompt_caching;
pub mod pruning_decisions;
pub mod router;
pub mod timeout_detector;
pub mod token_budget;
pub mod token_estimator;
pub mod trajectory;

// Re-export main types
pub use context_pruner::{
    ContextEfficiency, ContextPruner, MessageMetrics, RetentionDecision, SemanticScore,
};
pub use pruning_decisions::{
    PruningDecision, PruningDecisionLedger, PruningReport, RetentionChoice,
};
