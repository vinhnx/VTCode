use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use tempfile::tempdir;

#[derive(Default)]
struct SnapshotStorage {
    fail: AtomicBool,
}

impl IndexStorage for SnapshotStorage {
    fn init(&self, _: &Path) -> Result<()> {
        Ok(())
    }
    fn persist(&self, _: &Path, _: &FileIndex) -> Result<()> {
        Ok(())
    }
    fn prefers_snapshot_persistence(&self) -> bool {
        true
    }
    fn persist_batch(&self, _: &Path, _: &[FileIndex]) -> Result<()> {
        if self.fail.load(Ordering::Relaxed) {
            anyhow::bail!("snapshot failed");
        }
        Ok(())
    }
}

#[test]
fn snapshot_file_update_keeps_content_and_restores_it_on_failure() -> Result<()> {
    let temp = tempdir()?;
    let path = temp.path().join("note.txt");
    let key = path.to_string_lossy().into_owned();
    let storage = Arc::new(SnapshotStorage::default());
    let mut indexer = SimpleIndexer::new(temp.path().to_path_buf()).with_storage(storage.clone());
    fs::write(&path, "first content")?;
    indexer.index_file(&path)?;
    assert_eq!(indexer.content_cache.get(&key).unwrap().1.as_ref(), "first content");

    fs::write(&path, "different content")?;
    storage.fail.store(true, Ordering::Relaxed);
    assert!(indexer.index_file(&path).is_err());
    assert_eq!(indexer.content_cache.get(&key).unwrap().1.as_ref(), "first content");
    assert_eq!(indexer.index_cache.get(&key).unwrap().hash, calculate_hash("first content"));

    storage.fail.store(false, Ordering::Relaxed);
    indexer.index_file(&path)?;
    assert_eq!(indexer.content_cache.get(&key).unwrap().1.as_ref(), "different content");
    fs::remove_file(&path)?;
    indexer.index_file(&path)?;
    assert!(!indexer.content_cache.contains_key(&key));
    assert!(!indexer.index_cache.contains_key(&key));
    Ok(())
}
