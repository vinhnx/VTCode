//! Trace storage implementation for persisting Agent Trace records.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use vtcode_exec_events::trace::{TraceRecord, AGENT_TRACE_VERSION};

/// Default directory name for trace storage.
pub const TRACES_DIR: &str = "traces";

/// Trace storage for reading and writing Agent Trace records.
#[derive(Debug, Clone)]
pub struct TraceStore {
    /// Base directory for trace storage (usually `.vtcode/traces/`).
    base_dir: PathBuf,
}

impl TraceStore {
    /// Create a new trace store at the specified base directory.
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    /// Create a trace store under the `.vtcode` directory in the workspace.
    pub fn for_workspace(workspace_path: impl AsRef<Path>) -> Self {
        let base_dir = workspace_path.as_ref().join(".vtcode").join(TRACES_DIR);
        Self::new(base_dir)
    }

    /// Get the base directory for trace storage.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Ensure the trace storage directory exists.
    pub fn ensure_dir(&self) -> Result<()> {
        if !self.base_dir.exists() {
            fs::create_dir_all(&self.base_dir)
                .with_context(|| format!("Failed to create trace directory: {:?}", self.base_dir))?;
        }
        Ok(())
    }

    /// Write a trace record to storage.
    ///
    /// The filename is based on the trace ID or git revision if available.
    pub fn write_trace(&self, trace: &TraceRecord) -> Result<PathBuf> {
        self.ensure_dir()?;

        let filename = self.trace_filename(trace);
        let path = self.base_dir.join(&filename);

        let json = serde_json::to_string_pretty(trace)
            .with_context(|| "Failed to serialize trace record")?;

        fs::write(&path, json).with_context(|| format!("Failed to write trace to {:?}", path))?;

        Ok(path)
    }

    /// Read a trace record by filename.
    pub fn read_trace(&self, filename: &str) -> Result<TraceRecord> {
        let path = self.base_dir.join(filename);
        self.read_trace_from_path(&path)
    }

    /// Read a trace record from a specific path.
    pub fn read_trace_from_path(&self, path: &Path) -> Result<TraceRecord> {
        let content =
            fs::read_to_string(path).with_context(|| format!("Failed to read trace: {:?}", path))?;

        let trace: TraceRecord = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse trace: {:?}", path))?;

        Ok(trace)
    }

    /// Read a trace by git revision.
    pub fn read_by_revision(&self, revision: &str) -> Result<Option<TraceRecord>> {
        let short_rev = &revision[..revision.len().min(12)];
        let filename = format!("{}.json", short_rev);
        let path = self.base_dir.join(&filename);

        if path.exists() {
            Ok(Some(self.read_trace_from_path(&path)?))
        } else {
            // Try full revision
            let filename = format!("{}.json", revision);
            let path = self.base_dir.join(&filename);
            if path.exists() {
                Ok(Some(self.read_trace_from_path(&path)?))
            } else {
                Ok(None)
            }
        }
    }

    /// List all trace files in storage.
    pub fn list_traces(&self) -> Result<Vec<PathBuf>> {
        if !self.base_dir.exists() {
            return Ok(Vec::new());
        }

        let mut traces = Vec::new();
        for entry in fs::read_dir(&self.base_dir)
            .with_context(|| format!("Failed to read trace directory: {:?}", self.base_dir))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                traces.push(path);
            }
        }

        // Sort by modification time (newest first)
        traces.sort_by(|a, b| {
            let a_time = fs::metadata(a).and_then(|m| m.modified()).ok();
            let b_time = fs::metadata(b).and_then(|m| m.modified()).ok();
            b_time.cmp(&a_time)
        });

        Ok(traces)
    }

    /// Delete a trace by filename.
    pub fn delete_trace(&self, filename: &str) -> Result<()> {
        let path = self.base_dir.join(filename);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to delete trace: {:?}", path))?;
        }
        Ok(())
    }

    /// Clean up old traces, keeping only the most recent N.
    pub fn cleanup(&self, keep_count: usize) -> Result<usize> {
        let traces = self.list_traces()?;
        let to_delete = traces.into_iter().skip(keep_count);
        let mut deleted = 0;

        for path in to_delete {
            if let Err(e) = fs::remove_file(&path) {
                tracing::warn!("Failed to delete old trace {:?}: {}", path, e);
            } else {
                deleted += 1;
            }
        }

        Ok(deleted)
    }

    /// Generate filename for a trace record.
    fn trace_filename(&self, trace: &TraceRecord) -> String {
        // Prefer git revision for filename (first 12 chars)
        if let Some(vcs) = &trace.vcs {
            let short_rev = &vcs.revision[..vcs.revision.len().min(12)];
            format!("{}.json", short_rev)
        } else {
            // Fall back to trace ID
            format!("{}.json", trace.id)
        }
    }
}

