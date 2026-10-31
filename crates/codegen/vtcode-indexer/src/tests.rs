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
fn indexes_allowed_directories_inside_hidden_excluded_parents() -> Result<()> {
    let temp = tempdir()?;
    let workspace = temp.path();
    let allowed_dir = workspace.join(".vtcode").join("external");
    fs::create_dir_all(&allowed_dir)?;
    fs::write(allowed_dir.join("plugin.toml"), "name = 'demo'")?;

    let mut indexer = SimpleIndexer::new(workspace.to_path_buf());
    indexer.init()?;
    indexer.index_directory(workspace)?;

    let results = indexer.find_files("plugin\\.toml$")?;
    assert_eq!(results.len(), 1);

    Ok(())
}

#[test]
fn reindexing_prunes_deleted_files_from_cache() -> Result<()> {
    let temp = tempdir()?;
    let workspace = temp.path();
    let file_path = workspace.join("notes.txt");
    fs::write(&file_path, "remember this")?;

    let mut indexer = SimpleIndexer::new(workspace.to_path_buf());
    indexer.init()?;
    indexer.index_directory(workspace)?;
    assert_eq!(indexer.find_files("notes\\.txt$")?.len(), 1);

    fs::remove_file(&file_path)?;
    indexer.index_directory(workspace)?;

    assert!(indexer.find_files("notes\\.txt$")?.is_empty());
    assert!(indexer.all_files().is_empty());

    Ok(())
}

#[test]
fn index_file_skips_excluded_paths() -> Result<()> {
    let temp = tempdir()?;
    let workspace = temp.path();
    let index_dir = workspace.join(".vtcode").join("index");
    fs::create_dir_all(&index_dir)?;
    let generated_index = index_dir.join("index.md");
    fs::write(&generated_index, "# generated")?;

    let mut indexer = SimpleIndexer::new(workspace.to_path_buf());
    indexer.init()?;
    indexer.index_file(&generated_index)?;

    assert!(indexer.all_files().is_empty());

    Ok(())
}

#[test]
fn index_file_removes_stale_entry_when_file_becomes_unreadable() -> Result<()> {
    let temp = tempdir()?;
    let workspace = temp.path();
    let file_path = workspace.join("notes.txt");
    fs::write(&file_path, "remember this")?;

    let mut indexer = SimpleIndexer::new(workspace.to_path_buf());
    indexer.init()?;
    indexer.index_file(&file_path)?;
    assert!(
        indexer
            .find_files("notes\\.txt$")?
            .iter()
            .any(|file| file.ends_with("notes.txt"))
    );

    fs::write(&file_path, [0xFF, 0xFE, 0xFD])?;
    indexer.index_file(&file_path)?;

    assert!(indexer.find_files("notes\\.txt$")?.is_empty());

    let index_content = fs::read_to_string(workspace.join(".vtcode").join("index").join("index.md"))?;
    assert!(!index_content.contains(file_path.to_string_lossy().as_ref()));

    Ok(())
}

