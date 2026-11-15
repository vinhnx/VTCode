//! Single-stat build coverage: the metadata-aware index builder and filter
//! must agree with the plain path-based builders on every input class.

use super::super::*;
use super::{counting_indexer, persist_call_count, snapshot_text};
use std::fs;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

fn assert_same_entry(left: &FileIndex, right: &FileIndex) {
    assert_eq!(left.path, right.path);
    assert_eq!(left.hash, right.hash);
    assert_eq!(left.modified, right.modified);
    assert_eq!(left.modified_nanos, right.modified_nanos);
    assert_eq!(left.size, right.size);
    assert_eq!(left.language, right.language);
    assert_eq!(left.tags, right.tags);
}

#[test]
fn metadata_build_matches_plain_build_and_skips_non_files() -> Result<()> {
    let temp = tempdir()?;
    let workspace = temp.path();
    fs::write(workspace.join("notes.txt"), "hello")?;
    fs::write(workspace.join(".hidden.txt"), "hidden")?;
    // Non-hidden sensitive name: rejection must come from the sensitive list.
    fs::write(workspace.join("credentials.json"), "{}")?;
    fs::create_dir_all(workspace.join("subdir"))?;

    let config = SimpleIndexerConfig::new(workspace.to_path_buf());
    let mut indexer = SimpleIndexer::with_config(config);

    // Regular file: both builders agree on every field.
    let file = workspace.join("notes.txt");
    let metadata = fs::metadata(&file)?;
    let plain = indexer.build_file_index(&file)?.expect("plain build indexes the file");
    let via_metadata = indexer
        .build_file_index_with_metadata(&file, &metadata)?
        .expect("metadata build indexes the file");
    assert_same_entry(&plain, &via_metadata);

    // Directories, hidden files, and sensitive names are rejected by both.
    for rejected in [
        workspace.join("subdir"),
        workspace.join(".hidden.txt"),
        workspace.join("credentials.json"),
    ] {
        let metadata = fs::metadata(&rejected)?;
        assert!(indexer.build_file_index(&rejected)?.is_none(), "plain: {}", rejected.display());
        assert!(
            indexer.build_file_index_with_metadata(&rejected, &metadata)?.is_none(),
            "metadata: {}",
            rejected.display()
        );
    }

    Ok(())
}

#[test]
fn metadata_filter_default_delegates_to_plain_check_for_custom_filters() -> Result<()> {
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

    for (name, expected) in [("lib.rs", false), ("README.md", true)] {
        let path = workspace.join(name);
        let metadata = fs::metadata(&path)?;
        assert_eq!(indexer.build_file_index(&path)?.is_some(), expected, "plain: {name}");
        assert_eq!(indexer.build_file_index_with_metadata(&path, &metadata)?.is_some(), expected, "metadata: {name}");
    }

    // The directory-walk hot loop consults the custom filter too.
    indexer.init()?;
    indexer.index_directory(workspace)?;
    assert!(indexer.find_files("lib\\.rs$")?.is_empty());
    assert!(!indexer.find_files("README\\.md$")?.is_empty());

    Ok(())
}

#[test]
fn reindex_serves_unchanged_entries_and_rebuilds_only_changed_files() -> Result<()> {
    let temp = tempdir()?;
    let workspace = temp.path();
    fs::write(workspace.join("stable.txt"), "unchanged")?;
    fs::write(workspace.join("edited.txt"), "version one")?;

    let mut indexer = SimpleIndexer::new(workspace.to_path_buf());
    indexer.init()?;
    indexer.index_directory(workspace)?;
    // Hydrate so the second run exercises the cached unchanged screen.
    indexer.hydrate_from_snapshot();

    let before: Vec<FileIndex> = indexer
        .all_files()
        .iter()
        .filter_map(|path| indexer.index_cache.get(path))
        .cloned()
        .collect();
    assert_eq!(before.len(), 2);

    // A no-change re-index must serve every entry from the screen.
    indexer.index_directory(workspace)?;
    let rerun: Vec<FileIndex> = indexer
        .all_files()
        .iter()
        .filter_map(|path| indexer.index_cache.get(path))
        .cloned()
        .collect();
    assert_eq!(rerun.len(), 2);
    for (left, right) in before.iter().zip(rerun.iter()) {
        assert_same_entry(left, right);
    }

    // A different-size rewrite forces a rebuild of that file only.
    fs::write(workspace.join("edited.txt"), "version one plus much more content")?;
    indexer.index_directory(workspace)?;
    let stable = indexer
        .index_cache
        .get(workspace.join("stable.txt").to_string_lossy().as_ref())
        .expect("stable entry survives");
    let edited = indexer
        .index_cache
        .get(workspace.join("edited.txt").to_string_lossy().as_ref())
        .expect("edited entry survives");
    assert_eq!(stable.hash, before.iter().find(|entry| entry.path.ends_with("stable.txt")).unwrap().hash);
    assert_ne!(edited.hash, before.iter().find(|entry| entry.path.ends_with("edited.txt")).unwrap().hash);
    assert_eq!(edited.size, "version one plus much more content".len() as u64);

    Ok(())
}

#[test]
fn same_second_same_size_edit_rebuilds_after_hydration() -> Result<()> {
    use std::fs::{File, FileTimes};
    use std::time::{Duration, UNIX_EPOCH};

    let temp = tempdir()?;
    let workspace = temp.path();
    let path = workspace.join("alpha.txt");
    let set_mtime = |nanos| -> Result<()> {
        let modified = UNIX_EPOCH + Duration::new(1_735_689_600, nanos);
        File::options()
            .write(true)
            .open(&path)?
            .set_times(FileTimes::new().set_modified(modified))?;
        Ok(())
    };
    fs::write(&path, "first")?;
    set_mtime(100_000_000)?;
    let calls = Arc::new(Mutex::new(0));
    let mut indexer = counting_indexer(workspace, &calls);
    indexer.init()?;
    indexer.index_directory(workspace)?;

    fs::write(&path, "other")?;
    set_mtime(800_000_000)?;
    let mut indexer = counting_indexer(workspace, &calls);
    indexer.init()?;
    assert_eq!(indexer.hydrate_from_snapshot(), 1);
    indexer.index_directory(workspace)?;
    assert_eq!(persist_call_count(&calls), 2);
    assert!(snapshot_text(&indexer)?.contains(&vtcode_commons::utils::calculate_sha256(b"other")));
    Ok(())
}

#[test]
fn legacy_snapshot_without_subseconds_is_rebuilt() -> Result<()> {
    let temp = tempdir()?;
    let workspace = temp.path();
    fs::write(workspace.join("alpha.txt"), "content")?;
    let calls = Arc::new(Mutex::new(0));
    let mut indexer = counting_indexer(workspace, &calls);
    indexer.init()?;
    indexer.index_directory(workspace)?;
    let legacy = snapshot_text(&indexer)?
        .lines()
        .filter(|line| !line.starts_with("- **Modified nanos**:"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(indexer.index_dir().join("index.md"), legacy)?;

    let mut indexer = counting_indexer(workspace, &calls);
    indexer.init()?;
    assert_eq!(indexer.hydrate_from_snapshot(), 1);
    indexer.index_directory(workspace)?;
    assert_eq!(persist_call_count(&calls), 2);
    assert!(snapshot_text(&indexer)?.contains("- **Modified nanos**:"));
    Ok(())
}
