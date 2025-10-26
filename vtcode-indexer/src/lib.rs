//! Workspace-friendly file indexer extracted from VTCode.
//!
//! `vtcode-indexer` offers a lightweight alternative to heavyweight
//! search/indexing stacks. It recursively walks a workspace, computes
//! hashes, and stores per-file metadata in Markdown-friendly summaries
//! so changes remain easy to audit in git.

use anyhow::Result;
use ignore::WalkBuilder;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// Persistence backend for [`SimpleIndexer`].
pub trait IndexStorage: Send + Sync {
    /// Prepare any directories or resources required for persistence.
    fn init(&self, index_dir: &Path) -> Result<()>;

    /// Persist an indexed file entry.
    fn persist(&self, index_dir: &Path, entry: &FileIndex) -> Result<()>;
}

/// Directory traversal filter hook for [`SimpleIndexer`].
pub trait TraversalFilter: Send + Sync {
    /// Determine if the indexer should descend into the provided directory.
    fn should_descend(&self, path: &Path, config: &SimpleIndexerConfig) -> bool;

    /// Determine if the indexer should process the provided file.
    fn should_index_file(&self, path: &Path, config: &SimpleIndexerConfig) -> bool;
}

/// Markdown-backed [`IndexStorage`] implementation.
#[derive(Debug, Default, Clone)]
pub struct MarkdownIndexStorage;

impl IndexStorage for MarkdownIndexStorage {
    fn init(&self, index_dir: &Path) -> Result<()> {
        fs::create_dir_all(index_dir)?;
        Ok(())
    }

    fn persist(&self, index_dir: &Path, entry: &FileIndex) -> Result<()> {
        let file_name = format!("{}.md", calculate_hash(&entry.path));
        let index_path = index_dir.join(file_name);

        let markdown = format!(
            "# File Index: {}\n\n\
            - **Path**: {}\n\
            - **Hash**: {}\n\
            - **Modified**: {}\n\
            - **Size**: {} bytes\n\
            - **Language**: {}\n\
            - **Tags**: {}\n\n",
            entry.path,
            entry.path,
            entry.hash,
            entry.modified,
            entry.size,
            entry.language,
            entry.tags.join(", ")
        );

        fs::write(index_path, markdown)?;
        Ok(())
    }
}

/// Default traversal filter powered by [`SimpleIndexerConfig`].
#[derive(Debug, Default, Clone)]
pub struct ConfigTraversalFilter;

impl TraversalFilter for ConfigTraversalFilter {
    fn should_descend(&self, path: &Path, config: &SimpleIndexerConfig) -> bool {
        !should_skip_dir(path, config)
    }

    fn should_index_file(&self, path: &Path, config: &SimpleIndexerConfig) -> bool {
        if !path.is_file() {
            return false;
        }

        // Skip hidden files if config says so
        if config.ignore_hidden {
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if file_name.starts_with('.') {
                    return false;
                }
            }
        }

        // CRITICAL: Always skip sensitive files regardless of config
        if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
            let sensitive_files = [
                ".env",
                ".env.local",
                ".env.production",
                ".env.development",
                ".env.test",
                ".git",
                ".gitignore",
                ".DS_Store",
            ];

            if sensitive_files
                .iter()
                .any(|s| file_name == *s || file_name.starts_with(".env."))
            {
                return false;
            }
        }

        true
    }
}

/// Configuration for [`SimpleIndexer`].
#[derive(Clone, Debug)]
pub struct SimpleIndexerConfig {
    workspace_root: PathBuf,
    index_dir: PathBuf,
    ignore_hidden: bool,
    excluded_dirs: Vec<PathBuf>,
    allowed_dirs: Vec<PathBuf>,
}

impl SimpleIndexerConfig {
    /// Builds a configuration using VTCode's legacy layout as defaults.
    pub fn new(workspace_root: PathBuf) -> Self {
        let index_dir = workspace_root.join(".vtcode").join("index");
        let vtcode_dir = workspace_root.join(".vtcode");
        let external_dir = vtcode_dir.join("external");

        let mut excluded_dirs = vec![
            index_dir.clone(),
            vtcode_dir,
            workspace_root.join("target"),
            workspace_root.join("node_modules"),
        ];

        excluded_dirs.dedup();

        Self {
            workspace_root,
            index_dir,
            ignore_hidden: true,
            excluded_dirs,
            allowed_dirs: vec![external_dir],
        }
    }