#[test]
fn index_file_maintains_markdown_snapshot_across_updates() -> Result<()> {
    let temp = tempdir()?;
    let workspace = temp.path();
    let first = workspace.join("first.txt");
    let second = workspace.join("second.txt");
    fs::write(&first, "one")?;
    fs::write(&second, "two")?;

    let mut indexer = SimpleIndexer::new(workspace.to_path_buf());
    indexer.init()?;
    indexer.index_file(&first)?;
    indexer.index_file(&second)?;

    let index_dir = workspace.join(".vtcode").join("index");
    let files = fs::read_dir(&index_dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(files, vec!["index.md".to_string()]);

    let index_content = fs::read_to_string(index_dir.join("index.md"))?;
    assert!(index_content.contains(first.to_string_lossy().as_ref()));
    assert!(index_content.contains(second.to_string_lossy().as_ref()));

    Ok(())
}

#[test]
fn index_directory_writes_markdown_snapshot_without_manual_init() -> Result<()> {
    let temp = tempdir()?;
    let workspace = temp.path();
    fs::write(workspace.join("notes.txt"), "remember this")?;

    let mut indexer = SimpleIndexer::new(workspace.to_path_buf());
    indexer.index_directory(workspace)?;

    let index_content = fs::read_to_string(workspace.join(".vtcode").join("index").join("index.md"))?;
    assert!(index_content.contains(workspace.join("notes.txt").to_string_lossy().as_ref()));

    Ok(())
}

#[test]
fn get_file_content_clamps_ranges_without_panicking() -> Result<()> {
    let temp = tempdir()?;
    let workspace = temp.path();
    let file_path = workspace.join("notes.txt");
    fs::write(&file_path, "first\nsecond")?;

    let indexer = SimpleIndexer::new(workspace.to_path_buf());
    let file_path = file_path.to_string_lossy().into_owned();

    assert_eq!(indexer.get_file_content(&file_path, Some(5), None)?, "");
    assert_eq!(indexer.get_file_content(&file_path, Some(0), Some(1))?, "1: first\n");
    assert_eq!(indexer.get_file_content(&file_path, Some(2), Some(1))?, "");

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
    assert_eq!(entries[0].path, workspace.join("notes.txt").to_string_lossy().into_owned());

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
    let mut indexer = SimpleIndexer::with_config(config).with_filter(Arc::new(SkipRustFilter::default()));
    indexer.init()?;
    indexer.index_directory(workspace)?;

    assert!(indexer.find_files("lib\\.rs$")?.is_empty());
    assert!(!indexer.find_files("README\\.md$")?.is_empty());

    Ok(())
}

#[test]
fn custom_filters_can_skip_directories() -> Result<()> {
    #[derive(Default)]
    struct SkipGeneratedFilter {
        inner: ConfigTraversalFilter,
    }

    impl TraversalFilter for SkipGeneratedFilter {
        fn should_descend(&self, path: &Path, config: &SimpleIndexerConfig) -> bool {
            if path.ends_with("generated") {
                return false;
            }

            self.inner.should_descend(path, config)
        }

        fn should_index_file(&self, path: &Path, config: &SimpleIndexerConfig) -> bool {
            self.inner.should_index_file(path, config)
        }
    }

    let temp = tempdir()?;
    let workspace = temp.path();
    let generated_dir = workspace.join("generated");
    fs::create_dir_all(&generated_dir)?;
    fs::write(generated_dir.join("skip.txt"), "ignore me")?;
    fs::write(workspace.join("README.md"), "# Notes")?;

    let config = SimpleIndexerConfig::new(workspace.to_path_buf());
    let indexer = SimpleIndexer::with_config(config).with_filter(Arc::new(SkipGeneratedFilter::default()));
    let files = indexer.discover_files(workspace);

    assert!(!files.iter().any(|file| file.ends_with("skip.txt")));
    assert!(files.iter().any(|file| file.ends_with("README.md")));

    Ok(())
}

#[test]
fn discover_dir_entries_is_shallow_and_ignore_aware() -> Result<()> {
    let temp = tempdir()?;
    let workspace = temp.path();
    fs::create_dir_all(workspace.join("src"))?;
    fs::create_dir_all(workspace.join("node_modules"))?;
    fs::create_dir_all(workspace.join(".git"))?;
    fs::write(workspace.join("README.md"), "# Notes")?;
    fs::write(workspace.join("src").join("lib.rs"), "fn main() {}")?;
    fs::write(workspace.join("node_modules").join("dep.js"), "x")?;
    fs::write(workspace.join(".git").join("config"), "x")?;

    let config = SimpleIndexerConfig::new(workspace.to_path_buf());
    let indexer = SimpleIndexer::with_config(config);
    let entries = indexer.discover_dir_entries(workspace);

    let names: Vec<String> = entries
        .iter()
        .map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();

    assert_eq!(entries.len(), 2, "expected exactly README.md and src, got {names:?}");
    assert!(names.contains(&"README.md".to_string()));
    assert!(names.contains(&"src".to_string()));

    let (_, src_is_dir) = entries.iter().find(|(p, _)| p.ends_with("src")).unwrap();
    assert!(*src_is_dir);

    Ok(())
}

#[test]
fn indexing_multiple_directories_preserves_existing_cache_entries() -> Result<()> {
    let temp = tempdir()?;
    let workspace = temp.path();
    let src_dir = workspace.join("src");
    let docs_dir = workspace.join("docs");
    fs::create_dir_all(&src_dir)?;
    fs::create_dir_all(&docs_dir)?;
    fs::write(src_dir.join("lib.rs"), "fn main() {}")?;
    fs::write(docs_dir.join("guide.md"), "# Guide")?;

    let mut indexer = SimpleIndexer::new(workspace.to_path_buf());
    indexer.init()?;
    indexer.index_directory(&src_dir)?;
    indexer.index_directory(&docs_dir)?;

    assert!(indexer.find_files("lib\\.rs$")?.iter().any(|file| file.ends_with("lib.rs")));
    assert!(indexer.find_files("guide\\.md$")?.iter().any(|file| file.ends_with("guide.md")));

    let index_content = fs::read_to_string(workspace.join(".vtcode").join("index").join("index.md"))?;
    assert!(index_content.contains(src_dir.join("lib.rs").to_string_lossy().as_ref()));
    assert!(index_content.contains(docs_dir.join("guide.md").to_string_lossy().as_ref()));

    Ok(())
}

#[test]
fn batch_indexing_writes_single_markdown_file() -> Result<()> {
    let temp = tempdir()?;
    let workspace = temp.path();
    fs::write(workspace.join("lib.rs"), "fn main() {}")?;
    fs::write(workspace.join("README.md"), "# Notes")?;

    let mut indexer = SimpleIndexer::new(workspace.to_path_buf());
    indexer.init()?;
    indexer.index_directory(workspace)?;

    let index_dir = workspace.join(".vtcode").join("index");
    let files = fs::read_dir(&index_dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(files, vec!["index.md".to_string()]);

    let index_content = fs::read_to_string(index_dir.join("index.md"))?;
    assert!(index_content.contains(workspace.join("lib.rs").to_string_lossy().as_ref()));
    assert!(index_content.contains(workspace.join("README.md").to_string_lossy().as_ref()));

    Ok(())
}

#[test]
fn batch_indexing_removes_legacy_hashed_entries() -> Result<()> {
    let temp = tempdir()?;
    let workspace = temp.path();
    fs::write(workspace.join("lib.rs"), "fn main() {}")?;

    let mut indexer = SimpleIndexer::new(workspace.to_path_buf());
    indexer.init()?;

    let legacy_file_name = format!("{}.md", calculate_hash("legacy-path"));
    let legacy_file_path = workspace.join(".vtcode").join("index").join(&legacy_file_name);
    fs::write(&legacy_file_path, "# legacy")?;
    assert!(legacy_file_path.exists());

    indexer.index_directory(workspace)?;

    assert!(!legacy_file_path.exists());
    let files = fs::read_dir(workspace.join(".vtcode").join("index"))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(files, vec!["index.md".to_string()]);

    Ok(())
}

#[test]
fn snapshot_storage_uses_default_ref_batch_persistence() -> Result<()> {
    #[derive(Clone, Default)]
    struct SnapshotMemoryStorage {
        snapshots: Arc<Mutex<Vec<Vec<FileIndex>>>>,
    }

    impl SnapshotMemoryStorage {
        fn new(snapshots: Arc<Mutex<Vec<Vec<FileIndex>>>>) -> Self {
            Self { snapshots }
        }
    }

    impl IndexStorage for SnapshotMemoryStorage {
        fn init(&self, _index_dir: &Path) -> Result<()> {
            Ok(())
        }

        fn persist(&self, _index_dir: &Path, _entry: &FileIndex) -> Result<()> {
            Ok(())
        }

        fn prefers_snapshot_persistence(&self) -> bool {
            true
        }

        fn persist_batch(&self, _index_dir: &Path, entries: &[FileIndex]) -> Result<()> {
            self.snapshots.lock().expect("lock poisoned").push(entries.to_vec());
            Ok(())
        }
    }

    let temp = tempdir()?;
    let workspace = temp.path();
    let file_path = workspace.join("notes.txt");
    fs::write(&file_path, "remember this")?;

    let snapshots = Arc::new(Mutex::new(Vec::new()));
    let storage = SnapshotMemoryStorage::new(snapshots.clone());

    let config = SimpleIndexerConfig::new(workspace.to_path_buf());
    let mut indexer = SimpleIndexer::with_config(config).with_storage(Arc::new(storage));
    indexer.index_file(&file_path)?;

    let snapshots = snapshots.lock().expect("lock poisoned");
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].len(), 1);
    assert_eq!(snapshots[0][0].path, workspace.join("notes.txt").to_string_lossy().into_owned());

    Ok(())
}

