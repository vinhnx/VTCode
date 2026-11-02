use super::{
    FileIndexCache, FileSearchConfig, MatchType, run, run_bounded_no_follow, run_bounded_no_follow_with_visit,
    run_with_index,
};
use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tempfile::TempDir;

#[tokio::test(flavor = "current_thread")]
async fn concurrent_index_builds_share_async_cache_entry() {
    let workspace = TempDir::new().expect("workspace");
    std::fs::write(workspace.path().join("widget.rs"), "fn widget() {}\n").expect("fixture source");

    let cache = FileIndexCache::new(workspace.path().to_path_buf(), Vec::new(), true, 1);
    let (first, second) = tokio::join!(cache.get_or_build(), cache.get_or_build());
    let first = first.expect("build file index");
    let second = second.expect("reuse file index");

    assert!(Arc::ptr_eq(&first, &second));
}

#[tokio::test(flavor = "current_thread")]
async fn background_refresh_is_safe_when_called_from_tokio() {
    let workspace = TempDir::new().expect("workspace");
    std::fs::write(workspace.path().join("widget.rs"), "fn widget() {}\n").expect("fixture source");

    let cache = FileIndexCache::new(workspace.path().to_path_buf(), Vec::new(), false, 1);
    cache.get_or_build().await.expect("initial index");

    assert!(cache.refresh_background().is_some());
}