    /// Updates the index directory used for persisted metadata.
    pub fn with_index_dir(mut self, index_dir: impl Into<PathBuf>) -> Self {
        let index_dir = index_dir.into();
        self.index_dir = index_dir.clone();
        self.push_unique_excluded(index_dir);
        self
    }

    /// Adds an allowed directory that should be indexed even if hidden or inside an excluded parent.
    pub fn add_allowed_dir(mut self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        if !self.allowed_dirs.iter().any(|existing| existing == &path) {
            self.allowed_dirs.push(path);
        }
        self
    }

    /// Adds an additional excluded directory to skip during traversal.
    pub fn add_excluded_dir(mut self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        self.push_unique_excluded(path);
        self
    }

    /// Toggles whether hidden directories (prefix `.`) are ignored.
    pub fn ignore_hidden(mut self, ignore_hidden: bool) -> Self {
        self.ignore_hidden = ignore_hidden;
        self
    }

    /// Workspace root accessor.
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Index directory accessor.
    pub fn index_dir(&self) -> &Path {
        &self.index_dir
    }

    fn push_unique_excluded(&mut self, path: PathBuf) {
        if !self.excluded_dirs.iter().any(|existing| existing == &path) {
            self.excluded_dirs.push(path);
        }
    }
}

/// Simple file index entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileIndex {
    /// File path.
    pub path: String,
    /// File content hash for change detection.
    pub hash: String,
    /// Last modified timestamp.
    pub modified: u64,
    /// File size.
    pub size: u64,
    /// Language/extension.
    pub language: String,
    /// Simple tags.
    pub tags: Vec<String>,
}

/// Simple search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub file_path: String,
    pub line_number: usize,
    pub line_content: String,
    pub matches: Vec<String>,
}

/// Simple file indexer.
pub struct SimpleIndexer {
    config: SimpleIndexerConfig,
    index_cache: HashMap<String, FileIndex>,
    storage: Arc<dyn IndexStorage>,
    filter: Arc<dyn TraversalFilter>,
}

impl SimpleIndexer {
    /// Create a new simple indexer with default VTCode paths.
    pub fn new(workspace_root: PathBuf) -> Self {
        Self::with_components(
            SimpleIndexerConfig::new(workspace_root),
            Arc::new(MarkdownIndexStorage),
            Arc::new(ConfigTraversalFilter),
        )
    }

    /// Create a simple indexer with the provided configuration.
    pub fn with_config(config: SimpleIndexerConfig) -> Self {
        Self::with_components(
            config,
            Arc::new(MarkdownIndexStorage),
            Arc::new(ConfigTraversalFilter),
        )
    }

    /// Create a new simple indexer using a custom index directory.
    pub fn with_index_dir(workspace_root: PathBuf, index_dir: PathBuf) -> Self {
        let config = SimpleIndexerConfig::new(workspace_root).with_index_dir(index_dir);
        Self::with_config(config)
    }

    /// Create an indexer with explicit storage and traversal filter implementations.
    pub fn with_components(
        config: SimpleIndexerConfig,
        storage: Arc<dyn IndexStorage>,
        filter: Arc<dyn TraversalFilter>,
    ) -> Self {
        Self {
            config,
            index_cache: HashMap::new(),
            storage,
            filter,
        }
    }

    /// Replace the storage backend used to persist index entries.
    pub fn with_storage(self, storage: Arc<dyn IndexStorage>) -> Self {
        Self { storage, ..self }
    }

    /// Replace the traversal filter used to decide which files and directories are indexed.
    pub fn with_filter(self, filter: Arc<dyn TraversalFilter>) -> Self {
        Self { filter, ..self }
    }

    /// Initialize the index directory.
    pub fn init(&self) -> Result<()> {
        self.storage.init(self.config.index_dir())
    }

    /// Get the workspace root path.
    pub fn workspace_root(&self) -> &Path {
        self.config.workspace_root()
    }

