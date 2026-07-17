//! Code quality tools with modular architecture
//!
//! This module provides comprehensive code formatting, linting, and quality
//! assurance tools with language-specific implementations.

pub mod config;
pub mod formatting;
pub mod linting;
pub mod metrics;

// Re-export main types for backward compatibility
pub use config::{FormatConfig, LintConfig, LintSeverity};
pub use formatting::{FormatResult, FormattingOrchestrator};
pub use linting::{LintResult, LintingOrchestrator};
pub use metrics::{ComplexityAnalyzer, QualityMetrics};
