//! Dynamic Context Discovery Initialization
//!
//! This module handles initialization of dynamic context discovery directories
//! and index files at agent startup.

use anyhow::Result;
use std::path::Path;
use tokio::fs;
use tracing::{debug, info, warn};

/// Directory structure for dynamic context discovery
pub struct DynamicContextDirs {
    /// Root .vtcode directory
    pub vtcode_dir: std::path::PathBuf,
    /// Tool output spool directory
    pub tool_outputs: std::path::PathBuf,
    /// Conversation history directory
    pub history: std::path::PathBuf,
    /// MCP tools directory
    pub mcp_tools: std::path::PathBuf,
    /// Terminal sessions directory
    pub terminals: std::path::PathBuf,
    /// Skills directory
    pub skills: std::path::PathBuf,
}

impl DynamicContextDirs {
    /// Create directory structure from workspace root
    pub fn from_workspace(workspace: &Path) -> Self {
        let vtcode_dir = workspace.join(".vtcode");
        Self {
            tool_outputs: vtcode_dir.join("context").join("tool_outputs"),
            history: vtcode_dir.join("history"),
            mcp_tools: vtcode_dir.join("mcp").join("tools"),
            terminals: vtcode_dir.join("terminals"),
            skills: vtcode_dir.join("skills"),
            vtcode_dir,
        }
    }

    /// Get all directories as a slice
    pub fn all_dirs(&self) -> Vec<&std::path::PathBuf> {
        vec![
            &self.tool_outputs,
            &self.history,
            &self.mcp_tools,
            &self.terminals,
            &self.skills,
        ]
    }
}

/// Initialize dynamic context discovery directories and index files
///
/// This should be called at agent startup when dynamic context is enabled.
pub async fn initialize_dynamic_context(
    workspace: &Path,
    config: &vtcode_config::DynamicContextConfig,
) -> Result<DynamicContextDirs> {
    if !config.enabled {
        debug!("Dynamic context discovery is disabled, skipping initialization");
        return Ok(DynamicContextDirs::from_workspace(workspace));
    }

    let dirs = DynamicContextDirs::from_workspace(workspace);

    // Create all directories
    for dir in dirs.all_dirs() {
        if let Err(e) = fs::create_dir_all(dir).await {
            warn!(
                path = %dir.display(),
                error = %e,
                "Failed to create dynamic context directory"
            );
        } else {
            debug!(path = %dir.display(), "Created dynamic context directory");
        }
    }

    // Create README in .vtcode explaining the directory structure
    let readme_path = dirs.vtcode_dir.join("README.md");
    if !readme_path.exists() {
        let readme_content = generate_vtcode_readme();
        if let Err(e) = fs::write(&readme_path, &readme_content).await {
            warn!(error = %e, "Failed to create .vtcode/README.md");
        }
    }

    // Create initial index files
    if config.sync_skills {
        create_initial_skills_index(&dirs.skills).await;
    }
    if config.sync_terminals {
        create_initial_terminals_index(&dirs.terminals).await;
    }
    if config.sync_mcp_tools {
        create_initial_mcp_index(&dirs.mcp_tools).await;
    }

    info!(
        workspace = %workspace.display(),
        "Initialized dynamic context discovery directories"
    );

    Ok(dirs)
}

