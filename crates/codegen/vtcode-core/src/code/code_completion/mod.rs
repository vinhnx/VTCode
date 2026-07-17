//! Code completion engine with modular architecture
//!
//! This module provides intelligent code completion with learning capabilities,
//! context analysis, and language-specific optimizations.

pub mod cache;
pub mod context;
pub mod engine;
pub mod languages;
pub mod learning;

// Re-export main types for backward compatibility
pub use cache::CompletionCache;
pub use context::{CompletionContext, ContextAnalyzer};
pub use engine::{CompletionEngine, CompletionKind, CompletionSuggestion};
pub use learning::{CompletionLearningData, LearningSystem};
