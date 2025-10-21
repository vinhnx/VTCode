//! Markdown-backed storage utilities extracted from VTCode.
//!
//! This crate provides lightweight persistence helpers that serialize
//! structured data into Markdown files with embedded JSON and YAML blocks.
//! It also exposes simple project and cache managers built on top of the
//! markdown storage abstraction so command-line tools can persist
//! human-readable state without requiring a database.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use fs2::FileExt;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Simple markdown storage manager
#[derive(Clone)]
pub struct MarkdownStorage {
    storage_dir: PathBuf,
}

impl MarkdownStorage {
    /// Create a new markdown storage instance rooted at `storage_dir`.
    pub fn new(storage_dir: PathBuf) -> Self {
        Self { storage_dir }
    }

    /// Initialize storage directory
    pub fn init(&self) -> Result<()> {
        fs::create_dir_all(&self.storage_dir)?;
        Ok(())
    }

    /// Store data as markdown
    pub fn store<T: Serialize>(&self, key: &str, data: &T, title: &str) -> Result<()> {
        let file_path = self.storage_dir.join(format!("{}.md", key));
        let markdown = self.serialize_to_markdown(data, title)?;
        write_with_lock(&file_path, markdown.as_bytes())
    }

    /// Load data from markdown
    pub fn load<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Result<T> {
        let file_path = self.storage_dir.join(format!("{}.md", key));
        let content = read_with_shared_lock(&file_path)?;
        self.deserialize_from_markdown(&content)
    }

    /// List all stored items
    pub fn list(&self) -> Result<Vec<String>> {
        let mut items = Vec::new();

        for entry in fs::read_dir(&self.storage_dir)? {
            let entry = entry?;
            if let Some(name) = entry
                .path()
                .file_stem()
                .and_then(|file_name| file_name.to_str())
            {
                items.push(name.to_string());
            }
        }

        Ok(items)
    }

    /// Delete stored item
    pub fn delete(&self, key: &str) -> Result<()> {
        let file_path = self.storage_dir.join(format!("{}.md", key));
        if file_path.exists() {
            // Try to obtain an exclusive lock before removing the file so
            // concurrent readers or writers can finish gracefully.
            if let Ok(file) = OpenOptions::new().read(true).write(true).open(&file_path) {
                let _ = file.lock_exclusive();
                // Explicit drop to release the lock prior to removal.
                drop(file);
            }

            // Removing a file that was concurrently deleted is not an error –
            // treat it as best-effort cleanup.
            match fs::remove_file(&file_path) {
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => {
                    return Err(err).with_context(|| {
                        format!("Failed to delete markdown file at {}", file_path.display())
                    });
                }
            }
        }
        Ok(())
    }

    /// Check if item exists
    pub fn exists(&self, key: &str) -> bool {
        let file_path = self.storage_dir.join(format!("{}.md", key));
        file_path.exists()
    }

    fn serialize_to_markdown<T: Serialize>(&self, data: &T, title: &str) -> Result<String> {
        let json = serde_json::to_string_pretty(data)?;
        let yaml = serde_yaml::to_string(data)?;

        let markdown = format!(
            "# {}\n\n\
            ## JSON\n\n\
            ```json\n\
            {}\n\
            ```\n\n\
            ## YAML\n\n\
            ```yaml\n\
            {}\n\
            ```\n\n\
            ## Raw Data\n\n\
            {}\n",
            title,
            json,
            yaml,
            self.format_raw_data(data)
        );

        Ok(markdown)
    }

    fn deserialize_from_markdown<T: for<'de> Deserialize<'de>>(&self, content: &str) -> Result<T> {
        if let Some(json_block) = self.extract_code_block(content, "json") {
            return serde_json::from_str(json_block).context("Failed to parse JSON from markdown");
        }

        if let Some(yaml_block) = self.extract_code_block(content, "yaml") {
            return serde_yaml::from_str(yaml_block).context("Failed to parse YAML from markdown");
        }

        Err(anyhow::anyhow!("No valid JSON or YAML found in markdown"))
    }

    fn extract_code_block<'a>(&self, content: &'a str, language: &str) -> Option<&'a str> {
        let start_pattern = format!("```{}", language);
        let end_pattern = "```";

        if let Some(start_idx) = content.find(&start_pattern) {
            let code_start = start_idx + start_pattern.len();
            if let Some(end_idx) = content[code_start..].find(end_pattern) {
                let code_end = code_start + end_idx;
                return Some(content[code_start..code_end].trim());
            }
        }

        None
    }

    fn format_raw_data<T: Serialize>(&self, data: &T) -> String {
        match serde_json::to_value(data) {
            Ok(serde_json::Value::Object(map)) => {
                let mut lines = Vec::new();
                for (key, value) in map {
                    lines.push(format!("- **{}**: {}", key, self.format_value(&value)));
                }
                lines.join("\n")
            }
            _ => "Complex data structure".to_string(),
        }
    }

    fn format_value(&self, value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::String(s) => format!("\"{}\"", s),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Array(arr) => format!("[{} items]", arr.len()),
            serde_json::Value::Object(obj) => format!("{{{} fields}}", obj.len()),
            serde_json::Value::Null => "null".to_string(),
        }
    }
}

