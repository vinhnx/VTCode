//! Reusable sandbox environment abstractions.
//!
//! This module packages the sandbox coordination logic used by VT Code into a
//! standalone, well-documented API that can be embedded in other projects.
//! It focuses on ergonomic configuration of sandbox permissions, event
//! logging, and runtime profile creation without relying on the surrounding
//! application state.
//!
//! # Features
//!
//! - Builder-based configuration for sandbox directories and metadata.
//! - Domain and filesystem allowlist management with workspace boundary
//!   enforcement.
//! - JSON settings generation compatible with Anthropic's sandbox runtime.
//! - Structured event logging helpers for auditing sandbox changes.
//!
//! # Example
//!
//! ```rust,no_run
//! use vtcode_core::sandbox::{SandboxEnvironment, SandboxRuntimeKind};
//! # use anyhow::Result;
//!
//! # fn main() -> Result<()> {
//! let mut environment = SandboxEnvironment::builder("./workspace")
//!     .sandbox_root("./.vtcode/sandbox")
//!     .runtime_kind(SandboxRuntimeKind::AnthropicSrt)
//!     .build();
//!
//! environment.allow_domain("example.com")?;
//! environment.allow_path("logs")?;
//! environment.write_settings()?;
//! let profile = environment.create_profile("/usr/local/bin/srt");
//! println!("Sandbox settings stored at {}", environment.settings_path().display());
//! # Ok(())
//! # }
//! ```

mod environment;
mod profile;
mod settings;

#[cfg(test)]
mod tests;

pub use environment::{
    DEFAULT_DENY_RULES, DomainAddition, DomainRemoval, PathAddition, PathRemoval,
    SandboxEnvironment, SandboxEnvironmentBuilder,
};
pub use profile::{SandboxProfile, SandboxRuntimeKind};
pub use settings::{
    SandboxNetworkPermissions, SandboxPermissions, SandboxRuntimeConfig, SandboxSettings,
};
