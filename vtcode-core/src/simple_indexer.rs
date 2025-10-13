//! Simple file indexer using regex and markdown storage
//!
//! This module provides a simple, direct approach to code indexing and retrieval
//! using regex patterns and markdown files for storage. No complex embeddings
//! or databases - just direct file operations like a human using bash.

use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// Simple file index entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileIndex {
    /// File path
    pub path: String,
    /// File content hash for change detection
    pub hash: String,
    /// Last modified timestamp
    pub modified: u64,
    /// File size
    pub size: u64,
    /// Language/extension
    pub language: String,
    /// Simple tags
    pub tags: Vec<String>,
}

/// Simple search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub file_path: String,
    pub line_number: usize,
    pub line_content: String,
    pub matches: Vec<String>,
}

/// Sink that receives indexed file metadata.
pub trait IndexSink: Send + Sync {
    /// Prepare the sink so it can receive entries (e.g., create directories).
    fn prepare(&self) -> Result<()>;

    /// Persist a [`FileIndex`] entry.
    fn persist(&self, entry: &FileIndex) -> Result<()>;
}

/// Options that control how the [`SimpleIndexer`] walks directories and stores results.
pub struct SimpleIndexerOptions {
    index_directory: Option<PathBuf>,
    ignore_hidden_directories: bool,
    ignored_directory_names: Vec<String>,
    sink: Option<Arc<dyn IndexSink>>,
}

impl Default for SimpleIndexerOptions {
    fn default() -> Self {
        Self {
            index_directory: None,
            ignore_hidden_directories: true,
            ignored_directory_names: vec!["target".into(), "node_modules".into()],
            sink: None,
        }
    }
}

impl Clone for SimpleIndexerOptions {
    fn clone(&self) -> Self {
        Self {
            index_directory: self.index_directory.clone(),
            ignore_hidden_directories: self.ignore_hidden_directories,
            ignored_directory_names: self.ignored_directory_names.clone(),
            sink: self.sink.as_ref().map(Arc::clone),
        }
    }
}

impl SimpleIndexerOptions {
    /// Create default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the directory used by the default markdown sink.
    pub fn with_index_directory(mut self, path: PathBuf) -> Self {
        self.index_directory = Some(path);
        self
    }

    /// Include directories that start with a dot.
    pub fn include_hidden_directories(mut self) -> Self {
        self.ignore_hidden_directories = false;
        self
    }

    /// Replace the ignored directory list with a custom set.
    pub fn set_ignored_directories<I, S>(mut self, directories: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.ignored_directory_names = directories.into_iter().map(Into::into).collect();
        self
    }

    /// Append an ignored directory name.
    pub fn add_ignored_directory(mut self, directory: impl Into<String>) -> Self {
        self.ignored_directory_names.push(directory.into());
        self
    }

    /// Use a custom sink implementation.
    pub fn with_sink(mut self, sink: Arc<dyn IndexSink>) -> Self {
        self.sink = Some(sink);
        self
    }

    fn ensure_directory(&mut self, default: PathBuf) {
        if self.index_directory.is_none() {
            self.index_directory = Some(default);
        }
    }

    fn sink(&self) -> Option<&Arc<dyn IndexSink>> {
        self.sink.as_ref()
    }
}

/// Sink implementation that writes each entry to a markdown file in a directory.
#[derive(Clone)]
pub struct MarkdownIndexSink {
    directory: PathBuf,
}

impl MarkdownIndexSink {
    /// Create a new markdown sink rooted at the given directory.
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    fn render_entry(&self, index: &FileIndex) -> String {
        format!(
            "# File Index: {}\n\n- **Path**: {}\n- **Hash**: {}\n- **Modified**: {}\n- **Size**: {} bytes\n- **Language**: {}\n- **Tags**: {}\n\n",
            index.path,
            index.path,
            index.hash,
            index.modified,
            index.size,
            index.language,
            index.tags.join(", ")
        )
    }

    fn entry_path(&self, index: &FileIndex) -> PathBuf {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        index.path.hash(&mut hasher);
        let file_name = format!("{:x}.md", hasher.finish());
        self.directory.join(file_name)
    }
}

impl IndexSink for MarkdownIndexSink {
    fn prepare(&self) -> Result<()> {
        fs::create_dir_all(&self.directory)?;
        Ok(())
    }

    fn persist(&self, entry: &FileIndex) -> Result<()> {
        let markdown = self.render_entry(entry);
        let path = self.entry_path(entry);
        fs::write(path, markdown)?;
        Ok(())
    }
}

/// Simple file indexer
pub struct SimpleIndexer {
    /// Workspace root
    workspace_root: PathBuf,
    /// Configuration options
    options: SimpleIndexerOptions,
    /// Storage backend
    sink: Arc<dyn IndexSink>,
    /// In-memory index cache
    index_cache: HashMap<String, FileIndex>,
}

impl Clone for SimpleIndexer {
    fn clone(&self) -> Self {
        Self {
            workspace_root: self.workspace_root.clone(),
            options: self.options.clone(),
            sink: Arc::clone(&self.sink),
            index_cache: self.index_cache.clone(),
        }
    }
}

impl SimpleIndexer {
    /// Create a new simple indexer
    pub fn new(workspace_root: PathBuf) -> Self {
        Self::with_options(workspace_root, SimpleIndexerOptions::default())
    }