fn write_with_lock(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to ensure parent directory exists for {}",
                path.display()
            )
        })?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .with_context(|| format!("Failed to open file at {}", path.display()))?;

    file.lock_exclusive()
        .with_context(|| format!("Failed to acquire exclusive lock for {}", path.display()))?;

    file.set_len(0).with_context(|| {
        format!(
            "Failed to truncate file at {} while holding exclusive lock",
            path.display()
        )
    })?;

    file.write_all(data).with_context(|| {
        format!(
            "Failed to write file content to {} while holding exclusive lock",
            path.display()
        )
    })?;

    file.sync_all().with_context(|| {
        format!(
            "Failed to sync file at {} after writing with exclusive lock",
            path.display()
        )
    })?;

    file.unlock()
        .with_context(|| format!("Failed to release exclusive lock for {}", path.display()))
}

fn read_with_shared_lock(path: &Path) -> Result<String> {
    let mut file = OpenOptions::new()
        .read(true)
        .open(path)
        .with_context(|| format!("Failed to open file at {}", path.display()))?;

    file.lock_shared()
        .with_context(|| format!("Failed to acquire shared lock for {}", path.display()))?;

    let mut content = String::new();
    file.read_to_string(&mut content).with_context(|| {
        format!(
            "Failed to read file content from {} while holding shared lock",
            path.display()
        )
    })?;

    file.unlock()
        .with_context(|| format!("Failed to release shared lock for {}", path.display()))?;

    Ok(content)
}

/// Simple key-value storage using markdown
#[cfg(feature = "kv")]
pub struct SimpleKVStorage {
    storage: MarkdownStorage,
}

#[cfg(feature = "kv")]
impl SimpleKVStorage {
    pub fn new(storage_dir: PathBuf) -> Self {
        Self {
            storage: MarkdownStorage::new(storage_dir),
        }
    }

    pub fn init(&self) -> Result<()> {
        self.storage.init()
    }

    pub fn put(&self, key: &str, value: &str) -> Result<()> {
        let data = IndexMap::from([("value".to_string(), value.to_string())]);
        self.storage
            .store(key, &data, &format!("Key-Value: {}", key))
    }

    pub fn get(&self, key: &str) -> Result<String> {
        let data: IndexMap<String, String> = self.storage.load(key)?;
        data.get("value")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Value not found for key: {}", key))
    }

    pub fn delete(&self, key: &str) -> Result<()> {
        self.storage.delete(key)
    }

    pub fn list_keys(&self) -> Result<Vec<String>> {
        self.storage.list()
    }
}

/// Simple project metadata storage
#[cfg(feature = "projects")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectData {
    pub name: String,
    pub description: Option<String>,
    pub version: String,
    pub tags: Vec<String>,
    pub metadata: IndexMap<String, String>,
}

#[cfg(feature = "projects")]
impl ProjectData {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            description: None,
            version: "1.0.0".to_string(),
            tags: vec![],
            metadata: IndexMap::new(),
        }
    }
}

/// Project storage using markdown
#[cfg(feature = "projects")]
#[derive(Clone)]
pub struct ProjectStorage {
    storage: MarkdownStorage,
}

#[cfg(feature = "projects")]
impl ProjectStorage {
    pub fn new(storage_dir: PathBuf) -> Self {
        Self {
            storage: MarkdownStorage::new(storage_dir),
        }
    }

    pub fn init(&self) -> Result<()> {
        self.storage.init()
    }

    pub fn save_project(&self, project: &ProjectData) -> Result<()> {
        self.storage.store(
            &project.name,
            project,
            &format!("Project: {}", project.name),
        )
    }

    pub fn load_project(&self, name: &str) -> Result<ProjectData> {
        self.storage.load(name)
    }

    pub fn list_projects(&self) -> Result<Vec<String>> {
        self.storage.list()
    }

    pub fn delete_project(&self, name: &str) -> Result<()> {
        self.storage.delete(name)
    }

    pub fn storage_dir(&self) -> &Path {
        &self.storage.storage_dir
    }
}

/// Simple project manager that orchestrates project metadata persistence.
#[cfg(feature = "projects")]
#[derive(Clone)]
pub struct SimpleProjectManager {
    storage: ProjectStorage,
    workspace_root: PathBuf,
    project_root: PathBuf,
}