    /// Get the index directory used for persisted metadata.
    pub fn index_dir(&self) -> &Path {
        self.config.index_dir()
    }

    /// Index a single file.
    pub fn index_file(&mut self, file_path: &Path) -> Result<()> {
        if !file_path.exists() || !self.filter.should_index_file(file_path, &self.config) {
            return Ok(());
        }

        let content = match fs::read_to_string(file_path) {
            Ok(text) => text,
            Err(err) => {
                if err.kind() == ErrorKind::InvalidData {
                    return Ok(());
                }
                return Err(err.into());
            }
        };
        let hash = calculate_hash(&content);
        let modified = self.get_modified_time(file_path)?;
        let size = content.len() as u64;
        let language = self.detect_language(file_path);

        let index = FileIndex {
            path: file_path.to_string_lossy().to_string(),
            hash,
            modified,
            size,
            language,
            tags: vec![],
        };

        self.index_cache
            .insert(file_path.to_string_lossy().to_string(), index.clone());

        self.storage.persist(self.config.index_dir(), &index)?;

        Ok(())
    }

    /// Index all files in directory recursively.
    /// Respects .gitignore, .ignore, and other ignore files.
    /// SECURITY: Always skips hidden files and sensitive data (.env, .git, etc.)
    pub fn index_directory(&mut self, dir_path: &Path) -> Result<()> {
        let walker = WalkBuilder::new(dir_path)
            .hidden(true) // CRITICAL: Skip hidden files (.env, .git, etc.)
            .git_ignore(true) // Respect .gitignore
            .git_global(true) // Respect global gitignore
            .git_exclude(true) // Respect .git/info/exclude
            .ignore(true) // Respect .ignore files
            .parents(true) // Check parent directories for ignore files
            .build();

        for entry in walker.filter_map(|e| e.ok()) {
            let path = entry.path();

            // Only index files, not directories
            if entry.file_type().map_or(false, |ft| ft.is_file()) {
                // Additional check: skip if in excluded dirs
                let should_skip = self
                    .config
                    .excluded_dirs
                    .iter()
                    .any(|excluded| path.starts_with(excluded));

                if !should_skip && self.filter.should_index_file(path, &self.config) {
                    self.index_file(path)?;
                }
            }
        }

        Ok(())
    }

    /// Search files using regex pattern.
    pub fn search(&self, pattern: &str, path_filter: Option<&str>) -> Result<Vec<SearchResult>> {
        let regex = Regex::new(pattern)?;

        let mut results = Vec::new();

        // Search through indexed files.
        for file_path in self.index_cache.keys() {
            if path_filter.is_some_and(|filter| !file_path.contains(filter)) {
                continue;
            }

            if let Ok(content) = fs::read_to_string(file_path) {
                for (line_num, line) in content.lines().enumerate() {
                    if regex.is_match(line) {
                        let matches: Vec<String> = regex
                            .find_iter(line)
                            .map(|m| m.as_str().to_string())
                            .collect();

                        results.push(SearchResult {
                            file_path: file_path.clone(),
                            line_number: line_num + 1,
                            line_content: line.to_string(),
                            matches,
                        });
                    }
                }
            }
        }

        Ok(results)
    }

    /// Find files by name pattern.
    pub fn find_files(&self, pattern: &str) -> Result<Vec<String>> {
        let regex = Regex::new(pattern)?;
        let mut results = Vec::new();

        for file_path in self.index_cache.keys() {
            if regex.is_match(file_path) {
                results.push(file_path.clone());
            }
        }

        Ok(results)
    }

    /// Get all indexed files without pattern matching.
    /// This is more efficient than using find_files(".*").
    pub fn all_files(&self) -> Vec<String> {
        self.index_cache.keys().cloned().collect()
    }

    /// Get file content with line numbers.
    pub fn get_file_content(
        &self,
        file_path: &str,
        start_line: Option<usize>,
        end_line: Option<usize>,
    ) -> Result<String> {
        let content = fs::read_to_string(file_path)?;
        let lines: Vec<&str> = content.lines().collect();

        let start = start_line.unwrap_or(1).saturating_sub(1);
        let end = end_line.unwrap_or(lines.len());

        let selected_lines = &lines[start..end.min(lines.len())];

        let mut result = String::new();
        for (i, line) in selected_lines.iter().enumerate() {
            result.push_str(&format!("{}: {}\n", start + i + 1, line));
        }

        Ok(result)
    }