    /// Create an indexer with custom options.
    pub fn with_options(workspace_root: PathBuf, mut options: SimpleIndexerOptions) -> Self {
        let default_dir = workspace_root.join(".vtcode").join("index");
        options.ensure_directory(default_dir);

        let sink = options.sink().cloned().unwrap_or_else(|| {
            Arc::new(MarkdownIndexSink::new(
                options.index_directory.clone().unwrap(),
            ))
        });
        options.sink = Some(Arc::clone(&sink));

        Self {
            workspace_root,
            options,
            sink,
            index_cache: HashMap::new(),
        }
    }

    /// Initialize the index directory
    pub fn init(&self) -> Result<()> {
        self.sink.prepare()
    }

    /// Get the workspace root path
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Retrieve the resolved index directory, if available.
    pub fn index_store_path(&self) -> Option<&Path> {
        self.options.index_directory.as_deref()
    }

    /// Access the configuration options.
    pub fn options(&self) -> &SimpleIndexerOptions {
        &self.options
    }

    /// Index a single file
    pub fn index_file(&mut self, file_path: &Path) -> Result<()> {
        if !file_path.exists() || !file_path.is_file() {
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
        let hash = self.calculate_hash(&content);
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

        self.sink.persist(&index)?;

        Ok(())
    }

    /// Index all files in directory recursively
    pub fn index_directory(&mut self, dir_path: &Path) -> Result<()> {
        let mut file_paths = Vec::new();

        // First pass: collect all file paths
        self.walk_directory(dir_path, &mut |file_path| {
            file_paths.push(file_path.to_path_buf());
            Ok(())
        })?;

        // Second pass: index each file
        for file_path in file_paths {
            self.index_file(&file_path)?;
        }

        Ok(())
    }

    /// Search files using regex pattern
    pub fn search(&self, pattern: &str, path_filter: Option<&str>) -> Result<Vec<SearchResult>> {
        let regex = Regex::new(pattern)?;

        let mut results = Vec::new();

        // Search through indexed files
        for (file_path, _) in &self.index_cache {
            if let Some(filter) = path_filter {
                if !file_path.contains(filter) {
                    continue;
                }
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

    /// Find files by name pattern
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

    /// Get file content with line numbers
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

    /// List files in directory (like ls)
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

    /// Grep-like search (like grep command)
    pub fn grep(&self, pattern: &str, file_pattern: Option<&str>) -> Result<Vec<SearchResult>> {
        let regex = Regex::new(pattern)?;
        let mut results = Vec::new();

        for (file_path, _) in &self.index_cache {
            if let Some(fp) = file_pattern {
                if !file_path.contains(fp) {
                    continue;
                }
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

    // Helper methods

    fn walk_directory<F>(&mut self, dir_path: &Path, callback: &mut F) -> Result<()>
    where
        F: FnMut(&Path) -> Result<()>,
    {
        if !dir_path.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(dir_path)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if self.options.ignore_hidden_directories && name.starts_with('.') {
                        continue;
                    }
                    if self
                        .options
                        .ignored_directory_names
                        .iter()
                        .any(|ignored| ignored == name)
                    {
                        continue;
                    }
                }
                self.walk_directory(&path, callback)?;
            } else if path.is_file() {
                callback(&path)?;
            }
        }

        Ok(())
    }

    fn calculate_hash(&self, content: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        format!("{:x}", hasher.finish())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    #[derive(Default)]
    struct RecordingSink {
        entries: Mutex<Vec<FileIndex>>,
    }

    impl IndexSink for RecordingSink {
        fn prepare(&self) -> Result<()> {
            Ok(())
        }

        fn persist(&self, entry: &FileIndex) -> Result<()> {
            let mut entries = self.entries.lock().unwrap();
            entries.push(entry.clone());
            Ok(())
        }
    }

    #[test]
    fn init_creates_default_directory() {
        let temp = tempdir().expect("tempdir");
        let workspace = temp.path().to_path_buf();

        let indexer = SimpleIndexer::new(workspace.clone());
        indexer.init().expect("init succeeds");

        let expected = workspace.join(".vtcode").join("index");
        assert!(expected.exists(), "expected {:?} to exist", expected);
    }

    #[test]
    fn custom_sink_receives_entries_and_ignore_rules_apply() {
        let temp = tempdir().expect("tempdir");
        let workspace = temp.path().to_path_buf();
        let included = workspace.join("src");
        let ignored = workspace.join("target");
        let hidden = workspace.join(".git");

        fs::create_dir_all(&included).unwrap();
        fs::create_dir_all(&ignored).unwrap();
        fs::create_dir_all(&hidden).unwrap();

        let good_file = included.join("main.rs");
        fs::write(&good_file, "fn main() {}\n").unwrap();

        let ignored_file = ignored.join("lib.rs");
        fs::write(&ignored_file, "fn ignored() {}\n").unwrap();

        let hidden_file = hidden.join("secret");
        fs::write(&hidden_file, "classified\n").unwrap();

        let sink = Arc::new(RecordingSink::default());
        let options = SimpleIndexerOptions::default().with_sink(sink.clone());
        let mut indexer = SimpleIndexer::with_options(workspace.clone(), options);

        indexer
            .index_directory(workspace.as_path())
            .expect("indexing succeeds");

        let entries = sink.entries.lock().unwrap().clone();
        assert_eq!(entries.len(), 1, "only included file should be indexed");
        assert_eq!(entries[0].path, good_file.to_string_lossy());
    }
}