#[test]
fn snapshot_index_file_rolls_back_cache_when_persist_fails() -> Result<()> {
    #[derive(Clone, Default)]
    struct FlakySnapshotStorage {
        persist_count: Arc<Mutex<usize>>,
    }

    impl IndexStorage for FlakySnapshotStorage {
        fn init(&self, _index_dir: &Path) -> Result<()> {
            Ok(())
        }

        fn persist(&self, _index_dir: &Path, _entry: &FileIndex) -> Result<()> {
            Ok(())
        }

        fn prefers_snapshot_persistence(&self) -> bool {
            true
        }

        fn persist_batch(&self, _index_dir: &Path, _entries: &[FileIndex]) -> Result<()> {
            let mut count = self.persist_count.lock().expect("lock poisoned");
            *count += 1;
            if *count == 2 {
                anyhow::bail!("simulated snapshot persistence failure");
            }
            Ok(())
        }
    }

    let temp = tempdir()?;
    let workspace = temp.path();
    let first = workspace.join("first.txt");
    let second = workspace.join("second.txt");
    fs::write(&first, "one")?;
    fs::write(&second, "two")?;

    let config = SimpleIndexerConfig::new(workspace.to_path_buf());
    let storage = Arc::new(FlakySnapshotStorage::default());
    let mut indexer = SimpleIndexer::with_config(config).with_storage(storage);

    indexer.index_file(&first)?;
    assert!(
        indexer
            .find_files("first\\.txt$")?
            .iter()
            .any(|path| path.ends_with("first.txt"))
    );

    let err = indexer.index_file(&second).expect_err("second persist should fail");
    assert!(err.to_string().contains("simulated snapshot persistence failure"));
    assert!(
        indexer
            .find_files("first\\.txt$")?
            .iter()
            .any(|path| path.ends_with("first.txt"))
    );
    assert!(indexer.find_files("second\\.txt$")?.is_empty());

    Ok(())
}