#[test]
fn background_refresh_returns_snapshot_without_runtime() {
    let workspace = TempDir::new().expect("workspace");
    std::fs::write(workspace.path().join("widget.rs"), "fn widget() {}\n").expect("fixture source");

    let runtime = tokio::runtime::Runtime::new().expect("Tokio runtime");
    let cache = FileIndexCache::new(workspace.path().to_path_buf(), Vec::new(), false, 1);
    runtime.block_on(cache.get_or_build()).expect("initial index");
    drop(runtime);

    assert!(cache.refresh_background().is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn incremental_directory_updates_use_the_cache_root() {
    let workspace = TempDir::new().expect("workspace");
    std::fs::write(workspace.path().join("widget.rs"), "fn widget() {}\n").expect("fixture source");
    std::fs::create_dir(workspace.path().join("new_directory")).expect("fixture directory");

    let cache = Arc::new(FileIndexCache::new(workspace.path().to_path_buf(), Vec::new(), false, 1));
    cache.get_or_build().await.expect("initial index");

    tokio::task::spawn_blocking({
        let cache = Arc::clone(&cache);
        move || cache.update_file("new_directory", true)
    })
    .await
    .expect("incremental directory update task");

    let index = cache.get_or_build().await.expect("updated index");
    let matches = index.query("new_directory", 16, None);
    assert!(matches.iter().any(|(_, _, match_type)| *match_type == MatchType::Directory));
}

fn indexed_search_config(workspace: &std::path::Path, pattern: &str, cancel_flag: Arc<AtomicBool>) -> FileSearchConfig {
    FileSearchConfig {
        pattern_text: pattern.to_string(),
        limit: NonZero::new(16).expect("non-zero limit"),
        search_directory: workspace.to_path_buf(),
        exclude: Vec::new(),
        threads: NonZero::new(1).expect("non-zero threads"),
        cancel_flag,
        compute_indices: false,
        respect_gitignore: false,
    }
}

fn result_signature(results: &super::FileSearchResults) -> Vec<(u32, String, MatchType)> {
    results
        .matches
        .iter()
        .map(|candidate| (candidate.score, candidate.path.clone(), candidate.match_type))
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn indexed_search_preserves_scores_order_and_match_types() {
    let workspace = TempDir::new().expect("workspace");
    std::fs::create_dir_all(workspace.path().join("src/widget_dir")).expect("fixture directory");
    std::fs::write(workspace.path().join("src/widget.rs"), "fn widget() {}\n").expect("fixture source");
    std::fs::write(workspace.path().join("src/widget_test.rs"), "fn widget_test() {}\n").expect("fixture source");
    std::fs::write(workspace.path().join("README.md"), "widget documentation\n").expect("fixture docs");

    let cache = FileIndexCache::new(workspace.path().to_path_buf(), Vec::new(), false, 1);
    let first =
        run_with_index(indexed_search_config(workspace.path(), "widget", Arc::new(AtomicBool::new(false))), &cache)
            .await
            .expect("indexed search");
    let second =
        run_with_index(indexed_search_config(workspace.path(), "widget", Arc::new(AtomicBool::new(false))), &cache)
            .await
            .expect("repeat indexed search");

    assert_eq!(result_signature(&first), result_signature(&second));
    assert!(
        first
            .matches
            .iter()
            .any(|candidate| candidate.match_type == MatchType::Directory)
    );
    assert!(first.matches.iter().any(|candidate| candidate.match_type == MatchType::File));
    assert!(first.matches.windows(2).all(|window| window[0].score >= window[1].score));
}

#[tokio::test(flavor = "current_thread")]
async fn indexed_search_honors_cancellation_before_scoring() {
    let workspace = TempDir::new().expect("workspace");
    std::fs::write(workspace.path().join("widget.rs"), "fn widget() {}\n").expect("fixture source");
    let cache = FileIndexCache::new(workspace.path().to_path_buf(), Vec::new(), false, 1);
    let cancel_flag = Arc::new(AtomicBool::new(true));

    let results = run_with_index(indexed_search_config(workspace.path(), "widget", cancel_flag), &cache)
        .await
        .expect("cancelled indexed search");
    assert!(results.matches.is_empty());
    assert_eq!(results.total_match_count, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn incremental_updates_do_not_mutate_an_old_search_index() {
    let workspace = TempDir::new().expect("workspace");
    std::fs::write(workspace.path().join("old_widget.rs"), "fn old_widget() {}\n").expect("fixture source");
    let cache = Arc::new(FileIndexCache::new(workspace.path().to_path_buf(), Vec::new(), false, 1));
    let old_index = cache.get_or_build().await.expect("initial index");

    tokio::task::spawn_blocking({
        let cache = Arc::clone(&cache);
        move || cache.update_file("new_widget.rs", true)
    })
    .await
    .expect("incremental update task");

    let old_matches = old_index.query("widget", 16, None);
    let new_index = cache.get_or_build().await.expect("updated index");
    let new_matches = new_index.query("widget", 16, None);
    let old_paths = old_matches
        .iter()
        .filter_map(|(_, path_id, _)| old_index.path_texts_by_id.get(path_id.as_u32() as usize))
        .map(AsRef::as_ref)
        .collect::<Vec<&str>>();
    let new_paths = new_matches
        .iter()
        .filter_map(|(_, path_id, _)| new_index.path_texts_by_id.get(path_id.as_u32() as usize))
        .map(AsRef::as_ref)
        .collect::<Vec<&str>>();

    assert!(!old_paths.contains(&"new_widget.rs"));
    assert!(new_paths.contains(&"new_widget.rs"));
}

fn uncached_search_config(
    workspace: &std::path::Path,
    pattern: &str,
    limit: usize,
    cancel_flag: Arc<AtomicBool>,
) -> FileSearchConfig {
    FileSearchConfig {
        pattern_text: pattern.to_string(),
        limit: NonZero::new(limit).expect("non-zero limit"),
        search_directory: workspace.to_path_buf(),
        exclude: Vec::new(),
        threads: NonZero::new(4).expect("non-zero threads"),
        cancel_flag,
        compute_indices: false,
        respect_gitignore: false,
    }
}

#[test]
fn uncached_parallel_search_aggregates_worker_paths_and_counts() {
    let workspace = TempDir::new().expect("workspace");
    for index in 0..64 {
        std::fs::create_dir(workspace.path().join(format!("widget_dir_{index:03}"))).expect("fixture directory");
        std::fs::write(workspace.path().join(format!("widget_file_{index:03}.rs")), "fn widget() {}\n")
            .expect("fixture source");
    }

    let results = run(uncached_search_config(workspace.path(), "widget", 32, Arc::new(AtomicBool::new(false))))
        .expect("uncached parallel search");

    assert_eq!(results.total_match_count, 128);
    assert_eq!(results.matches.len(), 32);
    let unique_paths = results
        .matches
        .iter()
        .map(|candidate| candidate.path.as_str())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(unique_paths.len(), results.matches.len());
    assert!(results.matches.iter().all(|candidate| candidate.path.contains("widget")));
}

#[test]
fn uncached_parallel_search_honors_cancellation() {
    let workspace = TempDir::new().expect("workspace");
    for index in 0..32 {
        std::fs::write(workspace.path().join(format!("widget_{index:03}.rs")), "fn widget() {}\n")
            .expect("fixture source");
    }
    let cancel_flag = Arc::new(AtomicBool::new(true));

    let results =
        run(uncached_search_config(workspace.path(), "widget", 16, cancel_flag)).expect("cancelled uncached search");

    assert!(results.matches.is_empty());
    assert_eq!(results.total_match_count, 0);
}

#[test]
fn uncached_equal_score_results_are_lexically_ordered_and_repeatable() {
    let workspace = TempDir::new().expect("workspace");
    for name in ["a_x.rs", "b_x.rs", "c_x.rs", "d_x.rs", "e_x.rs"] {
        std::fs::write(workspace.path().join(name), "fn x() {}\n").expect("fixture source");
    }

    let search = || {
        run(uncached_search_config(workspace.path(), "x", 3, Arc::new(AtomicBool::new(false))))
            .expect("uncached equal-score search")
            .matches
            .into_iter()
            .map(|candidate| (candidate.score, candidate.path))
            .collect::<Vec<_>>()
    };

    let expected = search();
    for _ in 0..20 {
        assert_eq!(search(), expected);
    }
    assert!(
        expected
            .windows(2)
            .all(|window| { window[0].0 > window[1].0 || (window[0].0 == window[1].0 && window[0].1 <= window[1].1) })
    );
}

fn bounded_paths(workspace: &std::path::Path) -> Vec<String> {
    run_bounded_no_follow(FileSearchConfig {
        pattern_text: "widget".to_string(),
        limit: NonZero::new(2).expect("non-zero limit"),
        search_directory: workspace.to_path_buf(),
        exclude: Vec::new(),
        threads: NonZero::new(4).expect("non-zero threads"),
        cancel_flag: Arc::new(AtomicBool::new(false)),
        compute_indices: false,
        respect_gitignore: true,
    })
    .expect("bounded path search")
    .matches
    .into_iter()
    .map(|candidate| candidate.path)
    .collect()
}

#[test]
fn bounded_path_selection_is_stable_across_repeated_walks() {
    let workspace = TempDir::new().expect("workspace");
    for directory in ["z", "a", "m", "b", "y"] {
        let directory = workspace.path().join(directory);
        std::fs::create_dir(&directory).expect("fixture directory");
        std::fs::write(directory.join("widget.rs"), "fn widget() {}\n").expect("fixture source");
    }

    let expected = bounded_paths(workspace.path());
    assert_eq!(expected.len(), 2);
    for _ in 0..20 {
        assert_eq!(bounded_paths(workspace.path()), expected);
    }
}

#[test]
fn bounded_path_selection_is_the_sorted_prefix_and_stops_early() {
    let workspace = TempDir::new().expect("workspace");
    for directory in ["z", "a", "m", "b", "y"] {
        let directory = workspace.path().join(directory);
        std::fs::create_dir(&directory).expect("fixture directory");
        std::fs::write(directory.join("widget.rs"), "fn widget() {}\n").expect("fixture source");
    }
    let mut visited = Vec::new();

    let results = run_bounded_no_follow_with_visit(
        FileSearchConfig {
            pattern_text: "widget".to_string(),
            limit: NonZero::new(2).expect("non-zero limit"),
            search_directory: workspace.path().to_path_buf(),
            exclude: Vec::new(),
            threads: NonZero::new(4).expect("non-zero threads"),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            compute_indices: false,
            respect_gitignore: true,
        },
        |path| visited.push(path.to_path_buf()),
    )
    .expect("bounded path search");
    let mut paths = results.matches.into_iter().map(|candidate| candidate.path).collect::<Vec<_>>();
    paths.sort();

    assert_eq!(paths, vec!["a/widget.rs", "b/widget.rs"]);
    assert!(visited.len() < 11, "the bounded route must stop before traversing the complete fixture tree");
    assert_eq!(results.total_match_count, 3);
}
