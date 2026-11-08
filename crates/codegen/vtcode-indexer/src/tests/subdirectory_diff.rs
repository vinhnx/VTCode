//! Subdirectory-diff coverage: [`SimpleIndexer::cached_subdirectory_differs`]
//! must agree with naive set inequality on add/remove/replace/empty boundaries.

use super::super::*;

fn cached_entry(path: &str) -> FileIndex {
    FileIndex::from_parts(path.to_string(), String::from("hash"), 0, 0, String::from("txt"), Vec::new())
}

/// Independent oracle for [`SimpleIndexer::cached_subdirectory_differs`]: set
/// inequality via sorted comparison, sharing no logic with the implementation.
fn naive_sets_differ(cached_under_dir: &[&str], walked: &[&str]) -> bool {
    let mut expected: Vec<&str> = cached_under_dir.to_vec();
    let mut actual: Vec<&str> = walked.to_vec();
    expected.sort_unstable();
    actual.sort_unstable();
    expected != actual
}

#[test]
fn cached_subdirectory_diff_detects_add_remove_replace() {
    let dir = Path::new("/ws/src");
    let mut indexer = SimpleIndexer::new(PathBuf::from("/ws"));
    for name in ["a.rs", "b.rs"] {
        let path = format!("/ws/src/{name}");
        indexer.index_cache.insert(path.clone(), cached_entry(&path));
    }
    // Cached entries outside the directory are out of scope for the diff.
    indexer
        .index_cache
        .insert(String::from("/ws/other/z.rs"), cached_entry("/ws/other/z.rs"));

    let cases: &[&[&str]] = &[
        // Same set, reordered: order must not matter.
        &["/ws/src/b.rs", "/ws/src/a.rs"],
        // Added file.
        &["/ws/src/a.rs", "/ws/src/b.rs", "/ws/src/c.rs"],
        // Removed file.
        &["/ws/src/a.rs"],
        // Same count, swapped membership: a count-only check would miss this.
        &["/ws/src/a.rs", "/ws/src/c.rs"],
        // Same count, disjoint membership.
        &["/ws/src/c.rs", "/ws/src/d.rs"],
    ];
    for walked in cases {
        let entries: Vec<FileIndex> = walked.iter().map(|path| cached_entry(path)).collect();
        assert_eq!(
            indexer.cached_subdirectory_differs(dir, &entries),
            naive_sets_differ(&["/ws/src/a.rs", "/ws/src/b.rs"], walked),
            "walked: {walked:?}"
        );
    }
}

#[test]
fn cached_subdirectory_diff_empty_boundaries() {
    let dir = Path::new("/ws/empty");
    let indexer = SimpleIndexer::new(PathBuf::from("/ws"));
    assert!(!indexer.cached_subdirectory_differs(dir, &[]));

    let mut indexer = SimpleIndexer::new(PathBuf::from("/ws"));
    indexer
        .index_cache
        .insert(String::from("/ws/empty/a.rs"), cached_entry("/ws/empty/a.rs"));
    assert!(indexer.cached_subdirectory_differs(dir, &[]));

    let entries = [cached_entry("/ws/empty/a.rs")];
    assert!(!indexer.cached_subdirectory_differs(dir, &entries));
}