#[cfg(unix)]
#[test]
fn snapshot_persist_ignores_planted_tmp_symlink() -> Result<()> {
    use std::os::unix::fs::symlink;

    let temp = tempdir()?;
    let workspace = temp.path();
    fs::write(workspace.join("notes.txt"), "remember this")?;

    // A predictable temporary name would let a planted link redirect the
    // snapshot write. Exclusive unique temporaries never touch it.
    let index_dir = workspace.join(".vtcode").join("index");
    fs::create_dir_all(&index_dir)?;
    let victim = workspace.join("victim.txt");
    fs::write(&victim, "untouched")?;
    symlink(&victim, index_dir.join(".index.md.tmp"))?;

    let mut indexer = SimpleIndexer::new(workspace.to_path_buf());
    indexer.init()?;
    indexer.index_directory(workspace)?;

    assert_eq!(fs::read_to_string(&victim)?, "untouched");
    let snapshot = fs::read_to_string(index_dir.join("index.md"))?;
    assert!(snapshot.contains("notes.txt"));
    assert!(fs::symlink_metadata(index_dir.join("index.md"))?.is_file());
    Ok(())
}

#[cfg(unix)]
#[test]
fn snapshot_persist_rejects_symlinked_index_dir() -> Result<()> {
    use std::os::unix::fs::symlink;

    let temp = tempdir()?;
    let workspace = temp.path();
    let target = workspace.join("notes.txt");
    fs::write(&target, "remember this")?;

    let outside_tmp = tempdir()?;
    let outside = outside_tmp.path().join("target");
    fs::create_dir_all(&outside)?;
    symlink(&outside, workspace.join(".vtcode"))?;

    let mut indexer = SimpleIndexer::new(workspace.to_path_buf());
    let error = indexer.index_file(&target).expect_err("symlinked index dir must fail");
    assert!(format!("{error:#}").contains("escapes its containing directory"), "{error:?}");
    let landed = fs::read_dir(&outside)?
        .filter_map(|entry| entry.ok())
        .flat_map(|entry| fs::read_dir(entry.path()))
        .flatten()
        .filter_map(|entry| entry.ok())
        .any(|entry| entry.path().extension().is_some_and(|ext| ext == "md"));
    assert!(!landed, "no snapshot may land through the symlinked dir");
    Ok(())
}

