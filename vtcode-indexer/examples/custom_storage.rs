use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use tempfile::tempdir;
use vtcode_indexer::{
    ConfigTraversalFilter, FileIndex, IndexStorage, SimpleIndexer, SimpleIndexerConfig,
    TraversalFilter,
};

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
        println!("stored {}", entry.path);
        Ok(())
    }
}

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

fn main() -> Result<()> {
    let temp = tempdir()?;
    let workspace = temp.path();

    std::fs::write(workspace.join("README.md"), "# workspace\n")?;
    std::fs::write(workspace.join("lib.rs"), "fn main() {}\n")?;

    let records: Arc<Mutex<Vec<FileIndex>>> = Arc::new(Mutex::new(Vec::new()));
    let storage = MemoryStorage::new(records.clone());

    let config = SimpleIndexerConfig::new(workspace.to_path_buf());
    let mut indexer = SimpleIndexer::with_config(config)
        .with_storage(Arc::new(storage))
        .with_filter(Arc::new(SkipRustFilter::default()));

    indexer.init()?;
    indexer.index_directory(workspace)?;

    let guard = records.lock().expect("lock poisoned");
    println!("indexed files: {}", guard.len());

    Ok(())
}