#[cfg(feature = "projects")]
impl SimpleProjectManager {
    /// Construct a project manager that stores metadata under
    /// `<workspace_root>/.vtcode/projects`.
    pub fn new(workspace_root: PathBuf) -> Self {
        let project_root = workspace_root.join(".vtcode").join("projects");
        Self::with_project_root(workspace_root, project_root)
    }

    /// Construct a manager with a caller-supplied project storage root.
    pub fn with_project_root(workspace_root: PathBuf, project_root: PathBuf) -> Self {
        let storage = ProjectStorage::new(project_root.clone());
        Self {
            storage,
            workspace_root,
            project_root,
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
        self.project_root.join(project_name)
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

    /// Return the root directory backing project metadata.
    pub fn project_root(&self) -> &Path {
        &self.project_root
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
        let project_file = self.workspace_root.join(".vtcode-project");
        if project_file.exists() {
            let content = fs::read_to_string(&project_file)?;
            return Ok(content.trim().to_string());
        }

        self.workspace_root
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_string())
            .ok_or_else(|| anyhow::anyhow!("Could not determine project name from directory"))
    }

    /// Set current project
    pub fn set_current_project(&self, name: &str) -> Result<()> {
        let project_file = self.workspace_root.join(".vtcode-project");
        fs::write(project_file, name)?;
        Ok(())
    }
}

/// Simple cache using file system
#[cfg(feature = "cache")]
pub struct SimpleCache {
    cache_dir: PathBuf,
}

#[cfg(feature = "cache")]
impl SimpleCache {
    /// Create a new simple cache
    pub fn new(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }

    /// Initialize cache directory
    pub fn init(&self) -> Result<()> {
        fs::create_dir_all(&self.cache_dir)?;
        Ok(())
    }

    /// Store data in cache
    pub fn store(&self, key: &str, data: &str) -> Result<()> {
        let file_path = self.cache_dir.join(format!("{}.txt", key));
        write_with_lock(&file_path, data.as_bytes())
    }

    /// Load data from cache
    pub fn load(&self, key: &str) -> Result<String> {
        let file_path = self.cache_dir.join(format!("{}.txt", key));
        read_with_shared_lock(&file_path).map_err(|err| {
            if err
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io_err| io_err.kind() == std::io::ErrorKind::NotFound)
            {
                anyhow::anyhow!("Cache key '{}' not found", key)
            } else {
                err
            }
        })
    }

    /// Check if cache entry exists
    pub fn exists(&self, key: &str) -> bool {
        let file_path = self.cache_dir.join(format!("{}.txt", key));
        file_path.exists()
    }

    /// Clear cache
    pub fn clear(&self) -> Result<()> {
        for entry in fs::read_dir(&self.cache_dir)? {
            let entry = entry?;
            if entry.path().is_file() {
                fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }

    /// List cache entries
    pub fn list(&self) -> Result<Vec<String>> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(&self.cache_dir)? {
            let entry = entry?;
            if let Some(name) = entry
                .path()
                .file_stem()
                .and_then(|file_name| file_name.to_str())
            {
                entries.push(name.to_string());
            }
        }
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use tempfile::TempDir;

    #[test]
    fn markdown_storage_roundtrip() {
        let dir = TempDir::new().expect("temp dir");
        let storage = MarkdownStorage::new(dir.path().to_path_buf());
        storage.init().expect("init storage");

        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Sample {
            name: String,
            value: u32,
        }

        let data = Sample {
            name: "example".to_string(),
            value: 42,
        };

        storage
            .store("sample", &data, "Sample Data")
            .expect("store");
        let loaded: Sample = storage.load("sample").expect("load");
        assert_eq!(loaded, data);
    }

    #[test]
    #[serial]
    fn concurrent_writes_preserve_integrity() {
        let dir = TempDir::new().expect("temp dir");
        let storage = MarkdownStorage::new(dir.path().to_path_buf());
        storage.init().expect("init storage");

        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Sample {
            name: String,
            value: u32,
        }

        let barrier = Arc::new(Barrier::new(3));
        let shared = Arc::new(storage);
        let key = "concurrent";

        let mut handles = Vec::new();
        for (name, value) in [("first", 1u32), ("second", 2u32)] {
            let barrier = barrier.clone();
            let storage = shared.clone();
            let key = key.to_string();
            handles.push(thread::spawn(move || {
                let data = Sample {
                    name: name.to_string(),
                    value,
                };

                barrier.wait();
                storage
                    .store(&key, &data, "Concurrent Sample")
                    .expect("store concurrently");
            }));
        }

        // Release the worker threads at roughly the same time.
        barrier.wait();

        for handle in handles {
            handle.join().expect("join thread");
        }

        let final_value: Sample = shared
            .load(key)
            .expect("load value after concurrent writes");

        assert!(
            (final_value.name == "first" && final_value.value == 1)
                || (final_value.name == "second" && final_value.value == 2),
            "final value should match one of the writers, got {:?}",
            final_value
        );
    }
}
