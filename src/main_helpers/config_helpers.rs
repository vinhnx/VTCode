// Created by Vinh Nguyen on 2026-04-26
// Copyright © 2024 Cho Tot. All rights reserved.

//! Shared helpers for loading workspace configuration with consistent error context.
//!
//! Every call to [`vtcode_config::loader::manager::ConfigManager::load_from_workspace`]
//! across the codebase produced a slightly different `.with_context(…)` string, making
//! log searches unreliable and hiding the workspace path in some variants. This module
//! centralises all loading paths behind a single function so the error message is
//! consistent and always includes the workspace path.

use std::path::Path;

use anyhow::{Context, Result};
use vtcode_config::ConfigManager;

/// Load the workspace configuration with a consistent, path-inclusive error context.
///
/// This is the single canonical wrapper around `ConfigManager::load_from_workspace`.
/// All call sites in `src/` should use this function instead of calling the manager
/// directly with an ad-hoc `.with_context(…)`.
///
/// # Errors
///
/// Returns an error if the configuration file cannot be read or parsed, with a
/// message in the form:
/// `"Failed to load VT Code configuration for workspace '<path>'"`
#[must_use = "loading the config has no effect unless the result is used"]
pub(crate) fn load_workspace_config(workspace: impl AsRef<Path>) -> Result<ConfigManager> {
    let workspace = workspace.as_ref();
    ConfigManager::load_from_workspace(workspace).with_context(|| {
        format!(
            "Failed to load VT Code configuration for workspace '{}'",
            workspace.display()
        )
    })
}