#[test]
fn markdown_store_rejects_traversal_keys() -> Result<()> {
    use markdown_store::{SimpleCache, SimpleKVStorage, SimpleProjectManager};

    let temp = tempdir()?;
    let store = SimpleKVStorage::new(temp.path().join("store"));
    store.init()?;
    store.put("hello", "world")?;
    assert_eq!(store.get("hello")?, "world");

    for key in ["../escape", "..", "", "a/b", "/abs", "a\0b", "."] {
        assert!(store.put(key, "x").is_err(), "put {key:?}");
        assert!(store.get(key).is_err(), "get {key:?}");
        assert!(store.delete(key).is_err(), "delete {key:?}");
    }
    assert!(!temp.path().join("escape.md").exists());
    assert!(!temp.path().join("abs.md").exists());

    let cache = SimpleCache::new(temp.path().join("cache"));
    cache.init()?;
    cache.store("hello", "world")?;
    assert_eq!(cache.load("hello")?, "world");
    for key in ["../escape", "a/b", ""] {
        assert!(cache.store(key, "x").is_err(), "cache store {key:?}");
        assert!(cache.load(key).is_err(), "cache load {key:?}");
        assert!(!cache.exists(key), "cache exists {key:?}");
    }

    let projects = SimpleProjectManager::new(temp.path().to_path_buf());
    projects.init()?;
    let error = projects.create_project("../evil", None).expect_err("traversal name must fail");
    assert!(error.to_string().contains("single file name"), "{error:?}");
    assert!(!temp.path().join("evil.md").exists());
    assert!(!temp.path().join(".vtcode").join("evil.md").exists());
    projects.create_project("demo", None)?;
    assert!(projects.list_projects()?.contains(&"demo".to_string()));
    Ok(())
}

