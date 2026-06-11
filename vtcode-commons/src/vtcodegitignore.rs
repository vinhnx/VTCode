//! .vtcodegitignore file pattern matching utilities
//!
//! Uses the `ignore` crate's gitignore parser for correct, battle-tested
//! pattern matching instead of hand-rolled glob conversion.

use anyhow::{Result, anyhow};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;

/// Represents a .vtcodegitignore file with pattern matching capabilities
#[derive(Debug, Clone)]
pub struct VTCodeGitignore {
    /// Root directory where .vtcodegitignore was found
    root_dir: PathBuf,
    /// Compiled gitignore matcher
    matcher: Gitignore,
    /// Whether the .vtcodegitignore file exists and was loaded
    loaded: bool,
}

impl VTCodeGitignore {
    /// Create a new VTCodeGitignore instance by looking for .vtcodegitignore in the current directory
    pub async fn new() -> Result<Self> {
        let current_dir = std::env::current_dir()
            .map_err(|e| anyhow!("Failed to get current directory: {e}"))?;

        Self::from_directory(&current_dir).await
    }

    /// Create a VTCodeGitignore instance from a specific directory
    pub async fn from_directory(root_dir: &Path) -> Result<Self> {
        let gitignore_path = root_dir.join(".vtcodegitignore");

        let mut loaded = false;
        let mut builder = GitignoreBuilder::new(root_dir);

        if gitignore_path.exists() {
            match Self::load_patterns(&gitignore_path, &mut builder).await {
                Ok(()) => {
                    loaded = true;
                }
                Err(e) => {
                    // Log warning but don't fail - just treat as no patterns
                    tracing::warn!("Failed to load .vtcodegitignore: {}", e);
                }
            }
        }

        let matcher = builder.build().unwrap_or_else(|_| {
            // Fallback to empty matcher on build error
            GitignoreBuilder::new(root_dir)
                .build()
                .expect("empty gitignore builder should always succeed")
        });

        Ok(Self {
            root_dir: root_dir.to_path_buf(),
            matcher,
            loaded,
        })
    }

    /// Load patterns from the .vtcodegitignore file into the builder
    async fn load_patterns(file_path: &Path, builder: &mut GitignoreBuilder) -> Result<()> {
        let content = fs::read_to_string(file_path)
            .await
            .map_err(|e| anyhow!("Failed to read .vtcodegitignore: {e}"))?;

        for (line_num, line) in content.lines().enumerate() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            builder.add_line(None, line).map_err(|e| {
                anyhow!(
                    "Invalid pattern on line {}: '{}': {}",
                    line_num + 1,
                    line,
                    e
                )
            })?;
        }

        Ok(())
    }

    /// Check if a file path should be excluded based on the .vtcodegitignore patterns
    pub fn should_exclude(&self, file_path: &Path) -> bool {
        if !self.loaded {
            return false;
        }

        // Convert to relative path from the root directory
        let relative_path = match file_path.strip_prefix(&self.root_dir) {
            Ok(rel) => rel,
            Err(_) => file_path,
        };

        self.matcher
            .matched_path_or_any_parents(relative_path, file_path.is_dir())
            .is_ignore()
    }

    /// Filter a list of file paths based on .vtcodegitignore patterns
    pub fn filter_paths(&self, paths: Vec<PathBuf>) -> Vec<PathBuf> {
        if !self.loaded {
            return paths;
        }

        paths
            .into_iter()
            .filter(|path| !self.should_exclude(path))
            .collect()
    }

    /// Check if the .vtcodegitignore file was loaded successfully
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Get the number of patterns loaded
    pub fn pattern_count(&self) -> usize {
        self.matcher.num_ignores() as usize
    }

    /// Get the root directory
    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }
}

impl Default for VTCodeGitignore {
    fn default() -> Self {
        let root_dir = PathBuf::new();
        let matcher = GitignoreBuilder::new(&root_dir)
            .build()
            .expect("empty gitignore builder should always succeed");
        Self {
            root_dir,
            matcher,
            loaded: false,
        }
    }
}

/// Global .vtcodegitignore instance for easy access
pub static VTCODE_GITIGNORE: once_cell::sync::Lazy<tokio::sync::RwLock<Arc<VTCodeGitignore>>> =
    once_cell::sync::Lazy::new(|| tokio::sync::RwLock::new(Arc::new(VTCodeGitignore::default())));

/// Initialize the global .vtcodegitignore instance
pub async fn initialize_vtcode_gitignore() -> Result<()> {
    let gitignore = VTCodeGitignore::new().await?;
    let mut global_gitignore = VTCODE_GITIGNORE.write().await;
    *global_gitignore = Arc::new(gitignore);
    Ok(())
}

/// Snapshot the global .vtcodegitignore instance.
pub async fn snapshot_global_vtcode_gitignore() -> Arc<VTCodeGitignore> {
    VTCODE_GITIGNORE.read().await.clone()
}

/// Check if a file should be excluded by the global .vtcodegitignore
pub async fn should_exclude_file(file_path: &Path) -> bool {
    let gitignore = snapshot_global_vtcode_gitignore().await;
    gitignore.should_exclude(file_path)
}

/// Filter paths using the global .vtcodegitignore
pub async fn filter_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let gitignore = snapshot_global_vtcode_gitignore().await;
    gitignore.filter_paths(paths)
}

/// Reload the global .vtcodegitignore from disk
pub async fn reload_vtcode_gitignore() -> Result<()> {
    initialize_vtcode_gitignore().await
}
