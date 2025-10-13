use anyhow::Result;
use std::fs;
use std::path::PathBuf;

use super::FileIndex;

/// Sink that receives indexed file metadata.
pub trait IndexSink: Send + Sync {
    /// Prepare the sink so it can receive entries (e.g., create directories).
    fn prepare(&self) -> Result<()>;

    /// Persist a [`FileIndex`] entry.
    fn persist(&self, entry: &FileIndex) -> Result<()>;
}

/// Sink implementation that writes each entry to a markdown file in a directory.
#[derive(Clone, Debug)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn markdown_sink_writes_files() {
        let temp = tempdir().expect("tempdir");
        let sink = MarkdownIndexSink::new(temp.path().join("index"));
        sink.prepare().expect("prepare succeeds");

        let entry = FileIndex {
            path: "src/lib.rs".into(),
            hash: "abc".into(),
            modified: 1,
            size: 10,
            language: "rs".into(),
            tags: vec![],
        };

        sink.persist(&entry).expect("persist succeeds");

        let entries: Vec<_> = std::fs::read_dir(temp.path().join("index"))
            .expect("read_dir succeeds")
            .collect();
        assert_eq!(entries.len(), 1);
    }
}