/// Generate README content for .vtcode directory
fn generate_vtcode_readme() -> String {
    r#"# VT Code Dynamic Context Directory

This directory contains dynamic context files for VT Code agent operations.

## Directory Structure

```
.vtcode/
  context/
    tool_outputs/     # Large tool outputs spooled to files
  history/            # Conversation history during summarization
  mcp/
    tools/            # MCP tool descriptions and schemas
    status.json       # MCP provider status
  skills/
    INDEX.md          # Available skills index
    {skill_name}/     # Individual skill directories
  terminals/
    INDEX.md          # Terminal sessions index
    {session_id}.txt  # Terminal session output
```

## Purpose

These files implement **dynamic context discovery** - a pattern where large outputs
are written to files instead of being truncated. This allows the agent to:

1. Retrieve full tool outputs on demand via `read_file`
2. Search through outputs using `grep_file`
3. Recover conversation details lost during summarization
4. Discover available skills and MCP tools efficiently

## Configuration

Configure in `vtcode.toml`:

```toml
[context.dynamic]
enabled = true
tool_output_threshold = 8192  # Bytes before spooling
sync_terminals = true
persist_history = true
sync_mcp_tools = true
sync_skills = true
```

---
*This directory is managed by VT Code. Files may be automatically created, updated, or cleaned up.*
"#
    .to_string()
}

/// Create initial skills INDEX.md
async fn create_initial_skills_index(skills_dir: &Path) {
    let index_path = skills_dir.join("INDEX.md");
    if index_path.exists() {
        return;
    }

    let content = r#"# Skills Index

This file lists all available skills for dynamic discovery.
Use `read_file` on individual skill directories for full documentation.

*No skills available yet.*

Create skills using the `save_skill` tool.

---
*Generated automatically. Do not edit manually.*
"#;

    if let Err(e) = fs::write(&index_path, content).await {
        warn!(error = %e, "Failed to create initial skills index");
    }
}

/// Create initial terminals INDEX.md
async fn create_initial_terminals_index(terminals_dir: &Path) {
    let index_path = terminals_dir.join("INDEX.md");
    if index_path.exists() {
        return;
    }

    let content = r#"# Terminal Sessions Index

This file lists all active terminal sessions for dynamic discovery.
Use `read_file` on individual session files for full output.

*No active terminal sessions.*

---
*Generated automatically. Do not edit manually.*
"#;

    if let Err(e) = fs::write(&index_path, content).await {
        warn!(error = %e, "Failed to create initial terminals index");
    }
}

/// Create initial MCP tools INDEX.md
async fn create_initial_mcp_index(mcp_tools_dir: &Path) {
    let index_path = mcp_tools_dir.join("INDEX.md");
    if index_path.exists() {
        return;
    }

    let content = r#"# MCP Tools Index

This file lists all available MCP tools for dynamic discovery.
Use `read_file` on individual tool files for full schema details.

*No MCP tools available.*

Configure MCP servers in `vtcode.toml` or `.mcp.json`.

---
*Generated automatically. Do not edit manually.*
"#;

    if let Err(e) = fs::write(&index_path, content).await {
        warn!(error = %e, "Failed to create initial MCP tools index");
    }
}

/// Check if dynamic context directories exist
pub fn check_dynamic_context_exists(workspace: &Path) -> bool {
    let dirs = DynamicContextDirs::from_workspace(workspace);
    dirs.vtcode_dir.exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_initialize_dynamic_context() {
        let temp = tempdir().unwrap();
        let config = vtcode_config::DynamicContextConfig::default();

        let dirs = initialize_dynamic_context(temp.path(), &config)
            .await
            .unwrap();

        assert!(dirs.vtcode_dir.exists());
        assert!(dirs.tool_outputs.exists());
        assert!(dirs.history.exists());
        assert!(dirs.skills.exists());
        assert!(dirs.terminals.exists());

        // Check README was created
        assert!(dirs.vtcode_dir.join("README.md").exists());

        // Check index files were created
        assert!(dirs.skills.join("INDEX.md").exists());
        assert!(dirs.terminals.join("INDEX.md").exists());
    }

    #[tokio::test]
    async fn test_disabled_skips_creation() {
        let temp = tempdir().unwrap();
        let mut config = vtcode_config::DynamicContextConfig::default();
        config.enabled = false;

        let dirs = initialize_dynamic_context(temp.path(), &config)
            .await
            .unwrap();

        // Directories should not be created when disabled
        assert!(!dirs.vtcode_dir.exists());
    }
}
