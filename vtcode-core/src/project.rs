//! Simple project management using markdown storage
//!
//! This module provides simple project management capabilities using
//! markdown files for storage instead of complex database systems.

#[cfg(feature = "markdown-ledger-project")]
use crate::markdown_ledger::{ProjectData, ProjectStorage};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Simple project manager
#[cfg(feature = "markdown-ledger-project")]
#[derive(Clone)]
pub struct SimpleProjectManager {
    /// Project storage using markdown
    storage: ProjectStorage,
    /// Workspace root
    workspace_root: PathBuf,
}

#[cfg(feature = "markdown-ledger-project")]
impl SimpleProjectManager {
    /// Create a new simple project manager
    pub fn new(workspace_root: PathBuf) -> Self {
        let storage_dir = workspace_root.join(".vtcode").join("projects");
        let storage = ProjectStorage::new(storage_dir);

        Self {
            storage,
            workspace_root,
        }
    }

    /// Initialize the project manager
    pub fn init(&self) -> Result<()> {
        self.storage.init()
    }

    /// Create a new project
    pub fn create_project(&self, name: &str, description: Option<&str>) -> Result<()> {
        let mut project = ProjectData::new(name);
        project.description = description.map(|s| s.to_string());

        self.storage.save_project(&project)?;
        Ok(())
    }

    /// Load a project by name
    pub fn load_project(&self, name: &str) -> Result<ProjectData> {
        self.storage.load_project(name)
    }

    /// List all projects
    pub fn list_projects(&self) -> Result<Vec<String>> {
        self.storage.list_projects()
    }

    /// Delete a project
    pub fn delete_project(&self, name: &str) -> Result<()> {
        self.storage.delete_project(name)
    }

    /// Update project metadata
    pub fn update_project(&self, project: &ProjectData) -> Result<()> {
        self.storage.save_project(project)
    }

    /// Get project data directory
    pub fn project_data_dir(&self, project_name: &str) -> PathBuf {
        self.workspace_root
            .join(".vtcode")
            .join("projects")
            .join(project_name)
    }

    /// Get project config directory
    pub fn config_dir(&self, project_name: &str) -> PathBuf {
        self.project_data_dir(project_name).join("config")
    }

    /// Get project cache directory
    pub fn cache_dir(&self, project_name: &str) -> PathBuf {
        self.project_data_dir(project_name).join("cache")
    }

    /// Get workspace root
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Check if project exists
    pub fn project_exists(&self, name: &str) -> bool {
        self.storage
            .list_projects()
            .map(|projects| projects.contains(&name.to_string()))
            .unwrap_or(false)
    }

    /// Get project info as simple text
    pub fn get_project_info(&self, name: &str) -> Result<String> {
        let project = self.load_project(name)?;

        let mut info = format!("Project: {}\n", project.name);
        if let Some(desc) = &project.description {
            info.push_str(&format!("Description: {}\n", desc));
        }
        info.push_str(&format!("Version: {}\n", project.version));
        info.push_str(&format!("Tags: {}\n", project.tags.join(", ")));

        if !project.metadata.is_empty() {
            info.push_str("\nMetadata:\n");
            for (key, value) in &project.metadata {
                info.push_str(&format!("  {}: {}\n", key, value));
            }
        }

        Ok(info)
    }

    /// Simple project identification from current directory
    pub fn identify_current_project(&self) -> Result<String> {
        // Check for .vtcode-project file
        let project_file = self.workspace_root.join(".vtcode-project");
        if project_file.exists() {
            let content = std::fs::read_to_string(&project_file)?;
            return Ok(content.trim().to_string());
        }

        // Fallback to directory name
        self.workspace_root
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_string())
            .ok_or_else(|| anyhow::anyhow!("Could not determine project name from directory"))
    }

    /// Set current project
    pub fn set_current_project(&self, name: &str) -> Result<()> {
        let project_file = self.workspace_root.join(".vtcode-project");
        std::fs::write(project_file, name)?;
        Ok(())
    }
}

/// Simple cache using file system
pub struct SimpleCache {
    cache_dir: PathBuf,
}

impl SimpleCache {
    /// Create a new simple cache
    pub fn new(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }

    /// Initialize cache directory
    pub fn init(&self) -> Result<()> {
        std::fs::create_dir_all(&self.cache_dir)?;
        Ok(())
    }

    /// Store data in cache
    pub fn store(&self, key: &str, data: &str) -> Result<()> {
        let file_path = self.cache_dir.join(format!("{}.txt", key));
        std::fs::write(file_path, data)?;
        Ok(())
    }

    /// Load data from cache
    pub fn load(&self, key: &str) -> Result<String> {
        let file_path = self.cache_dir.join(format!("{}.txt", key));
        std::fs::read_to_string(file_path).with_context(|| format!("Cache key '{}' not found", key))
    }

    /// Check if cache entry exists
    pub fn exists(&self, key: &str) -> bool {
        let file_path = self.cache_dir.join(format!("{}.txt", key));
        file_path.exists()
    }

    /// Clear cache
    pub fn clear(&self) -> Result<()> {
        for entry in std::fs::read_dir(&self.cache_dir)? {
            let entry = entry?;
            if entry.path().is_file() {
                std::fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }

    /// List cache entries
    pub fn list(&self) -> Result<Vec<String>> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&self.cache_dir)? {
            let entry = entry?;
            if let Some(file_name) = entry.path().file_stem() {
                if let Some(name) = file_name.to_str() {
                    entries.push(name.to_string());
                }
            }
        }
        Ok(entries)
    }
}