#[test]
fn markdown_store_read_paths_do_not_create_stores() -> Result<()> {
    use markdown_store::{SimpleCache, SimpleKVStorage};

    let temp = tempdir()?;
    let store = SimpleKVStorage::new(temp.path().join("missing-store"));
    assert!(store.get("hello").is_err());
    store.delete("hello")?;
    assert!(!temp.path().join("missing-store").exists());

    let cache = SimpleCache::new(temp.path().join("missing-cache"));
    assert!(cache.load("hello").is_err());
    assert!(cache.list().is_err());
    assert!(!temp.path().join("missing-cache").exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn markdown_store_rejects_symlinked_records_and_dirs() -> Result<()> {
    use markdown_store::SimpleKVStorage;
    use std::os::unix::fs::symlink;

    let temp = tempdir()?;
    let victim = temp.path().join("victim.txt");
    fs::write(&victim, "untouched")?;

    let store = SimpleKVStorage::new(temp.path().join("store"));
    store.init()?;
    symlink(&victim, temp.path().join("store").join("linked.md"))?;
    assert!(store.put("linked", "overwrite").is_err());
    assert!(store.get("linked").is_err());
    assert_eq!(fs::read_to_string(&victim)?, "untouched");

    symlink(temp.path().join("elsewhere"), temp.path().join("linked-root"))?;
    fs::create_dir_all(temp.path().join("elsewhere"))?;
    let escaped = SimpleKVStorage::new(temp.path().join("linked-root"));
    assert!(escaped.put("hello", "world").is_err());
    assert!(fs::read_dir(temp.path().join("elsewhere"))?.next().is_none());
    Ok(())
}

/// Snapshot storage that counts `persist_batch_refs` calls while delegating
/// to the real markdown backend, so hydration can read the on-disk snapshot
/// while the test observes rewrite attempts.
#[derive(Clone, Default)]
struct CountingSnapshotStorage {
    persist_calls: Arc<Mutex<usize>>,
}

impl IndexStorage for CountingSnapshotStorage {
    fn init(&self, index_dir: &Path) -> Result<()> {
        MarkdownIndexStorage.init(index_dir)
    }

    fn persist(&self, index_dir: &Path, entry: &FileIndex) -> Result<()> {
        MarkdownIndexStorage.persist(index_dir, entry)
    }

    fn prefers_snapshot_persistence(&self) -> bool {
        true
    }

    fn persist_batch_refs(&self, index_dir: &Path, entries: &[&FileIndex]) -> Result<()> {
        *self.persist_calls.lock().expect("lock poisoned") += 1;
        MarkdownIndexStorage.persist_batch_refs(index_dir, entries)
    }
}

fn counting_indexer(workspace: &Path, persist_calls: &Arc<Mutex<usize>>) -> SimpleIndexer {
    let storage = CountingSnapshotStorage { persist_calls: Arc::clone(persist_calls) };
    SimpleIndexer::new(workspace.to_path_buf()).with_storage(Arc::new(storage))
}

fn persist_call_count(persist_calls: &Arc<Mutex<usize>>) -> usize {
    *persist_calls.lock().expect("lock poisoned")
}

fn snapshot_text(indexer: &SimpleIndexer) -> Result<String> {
    Ok(fs::read_to_string(indexer.index_dir().join("index.md"))?)
}

#[test]
fn hydrate_reuses_unchanged_entries_and_skips_snapshot_rewrite() -> Result<()> {
    let temp = tempdir()?;
    let workspace = temp.path();
    fs::write(workspace.join("alpha.txt"), "alpha contents")?;
    fs::write(workspace.join("beta.txt"), "beta contents")?;

    let persist_calls = Arc::new(Mutex::new(0usize));
    let mut indexer = counting_indexer(workspace, &persist_calls);
    indexer.init()?;
    indexer.index_directory(workspace)?;
    assert_eq!(persist_call_count(&persist_calls), 1);

    // Second run over an unchanged workspace: hydration screens every file
    // and the whole-snapshot rewrite is skipped entirely.
    let mut indexer = counting_indexer(workspace, &persist_calls);
    indexer.init()?;
    assert_eq!(indexer.hydrate_from_snapshot(), 2);
    indexer.index_directory(workspace)?;
    assert_eq!(persist_call_count(&persist_calls), 1);

    Ok(())
}

#[test]
fn changed_file_size_is_rebuilt_and_repersisted() -> Result<()> {
    let temp = tempdir()?;
    let workspace = temp.path();
    let alpha = workspace.join("alpha.txt");
    fs::write(&alpha, "alpha contents")?;

    let persist_calls = Arc::new(Mutex::new(0usize));
    let mut indexer = counting_indexer(workspace, &persist_calls);
    indexer.init()?;
    indexer.index_directory(workspace)?;
    assert_eq!(persist_call_count(&persist_calls), 1);

    fs::write(&alpha, "alpha contents extended")?;
    let mut indexer = counting_indexer(workspace, &persist_calls);
    indexer.init()?;
    assert_eq!(indexer.hydrate_from_snapshot(), 1);
    indexer.index_directory(workspace)?;
    assert_eq!(persist_call_count(&persist_calls), 2);

    let expected_hash = vtcode_commons::utils::calculate_sha256(b"alpha contents extended");
    assert!(snapshot_text(&indexer)?.contains(&expected_hash));
    Ok(())
}

#[test]
fn deleted_file_is_pruned_and_repersisted() -> Result<()> {
    let temp = tempdir()?;
    let workspace = temp.path();
    let beta = workspace.join("beta.txt");
    fs::write(workspace.join("alpha.txt"), "alpha contents")?;
    fs::write(&beta, "beta contents")?;

    let persist_calls = Arc::new(Mutex::new(0usize));
    let mut indexer = counting_indexer(workspace, &persist_calls);
    indexer.init()?;
    indexer.index_directory(workspace)?;
    assert_eq!(persist_call_count(&persist_calls), 1);

    fs::remove_file(&beta)?;
    let mut indexer = counting_indexer(workspace, &persist_calls);
    indexer.init()?;
    assert_eq!(indexer.hydrate_from_snapshot(), 2);
    indexer.index_directory(workspace)?;
    assert_eq!(persist_call_count(&persist_calls), 2);

    assert!(!snapshot_text(&indexer)?.contains("beta.txt"));
    let beta_key = workspace.join("beta.txt").to_string_lossy().into_owned();
    assert!(
        !indexer.content_cache.contains_key(beta_key.as_str()),
        "content for the deleted file must be pruned"
    );
    Ok(())
}

#[test]
fn binary_file_does_not_force_snapshot_rewrite() -> Result<()> {
    let temp = tempdir()?;
    let workspace = temp.path();
    fs::write(workspace.join("alpha.txt"), "alpha contents")?;
    fs::write(workspace.join("image.bin"), [0xFFu8, 0xFE, 0x00, 0xC3])?;

    let persist_calls = Arc::new(Mutex::new(0usize));
    let mut indexer = counting_indexer(workspace, &persist_calls);
    indexer.init()?;
    indexer.index_directory(workspace)?;
    assert_eq!(persist_call_count(&persist_calls), 1);

    // The unindexable binary file produces no entry and must not force the
    // snapshot rewrite on a quiet run.
    let mut indexer = counting_indexer(workspace, &persist_calls);
    indexer.init()?;
    assert_eq!(indexer.hydrate_from_snapshot(), 1);
    indexer.index_directory(workspace)?;
    assert_eq!(persist_call_count(&persist_calls), 1);

    Ok(())
}

#[test]
fn tampered_snapshot_entries_are_policy_filtered() -> Result<()> {
    let temp = tempdir()?;
    let workspace = temp.path();

    let hash = "a".repeat(64);
    let entry_block = |path: &str| {
        format!(
            "## {path}\n\n- **Path**: {path}\n- **Hash**: {hash}\n- **Modified**: 1735689600\n- **Size**: 42 \
             bytes\n- **Language**: env\n- **Tags**: \n\n"
        )
    };
    let snapshot = format!(
        "# Workspace File Index\n\n- **Entries**: 1\n\n{}",
        entry_block(&workspace.join(".env").to_string_lossy())
    );
    let index_dir = workspace.join(".vtcode").join("index");
    fs::create_dir_all(&index_dir)?;
    fs::write(index_dir.join("index.md"), snapshot)?;

    let mut indexer = SimpleIndexer::new(workspace.to_path_buf());
    indexer.init()?;
    assert_eq!(indexer.hydrate_from_snapshot(), 0);

    Ok(())
}

#[test]
fn corrupt_snapshot_falls_back_to_full_index() -> Result<()> {
    let temp = tempdir()?;
    let workspace = temp.path();
    fs::write(workspace.join("alpha.txt"), "alpha contents")?;

    let persist_calls = Arc::new(Mutex::new(0usize));
    let mut indexer = counting_indexer(workspace, &persist_calls);
    indexer.init()?;
    indexer.index_directory(workspace)?;

    fs::write(indexer.index_dir().join("index.md"), "not a snapshot")?;
    let mut indexer = counting_indexer(workspace, &persist_calls);
    indexer.init()?;
    assert_eq!(indexer.hydrate_from_snapshot(), 0);
    indexer.index_directory(workspace)?;
    assert_eq!(persist_call_count(&persist_calls), 2);
    assert!(snapshot_text(&indexer)?.contains("alpha.txt"));

    Ok(())
}

#[test]
fn foreign_root_snapshot_entries_are_discarded() -> Result<()> {
    let temp = tempdir()?;
    let workspace = temp.path();

    let hash = "a".repeat(64);
    let snapshot = format!(
        "# Workspace File Index\n\n- **Entries**: 1\n\n## /opt/other/project/file.txt\n\n- **Path**: \
         /opt/other/project/file.txt\n- **Hash**: {hash}\n- **Modified**: 1735689600\n- **Size**: 42 bytes\n- \
         **Language**: txt\n- **Tags**: \n\n"
    );
    let index_dir = workspace.join(".vtcode").join("index");
    fs::create_dir_all(&index_dir)?;
    fs::write(index_dir.join("index.md"), snapshot)?;

    let mut indexer = SimpleIndexer::new(workspace.to_path_buf());
    indexer.init()?;
    assert_eq!(indexer.hydrate_from_snapshot(), 0);

    Ok(())
}

#[test]
fn dotdot_snapshot_entries_are_discarded() -> Result<()> {
    let temp = tempdir()?;
    let workspace = temp.path();
    let root = workspace.to_string_lossy().into_owned();

    // Both escapes pass a lexical root-prefix check (`<root>/..` still
    // starts with `<root>`), so only the parent-reference guard rejects
    // them. `file..txt` carries dots without a `..` segment and must pass,
    // proving the check is component-based rather than a substring match.
    let escape = format!("{root}/../escape.txt");
    let nested_escape = format!("{root}/sub/../../nested-escape.txt");
    let dotted = format!("{root}/file..txt");

    let hash = "a".repeat(64);
    let entry_block = |path: &str| {
        format!(
            "## {path}\n\n- **Path**: {path}\n- **Hash**: {hash}\n- **Modified**: 1735689600\n- **Size**: 42 \
             bytes\n- **Language**: txt\n- **Tags**: \n\n"
        )
    };
    let snapshot = format!(
        "# Workspace File Index\n\n- **Entries**: 3\n\n{}{}{}",
        entry_block(&escape),
        entry_block(&nested_escape),
        entry_block(&dotted)
    );
    let index_dir = workspace.join(".vtcode").join("index");
    fs::create_dir_all(&index_dir)?;
    fs::write(index_dir.join("index.md"), snapshot)?;

    let mut indexer = SimpleIndexer::new(workspace.to_path_buf());
    indexer.init()?;
    assert_eq!(indexer.hydrate_from_snapshot(), 1);

    let cached = indexer.all_files();
    assert_eq!(cached, vec![dotted]);
    assert!(indexer.find_files("escape")?.is_empty());

    Ok(())
}

#[test]
fn parse_snapshot_round_trips_writer_format() {
    let hash = "b".repeat(64);
    let text = format!(
        "# Workspace File Index\n\n- **Entries**: 2\n\n## /ws/one.txt\n\n- **Path**: /ws/one.txt\n- **Hash**: \
         {hash}\n- **Modified**: 1735689600\n- **Size**: 12 bytes\n- **Language**: txt\n- **Tags**: \n\n## \
         /ws/two.rs\n\n- **Path**: /ws/two.rs\n- **Hash**: {hash}\n- **Modified**: 1735689601\n- **Size**: 34 \
         bytes\n- **Language**: rs\n- **Tags**: a, b\n\n"
    );

    let entries = snapshot::parse_snapshot(&text).expect("well-formed snapshot must parse");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].path, "/ws/one.txt");
    assert_eq!(entries[0].hash, hash);
    assert_eq!(entries[0].modified, 1_735_689_600);
    assert_eq!(entries[0].size, 12);
    assert_eq!(entries[0].language, "txt");
    assert!(entries[0].tags.is_empty());
    assert_eq!(entries[1].path, "/ws/two.rs");
    assert_eq!(entries[1].size, 34);
    assert_eq!(entries[1].tags, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn parse_snapshot_rejects_malformed_snapshots() {
    let hash = "c".repeat(64);
    let entry = |hash: &str| {
        format!(
            "## /ws/one.txt\n\n- **Path**: /ws/one.txt\n- **Hash**: {hash}\n- **Modified**: 1735689600\n- \
             **Size**: 12 bytes\n- **Language**: txt\n- **Tags**: \n\n"
        )
    };

    // Missing header.
    assert!(snapshot::parse_snapshot(&format!("- **Entries**: 1\n\n{}", entry(&hash))).is_none());
    // Entries count disagrees with the parsed blocks.
    assert!(
        snapshot::parse_snapshot(&format!("# Workspace File Index\n\n- **Entries**: 3\n\n{}", entry(&hash))).is_none()
    );
    // Hash is not a 64-hex digest.
    assert!(
        snapshot::parse_snapshot(&format!("# Workspace File Index\n\n- **Entries**: 1\n\n{}", entry("xyz"))).is_none()
    );
    // Size lacks the " bytes" suffix.
    assert!(
        snapshot::parse_snapshot(&format!(
            "# Workspace File Index\n\n- **Entries**: 1\n\n{}",
            entry(&hash).replace("12 bytes", "12")
        ))
        .is_none()
    );
}
