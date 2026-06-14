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
pub mod types;
pub mod validation_report;
pub mod versioning;