    /// List files in directory (like ls).
    pub fn list_files(&self, dir_path: &str, show_hidden: bool) -> Result<Vec<String>> {
        let path = Path::new(dir_path);
        if !path.exists() {
            return Ok(vec![]);
        }

        let mut files = Vec::new();

        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let file_name = entry.file_name().to_string_lossy().to_string();

            if !show_hidden && file_name.starts_with('.') {
                continue;
            }

            files.push(file_name);
        }

        Ok(files)
    }

    /// Grep-like search (like grep command).
    pub fn grep(&self, pattern: &str, file_pattern: Option<&str>) -> Result<Vec<SearchResult>> {
        let regex = Regex::new(pattern)?;
        let mut results = Vec::new();

        for file_path in self.index_cache.keys() {
            if file_pattern.is_some_and(|fp| !file_path.contains(fp)) {
                continue;
            }

            if let Ok(content) = fs::read_to_string(file_path) {
                for (line_num, line) in content.lines().enumerate() {
                    if regex.is_match(line) {
                        results.push(SearchResult {
                            file_path: file_path.clone(),
                            line_number: line_num + 1,
                            line_content: line.to_string(),
                            matches: vec![line.to_string()],
                        });
                    }
                }
            }
        }

        Ok(results)
    }

    #[allow(dead_code)]
    fn walk_directory<F>(&mut self, dir_path: &Path, callback: &mut F) -> Result<()>
    where
        F: FnMut(&Path) -> Result<()>,
    {
        if !dir_path.exists() {
            return Ok(());
        }

        self.walk_directory_internal(dir_path, callback)
    }

    #[allow(dead_code)]
    fn walk_directory_internal<F>(&mut self, dir_path: &Path, callback: &mut F) -> Result<()>
    where
        F: FnMut(&Path) -> Result<()>,
    {
        for entry in fs::read_dir(dir_path)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                if self.is_allowed_dir(&path) {
                    self.walk_directory_internal(&path, callback)?;
                    continue;
                }

                if !self.filter.should_descend(&path, &self.config) {
                    self.walk_allowed_descendants(&path, callback)?;
                    continue;
                }

                self.walk_directory_internal(&path, callback)?;
            } else if path.is_file() {
                callback(&path)?;
            }
        }

        Ok(())
    }

    #[allow(dead_code)]
    fn is_allowed_dir(&self, path: &Path) -> bool {
        self.config
            .allowed_dirs
            .iter()
            .any(|allowed| path.starts_with(allowed))
    }

    #[allow(dead_code)]
    fn walk_allowed_descendants<F>(&mut self, dir_path: &Path, callback: &mut F) -> Result<()>
    where
        F: FnMut(&Path) -> Result<()>,
    {
        let allowed_dirs = self.config.allowed_dirs.clone();
        for allowed in allowed_dirs {
            if allowed.starts_with(dir_path) && allowed.exists() {
                self.walk_directory_internal(&allowed, callback)?;
            }
        }
        Ok(())
    }

    fn get_modified_time(&self, file_path: &Path) -> Result<u64> {
        let metadata = fs::metadata(file_path)?;
        let modified = metadata.modified()?;
        Ok(modified.duration_since(SystemTime::UNIX_EPOCH)?.as_secs())
    }

    fn detect_language(&self, file_path: &Path) -> String {
        file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("unknown")
            .to_string()
    }
}

impl Clone for SimpleIndexer {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            index_cache: self.index_cache.clone(),
            storage: self.storage.clone(),
            filter: self.filter.clone(),
        }
    }
}

fn should_skip_dir(path: &Path, config: &SimpleIndexerConfig) -> bool {
    if config
        .allowed_dirs
        .iter()
        .any(|allowed| path.starts_with(allowed))
    {
        return false;
    }

    if config
        .excluded_dirs
        .iter()
        .any(|excluded| path.starts_with(excluded))
    {
        return true;
    }

    if config.ignore_hidden
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name_str| name_str.starts_with('.'))
    {
        return true;
    }

    false
}