/// Index file for quick lookup of traces by file path.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TraceIndex {
    /// Version of the index format.
    pub version: String,
    /// Mapping from file path to trace filenames containing that path.
    pub files: std::collections::HashMap<String, Vec<String>>,
}

impl TraceIndex {
    /// Create a new empty index.
    pub fn new() -> Self {
        Self {
            version: AGENT_TRACE_VERSION.to_string(),
            files: std::collections::HashMap::new(),
        }
    }

    /// Add a trace to the index.
    pub fn add_trace(&mut self, trace: &TraceRecord, filename: &str) {
        for file in &trace.files {
            self.files
                .entry(file.path.clone())
                .or_default()
                .push(filename.to_string());
        }
    }

    /// Get trace filenames for a file path.
    pub fn get_traces_for_file(&self, path: &str) -> Option<&Vec<String>> {
        self.files.get(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use vtcode_exec_events::trace::{TraceFile, TraceRange, TraceRecordBuilder};

    fn create_test_trace() -> TraceRecord {
        TraceRecordBuilder::new()
            .git_revision("abc123def456789012345678901234567890abcd")
            .file(TraceFile::with_ai_ranges(
                "src/main.rs",
                "anthropic/claude-opus-4",
                vec![TraceRange::new(1, 50)],
            ))
            .build()
    }

    #[test]
    fn test_trace_store_write_read() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let store = TraceStore::new(temp_dir.path().join("traces"));

        let trace = create_test_trace();
        let path = store.write_trace(&trace)?;

        assert!(path.exists());

        let loaded = store.read_trace_from_path(&path)?;
        assert_eq!(loaded.id, trace.id);
        assert_eq!(loaded.files.len(), 1);

        Ok(())
    }

    #[test]
    fn test_trace_store_read_by_revision() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let store = TraceStore::new(temp_dir.path().join("traces"));

        let trace = create_test_trace();
        store.write_trace(&trace)?;

        let loaded = store.read_by_revision("abc123def456789012345678901234567890abcd")?;
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().id, trace.id);

        Ok(())
    }

    #[test]
    fn test_trace_store_list() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let store = TraceStore::new(temp_dir.path().join("traces"));

        // Write multiple traces with unique revisions (using large distinct values)
        let revisions = [
            "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0",
            "b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0a1",
            "c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0a1b2",
        ];
        for rev in &revisions {
            let trace = TraceRecordBuilder::new().git_revision(*rev).build();
            store.write_trace(&trace)?;
        }

        let traces = store.list_traces()?;
        assert_eq!(traces.len(), 3);

        Ok(())
    }

    #[test]
    fn test_trace_store_cleanup() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let store = TraceStore::new(temp_dir.path().join("traces"));

        // Write multiple traces with unique revisions
        let revisions = [
            "1111111111111111111111111111111111111111",
            "2222222222222222222222222222222222222222",
            "3333333333333333333333333333333333333333",
            "4444444444444444444444444444444444444444",
            "5555555555555555555555555555555555555555",
        ];
        for rev in &revisions {
            let trace = TraceRecordBuilder::new().git_revision(*rev).build();
            store.write_trace(&trace)?;
            // Small delay to ensure different modification times
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let deleted = store.cleanup(2)?;
        assert_eq!(deleted, 3);

        let remaining = store.list_traces()?;
        assert_eq!(remaining.len(), 2);

        Ok(())
    }

    #[test]
    fn test_trace_index() {
        let mut index = TraceIndex::new();
        let trace = create_test_trace();

        index.add_trace(&trace, "abc123def456.json");

        let traces = index.get_traces_for_file("src/main.rs");
        assert!(traces.is_some());
        assert_eq!(traces.unwrap().len(), 1);
    }
}
