//! Tree-sitter integration for Research-preview code parsing and analysis
//!
//! This module provides syntax-aware code understanding and manipulation capabilities
//! using tree-sitter parsers for multiple programming languages.
//!
//! ## Features
//!
//! - **Multi-language Support**: Rust, Python, JavaScript, TypeScript, Go, Java, Bash, optional Swift
//! - **Syntax Tree Analysis**: Parse code into structured syntax trees
//! - **Symbol Extraction**: Extract functions, classes, variables, and imports
//! - **Code Navigation**: Navigate code structures with precision
//! - **Semantic Analysis**: Understand code semantics beyond syntax
//! - **Refactoring Support**: Intelligent code manipulation capabilities

pub mod analysis;
pub mod analyzer;
pub mod cache;
pub mod highlighting;
pub mod languages;
pub mod navigation;
// TODO: parse_cache and unified_extractor have incompatible types - disabled for now
// pub mod parse_cache;
pub mod refactoring;
// pub mod unified_extractor;

pub use analysis::*;
pub use analyzer::*;
pub use cache::*;
pub use highlighting::*;
pub use languages::*;
pub use navigation::*;
// pub use parse_cache::*;
pub use refactoring::*;
// pub use unified_extractor::*;
