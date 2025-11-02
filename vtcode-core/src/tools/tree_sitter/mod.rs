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
pub mod highlighting;
pub mod languages;
pub mod navigation;
pub mod refactoring;

pub use analysis::*;
pub use analyzer::*;
pub use highlighting::*;
pub use languages::*;
pub use navigation::*;
pub use refactoring::*;
