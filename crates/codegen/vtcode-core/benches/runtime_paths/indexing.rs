use std::{
    hint::black_box,
    num::NonZero,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

use criterion::{BenchmarkId, Criterion};
use vtcode_indexer::file_search::{FileSearchConfig, run_bounded_no_follow};

fn config(root: &Path, pattern: &str, cancelled: bool) -> FileSearchConfig {
    FileSearchConfig {
        pattern_text: pattern.to_string(),
        limit: NonZero::new(17).expect("result cap"),
        search_directory: root.to_path_buf(),
        exclude: Vec::new(),
        threads: NonZero::new(4).expect("worker count"),
        cancel_flag: Arc::new(AtomicBool::new(cancelled)),
        compute_indices: true,
        respect_gitignore: false,
    }
}

pub(super) fn benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("workspace_search");
    for file_count in [8_usize, 256, 4_096] {
        for deep in [false, true] {
            let workspace = tempfile::tempdir().expect("search workspace");
            let depth = if deep { file_count.min(64) } else { 0 };
            for index in 0..file_count {
                let mut directory = workspace.path().to_path_buf();
                for level in 0..index % depth.max(1) {
                    directory.push(format!("level_{level:02}"));
                }
                std::fs::create_dir_all(&directory).expect("search directory");
                std::fs::write(directory.join(format!("widget_{index:04}.rs")), "fn widget() {}\n")
                    .expect("search file");
            }
            // This is the same bounded, no-follow walker used by the tool bridge.
            // The existing agent_harness index-cache cases are library API measurements.
            let scan = || {
                let results = run_bounded_no_follow(config(workspace.path(), "widget", false)).expect("bounded search");
                assert_eq!(results.matches.len(), file_count.min(17));
                // At the cap, the bounded walker reports one extra match as
                // its truncation signal without scanning the complete tree.
                assert_eq!(results.total_match_count, if file_count < 17 { file_count } else { 18 });
                black_box(results)
            };
            drop(scan());
            let shape = if deep { "deep" } else { "wide" };
            let _ =
                group.bench_function(BenchmarkId::new(format!("{shape}_warm_metadata"), file_count), |b| b.iter(&scan));
            let _ = group.bench_function(BenchmarkId::new(format!("{shape}_cancelled"), file_count), |b| {
                b.iter(|| {
                    let results =
                        run_bounded_no_follow(config(workspace.path(), "widget", true)).expect("cancelled search");
                    assert!(results.matches.is_empty());
                    black_box(results)
                })
            });
            let sentinel = workspace.path().join("sentinel_unique.rs");
            let _ = group.bench_function(BenchmarkId::new(format!("{shape}_visible_mutation"), file_count), |b| {
                b.iter(|| {
                    std::fs::write(&sentinel, "fresh file\n").expect("create sentinel");
                    let added = run_bounded_no_follow(config(workspace.path(), "sentinel_unique", false))
                        .expect("new file search");
                    assert_eq!(added.matches.len(), 1);
                    assert_eq!(added.matches.first().expect("new match").path, "sentinel_unique.rs");
                    std::fs::remove_file(&sentinel).expect("remove owned sentinel");
                    let removed = run_bounded_no_follow(config(workspace.path(), "sentinel_unique", false))
                        .expect("removed file search");
                    assert!(removed.matches.is_empty());
                    black_box((added, removed))
                })
            });
        }
    }
    group.finish();
}