fn calculate_hash(content: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    #[test]
    fn skips_hidden_directories_by_default() -> Result<()> {
        let temp = tempdir()?;
        let workspace = temp.path();
        let hidden_dir = workspace.join(".private");
        fs::create_dir_all(&hidden_dir)?;
        fs::write(hidden_dir.join("secret.txt"), "classified")?;

        let visible_dir = workspace.join("src");
        fs::create_dir_all(&visible_dir)?;
        fs::write(visible_dir.join("lib.rs"), "fn main() {}")?;

        let mut indexer = SimpleIndexer::new(workspace.to_path_buf());
        indexer.init()?;
        indexer.index_directory(workspace)?;

        assert!(indexer.find_files("secret\\.txt$")?.is_empty());
        assert!(!indexer.find_files("lib\\.rs$")?.is_empty());

        Ok(())
    }

    #[test]
    fn can_include_hidden_directories_when_configured() -> Result<()> {
        let temp = tempdir()?;
        let workspace = temp.path();
        let hidden_dir = workspace.join(".cache");
        fs::create_dir_all(&hidden_dir)?;
        fs::write(hidden_dir.join("data.log"), "details")?;

        let config = SimpleIndexerConfig::new(workspace.to_path_buf()).ignore_hidden(false);
        let mut indexer = SimpleIndexer::with_config(config);
        indexer.init()?;
        indexer.index_directory(workspace)?;

        let results = indexer.find_files("data\\.log$")?;
        assert_eq!(results.len(), 1);

        Ok(())
    }

    #[test]
    fn supports_custom_storage_backends() -> Result<()> {
        #[derive(Clone, Default)]
        struct MemoryStorage {
            records: Arc<Mutex<Vec<FileIndex>>>,
        }

        impl MemoryStorage {
            fn new(records: Arc<Mutex<Vec<FileIndex>>>) -> Self {
                Self { records }
            }
        }

        impl IndexStorage for MemoryStorage {
            fn init(&self, _index_dir: &Path) -> Result<()> {
                Ok(())
            }

            fn persist(&self, _index_dir: &Path, entry: &FileIndex) -> Result<()> {
                let mut guard = self.records.lock().expect("lock poisoned");
                guard.push(entry.clone());
                Ok(())
            }
        }

        let temp = tempdir()?;
        let workspace = temp.path();
        fs::write(workspace.join("notes.txt"), "remember this")?;

        let records: Arc<Mutex<Vec<FileIndex>>> = Arc::new(Mutex::new(Vec::new()));
        let storage = MemoryStorage::new(records.clone());

        let config = SimpleIndexerConfig::new(workspace.to_path_buf());
        let mut indexer = SimpleIndexer::with_config(config).with_storage(Arc::new(storage));
        indexer.init()?;
        indexer.index_directory(workspace)?;

        let entries = records.lock().expect("lock poisoned");
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].path,
            workspace.join("notes.txt").to_string_lossy().to_string()
        );

        Ok(())
    }

    #[test]
    fn custom_filters_can_skip_files() -> Result<()> {
        #[derive(Default)]
        struct SkipRustFilter {
            inner: ConfigTraversalFilter,
        }

        impl TraversalFilter for SkipRustFilter {
            fn should_descend(&self, path: &Path, config: &SimpleIndexerConfig) -> bool {
                self.inner.should_descend(path, config)
            }

            fn should_index_file(&self, path: &Path, config: &SimpleIndexerConfig) -> bool {
                if path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
                {
                    return false;
                }

                self.inner.should_index_file(path, config)
            }
        }

        let temp = tempdir()?;
        let workspace = temp.path();
        fs::write(workspace.join("lib.rs"), "fn main() {}")?;
        fs::write(workspace.join("README.md"), "# Notes")?;

        let config = SimpleIndexerConfig::new(workspace.to_path_buf());
        let mut indexer =
            SimpleIndexer::with_config(config).with_filter(Arc::new(SkipRustFilter::default()));
        indexer.init()?;
        indexer.index_directory(workspace)?;

        assert!(indexer.find_files("lib\\.rs$")?.is_empty());
        assert!(!indexer.find_files("README\\.md$")?.is_empty());

        Ok(())
    }
}
