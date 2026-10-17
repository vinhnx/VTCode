#![allow(
    missing_docs,
    clippy::expect_used,
    dead_code,
    unused_imports,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
#![expect(
    unused_results,
    clippy::let_underscore_must_use,
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::cast_possible_truncation,
    reason = "Skill parsing and template migration intentionally use validated text offsets, bounded identifiers, and side-effect-only registry updates."
)]
//! # vtcode-skills - Skill Types, Discovery, and Validation
//!
//! Provides the core skill system for VT Code including skill manifests,
//! validation, bundling, template rendering, and native plugin support.

pub mod authoring;
pub mod bundle;
pub mod command_skills;
pub mod container;
pub mod container_validation;
pub mod context_manager;
pub mod document_processor;
pub mod enhanced_validator;
pub mod file_references;
pub mod injection;
pub mod instructions;
pub mod locations;
pub mod manifest;
pub mod model;
pub mod native_plugin;
pub mod prompt_integration;
pub mod render;
pub mod system;
pub mod templates;
pub mod trust;
pub mod types;
pub mod validation_report;
pub mod versioning;
