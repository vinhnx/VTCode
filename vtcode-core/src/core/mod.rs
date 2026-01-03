//! # Core Agent Architecture
//!
//! This module contains the core components of the VT Code agent system,
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

pub mod context_optimizer;
pub mod decision_tracker;
pub mod error_recovery;
pub mod execution_context;
pub mod interfaces;
pub mod loop_detector;
pub mod memory_pool;
pub mod optimized_agent;
pub mod orchestrator_retry;
pub mod performance_profiler;
pub mod prompt_caching;
pub mod timeout_detector;
pub mod trajectory;

// Re-export main types
pub use context_optimizer::ContextOptimizer;
pub use memory_pool::{MemoryPool, global_pool};
pub use optimized_agent::{OptimizedAgentEngine, AgentState, AgentContext};
pub use performance_profiler::{PerformanceProfiler, BenchmarkResults, BenchmarkUtils};
pub use execution_context::ExecutionContext;
