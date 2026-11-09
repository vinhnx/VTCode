//! Workspace snapshot: one gitignore-aware walk feeding the file tree.
//!
//! Both file surfaces (the inspector tree and the attachment picker) share a
//! single [`WorkspaceSnapshot`] per refresh so the desktop agrees with the
//! CLI about which files a workspace contains.

use std::path::Path;

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use super::FileEntry;

/// Hard ceiling on snapshot rows. The walk collects the whole pruned tree,
/// sorts it, then keeps this prefix, so the surviving set is deterministic
/// for listings under the hard stop. Hitting the ceiling sets
/// [`WorkspaceSnapshot::truncated`] instead of silently dropping rows.
pub(crate) const SNAPSHOT_MAX_ENTRIES: usize = 5_000;
/// Nesting bound: entries with `depth < SNAPSHOT_MAX_DEPTH` are kept, so a
/// 12-component path is the deepest visible row.
pub(crate) const SNAPSHOT_MAX_DEPTH: usize = 12;
/// Backstop against pathological trees. Collection stops here even before
/// sorting; realistic workspaces never approach it.
const SNAPSHOT_HARD_STOP: usize = 100_000;
/// Desktop-only skips beyond [`vtcode_commons::walk::is_excluded_dir`], whose
/// dependency-output entries (`vendor`, `.next`) stay hidden like
/// `node_modules`/`target` so the browser shows project source, matching CLI
/// search semantics. (`.venv` needs no entry: the hidden-name rule already
/// skips it.) Files outside the snapshot stay reachable through the picker's
/// This-computer tab and its native dialog.
const EXTRA_SKIPPED_DIRS: &[&str] = &["build"];

/// One workspace listing: sorted entries plus whether the walk hit a bound.
///
/// Contract: `entries` is sorted by (parent, directories-first, name); every
/// entry satisfies `depth < SNAPSHOT_MAX_DEPTH`; `truncated` is set exactly
/// when rows were dropped to a bound.
#[derive(Debug)]
pub(crate) struct WorkspaceSnapshot {
    pub(crate) entries: Vec<FileEntry>,
    pub(crate) truncated: bool,
}

/// Parent directory of a snapshot path (`None` for workspace roots), the
/// primary sort key so each tree level stays grouped and ordered.
fn parent_key(rel_path: &str) -> Option<&str> {
    rel_path.rsplit_once('/').map(|(dir, _)| dir)
}

/// List workspace entries in one gitignore-aware walk.
///
/// Respects `.gitignore`/`.ignore`/`.git/exclude` even outside git checkouts,
/// plus the shared exclusion
/// list, skips hidden entries, prunes past [`SNAPSHOT_MAX_DEPTH`], and caps
/// the sorted listing at [`SNAPSHOT_MAX_ENTRIES`]. Walk errors (unreadable
/// directories, non-UTF-8 names) skip their entry; a missing root scans as
/// empty. Returns `None` only when `is_cancelled` trips, in which case no
/// partial listing is published.
///
/// Call from a background task: the walk touches the filesystem.
pub(crate) fn scan_snapshot(root: &Path, is_cancelled: impl Fn() -> bool) -> Option<WorkspaceSnapshot> {
    let mut walker = vtcode_commons::walk::build_walker_single_threaded(root);
    // Search works outside git repos: `.gitignore` and global rules apply to
    // plain folders too, matching CLI file search.
    let _git = walker.require_git(false);
    let _prune = walker.filter_entry(|entry| {
        if entry.depth() == 0 {
            return true;
        }
        if entry.depth() > SNAPSHOT_MAX_DEPTH {
            return false;
        }
        if vtcode_commons::walk::is_excluded_dir(entry) {
            return false;
        }
        let Some(name) = entry.file_name().to_str() else {
            return false;
        };
        if name.starts_with('.') {
            return false;
        }
        if entry.file_type().is_some_and(|kind| kind.is_dir()) && EXTRA_SKIPPED_DIRS.contains(&name) {
            return false;
        }
        true
    });

    let mut entries = Vec::new();
    let mut truncated = false;
    for result in walker.build() {
        if is_cancelled() {
            return None;
        }
        if entries.len() >= SNAPSHOT_HARD_STOP {
            truncated = true;
            break;
        }
        let Ok(entry) = result else {
            continue;
        };
        if entry.depth() == 0 {
            continue;
        }
        if entry.file_name().to_str().is_none() {
            continue;
        }
        let Some(rel) = entry.path().strip_prefix(root).ok().map(|path| path.display().to_string()) else {
            continue;
        };
        entries.push(FileEntry {
            rel_path: rel.replace('\\', "/"),
            // The walker counts the root as depth 0; snapshot rows count the
            // root's children as depth 0.
            depth: entry.depth().saturating_sub(1),
            is_dir: entry.file_type().is_some_and(|kind| kind.is_dir()),
        });
    }
    entries.sort_by(|a, b| {
        parent_key(&a.rel_path)
            .cmp(&parent_key(&b.rel_path))
            .then(b.is_dir.cmp(&a.is_dir))
            .then(a.rel_path.cmp(&b.rel_path))
    });
    if entries.len() > SNAPSHOT_MAX_ENTRIES {
        entries.truncate(SNAPSHOT_MAX_ENTRIES);
        truncated = true;
    }
    debug_assert!(entries.len() <= SNAPSHOT_MAX_ENTRIES);
    Some(WorkspaceSnapshot { entries, truncated })
}

/// One ranked search hit over the workspace snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RankedPath {
    pub(crate) rel_path: String,
    pub(crate) score: usize,
    pub(crate) is_dir: bool,
}

/// Cap on ranked hits handed to the tree. Hits are best-first, so typing
/// more narrows instead of paging.
pub(crate) const SEARCH_MAX_HITS: usize = 200;

/// Ranking bonuses, mirroring the TUI `SearchScorer`
/// (`vtcode-ui/src/tui/core_tui/app/session/file_palette/search.rs`).
const ROOT_LEVEL_BONUS: usize = 1_000;
const PATH_PREFIX_BONUS: usize = 1_000;
const FILENAME_CONTAINS_BONUS: usize = 500;
const FALLBACK_NAME_MATCH_BONUS: usize = 2_000;
const FALLBACK_NAME_PREFIX_BONUS: usize = 200;
const NAME_PREFIX_BONUS: usize = 2_000;
const EXACT_NAME_BONUS: usize = 5_000;
const EXACT_PATH_BONUS: usize = 10_000;
const SEGMENT_MATCH_BONUS: usize = 50;
const DEPTH_PENALTY: usize = 5;
const OCCURRENCE_BONUS: usize = 10;

/// Fuzzy-ranked snapshot search.
///
/// Nucleo path matching first, filename/exact/prefix bonuses next, substring
/// scoring when fuzzy matching misses. Sorted directories-first, score
/// descending, path ascending. The query is trimmed, so blank queries and
/// zero limits yield no hits: browse mode owns the empty query.
pub(crate) fn fuzzy_search(entries: &[FileEntry], query: &str, limit: usize) -> Vec<RankedPath> {
    let query = query.trim().to_lowercase();
    if query.is_empty() || limit == 0 {
        return Vec::new();
    }
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let pattern = Pattern::parse(&query, CaseMatching::Ignore, Normalization::Smart);
    let mut buffer = Vec::new();
    let mut scored = Vec::new();
    for entry in entries {
        let path = entry.rel_path.to_lowercase();
        let mut score = if let Some(fuzzy) = fuzzy_score(&path, &query, &pattern, &mut matcher, &mut buffer) {
            fuzzy
        } else if path.contains(&query) {
            substring_score(&path, &query)
        } else {
            continue;
        };
        if !path.contains('/') {
            score = score.saturating_add(ROOT_LEVEL_BONUS);
        }
        scored.push(RankedPath {
            rel_path: entry.rel_path.clone(),
            score,
            is_dir: entry.is_dir,
        });
    }
    scored.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then(b.score.cmp(&a.score))
            .then(a.rel_path.cmp(&b.rel_path))
    });
    scored.truncate(limit);
    scored
}

/// File-only ranked search for attach flows. Directories filter out before
/// the limit applies, so dir-heavy matches never starve file hits.
pub(crate) fn fuzzy_search_files(entries: &[FileEntry], query: &str, limit: usize) -> Vec<RankedPath> {
    if limit == 0 {
        return Vec::new();
    }
    // `usize::MAX` disables truncation here: hits can never outnumber
    // entries, and the file-only prefix truncates below.
    let mut hits: Vec<RankedPath> = fuzzy_search(entries, query, usize::MAX)
        .into_iter()
        .filter(|hit| !hit.is_dir)
        .collect();
    hits.truncate(limit);
    hits
}

/// Nucleo match plus filename bonuses, or `None` when the path misses.
fn fuzzy_score(
    path: &str,
    query: &str,
    pattern: &Pattern,
    matcher: &mut Matcher,
    buffer: &mut Vec<char>,
) -> Option<usize> {
    let mut score = pattern.score(Utf32Str::new(path, buffer), matcher)? as usize;
    if let Some(filename) = path.rsplit('/').next()
        && filename.contains(query)
    {
        score = score.saturating_add(FILENAME_CONTAINS_BONUS);
    }
    if path == query {
        score = score.saturating_add(EXACT_PATH_BONUS);
    } else if let Some(filename) = path.rsplit('/').next() {
        if filename == query {
            score = score.saturating_add(EXACT_NAME_BONUS);
        } else if filename.starts_with(query) {
            score = score.saturating_add(NAME_PREFIX_BONUS);
        }
    }
    Some(score)
}

/// Substring fallback when fuzzy matching misses a path containing the query.
fn substring_score(path: &str, query: &str) -> usize {
    if path == query {
        return EXACT_PATH_BONUS;
    }
    let mut score = 0_usize;
    if path.starts_with(query) {
        score = score.saturating_add(PATH_PREFIX_BONUS);
    }
    if let Some(file_name) = path.rsplit('/').next() {
        if file_name == query {
            score = score.saturating_add(FALLBACK_NAME_MATCH_BONUS);
        } else if file_name.contains(query) {
            score = score.saturating_add(FILENAME_CONTAINS_BONUS);
        }
        if file_name.starts_with(query) {
            score = score.saturating_add(FALLBACK_NAME_PREFIX_BONUS);
        }
    }
    for segment in path.split('/') {
        if segment.contains(query) {
            score = score.saturating_add(SEGMENT_MATCH_BONUS);
        }
    }
    score = score.saturating_sub(path.matches('/').count().saturating_mul(DEPTH_PENALTY));
    score.saturating_add(path.matches(query).count().saturating_mul(OCCURRENCE_BONUS))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_workspace() -> tempfile::TempDir {
        tempfile::TempDir::new().expect("temp dir")
    }

    fn snapshot_paths(snapshot: &WorkspaceSnapshot) -> Vec<&str> {
        snapshot.entries.iter().map(|entry| entry.rel_path.as_str()).collect()
    }

    #[test]
    fn snapshot_respects_gitignore_and_skips_hidden_and_excluded_dirs() {
        let workspace = temp_workspace();
        let root = workspace.path();
        fs::write(root.join(".gitignore"), "ignored/\n*.log\n").expect("write gitignore");
        for dir in [
            "src",
            "ignored",
            "ignored-keep",
            ".hidden-dir",
            "target/debug",
            "build/output",
            ".venv/lib",
            "node_modules/pkg",
            ".git/objects",
        ] {
            fs::create_dir_all(root.join(dir)).expect("create dir");
        }
        fs::write(root.join("src/main.rs"), b"fn main() {}").expect("write");
        fs::write(root.join("ignored/nested.rs"), b"").expect("write");
        fs::write(root.join("ignored-keep/other.rs"), b"").expect("write");
        fs::write(root.join("debug.log"), b"").expect("write");
        fs::write(root.join(".hidden-file"), b"").expect("write");

        let snapshot = scan_snapshot(root, || false).expect("scan succeeds");
        let paths = snapshot_paths(&snapshot);
        assert!(paths.contains(&"src"));
        assert!(paths.contains(&"src/main.rs"));
        assert!(
            paths.contains(&"ignored-keep/other.rs"),
            "gitignore matches the exact directory, not its name prefix"
        );
        assert!(!paths.iter().any(|path| path.starts_with("ignored/")), "ignored/ is pruned");
        assert!(!paths.contains(&"debug.log"), "ignored patterns prune files too");
        assert!(!paths.iter().any(|path| path.starts_with("target") || path.starts_with("build")));
        assert!(
            !paths
                .iter()
                .any(|path| path.starts_with("node_modules") || path.starts_with(".git"))
        );
        assert!(!paths.contains(&".hidden-file"), "hidden files are skipped");
        assert!(
            !paths.iter().any(|path| path.starts_with(".hidden-dir")),
            "hidden directories are pruned, not just unlisted"
        );
        assert!(!paths.contains(&".gitignore"), "the dot rule covers .gitignore itself");
        assert!(!snapshot.truncated);
        let src = snapshot
            .entries
            .iter()
            .find(|entry| entry.rel_path == "src")
            .expect("src entry");
        assert!(src.is_dir);
        assert_eq!(src.depth, 0);
    }

    #[test]
    fn snapshot_respects_gitignore_without_a_git_checkout() {
        let workspace = temp_workspace();
        let root = workspace.path();
        // No `.git` directory on purpose: ignore rules must apply to plain
        // folders too, matching CLI search semantics.
        fs::write(root.join(".gitignore"), "ignored/\n").expect("write gitignore");
        fs::create_dir_all(root.join("ignored")).expect("create dir");
        fs::create_dir_all(root.join("src")).expect("create dir");
        fs::write(root.join("ignored/nested.rs"), b"").expect("write");
        fs::write(root.join("src/main.rs"), b"").expect("write");

        let snapshot = scan_snapshot(root, || false).expect("scan succeeds");
        let paths = snapshot_paths(&snapshot);
        assert!(paths.contains(&"src/main.rs"));
        assert!(!paths.iter().any(|path| path.starts_with("ignored/")), "gitignore applies outside git checkouts");
    }

    #[test]
    fn snapshot_numbers_depth_from_the_workspace_root() {
        let workspace = temp_workspace();
        let root = workspace.path();
        fs::create_dir_all(root.join("src/nested")).expect("create dirs");
        fs::write(root.join("src/nested/mod.rs"), b"").expect("write");

        let snapshot = scan_snapshot(root, || false).expect("scan succeeds");
        let depth_of = |path: &str| {
            snapshot
                .entries
                .iter()
                .find(|entry| entry.rel_path == path)
                .map(|entry| entry.depth)
        };
        assert_eq!(depth_of("src"), Some(0));
        assert_eq!(depth_of("src/nested"), Some(1));
        assert_eq!(depth_of("src/nested/mod.rs"), Some(2));
    }

    #[test]
    fn snapshot_keeps_directories_before_files_and_sorts_names() {
        let workspace = temp_workspace();
        let root = workspace.path();
        for name in ["z-dir", "a-dir"] {
            fs::create_dir(root.join(name)).expect("create directory");
        }
        for name in ["z.txt", "a.txt"] {
            fs::write(root.join(name), b"x").expect("write file");
        }

        let snapshot = scan_snapshot(root, || false).expect("scan succeeds");
        assert_eq!(snapshot_paths(&snapshot), ["a-dir", "z-dir", "a.txt", "z.txt"]);
    }

    #[test]
    fn snapshot_sorts_each_nesting_level() {
        let workspace = temp_workspace();
        let root = workspace.path();
        fs::create_dir_all(root.join("b-dir/c-sub")).expect("create dirs");
        fs::write(root.join("b-dir/z-child.txt"), b"x").expect("write");
        fs::write(root.join("b-dir/a-child.txt"), b"x").expect("write");
        fs::write(root.join("a-file.txt"), b"x").expect("write");

        let snapshot = scan_snapshot(root, || false).expect("scan succeeds");
        assert_eq!(
            snapshot_paths(&snapshot),
            [
                "b-dir",
                "a-file.txt",
                "b-dir/c-sub",
                "b-dir/a-child.txt",
                "b-dir/z-child.txt"
            ],
        );
    }

    #[test]
    fn snapshot_caps_entries_and_reports_truncation() {
        let workspace = temp_workspace();
        let root = workspace.path();
        for index in 0..(SNAPSHOT_MAX_ENTRIES + 50) {
            fs::write(root.join(format!("file-{index:05}.txt")), b"x").expect("write");
        }

        let snapshot = scan_snapshot(root, || false).expect("scan succeeds");
        assert_eq!(snapshot.entries.len(), SNAPSHOT_MAX_ENTRIES, "entry count is capped");
        assert!(snapshot.truncated, "dropping rows sets the truncation flag");
        assert_eq!(snapshot.entries.first().map(|entry| entry.rel_path.as_str()), Some("file-00000.txt"));
        assert_eq!(
            snapshot.entries.last().map(|entry| entry.rel_path.as_str()),
            Some("file-04999.txt"),
            "the sorted prefix remains stable when the directory is wider than the row limit"
        );
    }

    #[test]
    fn small_snapshot_reports_no_truncation() {
        let workspace = temp_workspace();
        let root = workspace.path();
        fs::write(root.join("only.txt"), b"x").expect("write");

        let snapshot = scan_snapshot(root, || false).expect("scan succeeds");
        assert_eq!(snapshot.entries.len(), 1);
        assert!(!snapshot.truncated);
    }

    #[test]
    fn snapshot_enforces_depth_bound_on_both_sides() {
        let workspace = temp_workspace();
        let root = workspace.path();
        // 12 components: depth 11, the deepest visible row.
        let visible_directory = "a/b/c/d/e/f/g/h/i/j/k/l";
        let beyond_bound = "a/b/c/d/e/f/g/h/i/j/k/l/m";
        fs::create_dir_all(root.join(beyond_bound)).expect("create deep dirs");
        fs::write(root.join(beyond_bound).join("buried.txt"), b"x").expect("write");

        let snapshot = scan_snapshot(root, || false).expect("scan succeeds");
        let paths = snapshot_paths(&snapshot);
        assert!(paths.contains(&visible_directory), "directory at the depth boundary is visible");
        assert!(!paths.contains(&beyond_bound), "directories beyond the depth bound are omitted");
        assert!(!paths.contains(&"a/b/c/d/e/f/g/h/i/j/k/l/m/buried.txt"), "files beyond the depth bound are omitted");
    }

    #[test]
    fn missing_root_scans_as_empty_not_cancelled() {
        let workspace = temp_workspace();
        let snapshot = scan_snapshot(&workspace.path().join("no-such-dir"), || false).expect("scan succeeds");
        assert!(snapshot.entries.is_empty());
        assert!(!snapshot.truncated, "None is reserved for cancellation");
    }

    #[test]
    fn cancelled_snapshot_discards_partial_results() {
        let workspace = temp_workspace();
        for index in 0..100 {
            fs::write(workspace.path().join(format!("file-{index:03}.txt")), b"x").expect("write file");
        }

        let checks = std::cell::Cell::new(0);
        let snapshot = scan_snapshot(workspace.path(), || {
            let checked = checks.get();
            checks.set(checked + 1);
            checked >= 5
        });

        assert!(snapshot.is_none(), "cancelled scans must not publish a partial file tree");
        assert!(checks.get() < 10, "the scanner should stop checking entries promptly");
    }

    #[test]
    fn precancelled_snapshot_returns_none() {
        let workspace = temp_workspace();
        fs::write(workspace.path().join("file.txt"), b"x").expect("write file");
        assert!(scan_snapshot(workspace.path(), || true).is_none());
    }

    fn search_entry(rel_path: &str, is_dir: bool) -> FileEntry {
        FileEntry {
            rel_path: rel_path.to_string(),
            depth: rel_path.matches('/').count(),
            is_dir,
        }
    }

    fn hit_paths(hits: &[RankedPath]) -> Vec<&str> {
        hits.iter().map(|hit| hit.rel_path.as_str()).collect()
    }

    /// Fixture mirrors the TUI `test_exact_match_ranked_first` corpus so the
    /// mirrored bonuses pin the same verified order.
    fn ranking_corpus() -> Vec<FileEntry> {
        [
            "src/domain/main_handler.rs",
            "src/utils/helpers.rs",
            "tests/main_test.rs",
            "main.rs",
            "src/main.rs",
        ]
        .into_iter()
        .map(|path| search_entry(path, false))
        .collect()
    }

    #[test]
    fn search_ranks_exact_filename_first() {
        let entries = ranking_corpus();
        let hits = fuzzy_search(&entries, "main.rs", SEARCH_MAX_HITS);
        assert_eq!(hit_paths(&hits)[..2], ["main.rs", "src/main.rs"]);
        assert!(!hit_paths(&hits).contains(&"src/utils/helpers.rs"), "paths missing the query are excluded");
    }

    #[test]
    fn search_ranks_filename_prefix_above_substring_matches() {
        let entries = ranking_corpus();
        let hits = fuzzy_search(&entries, "main", SEARCH_MAX_HITS);
        assert_eq!(hit_paths(&hits).first(), Some(&"main.rs"));
        assert!(hits.len() >= 4, "every path containing the query is found");
    }

    #[test]
    fn search_lists_directories_before_higher_scoring_files() {
        let entries = vec![search_entry("docs.md", false), search_entry("a/b/docs", true)];
        let hits = fuzzy_search(&entries, "docs", SEARCH_MAX_HITS);
        assert_eq!(hit_paths(&hits), ["a/b/docs", "docs.md"]);
    }

    #[test]
    fn search_treats_blank_query_and_zero_limit_as_browse() {
        let entries = ranking_corpus();
        assert!(fuzzy_search(&entries, "", SEARCH_MAX_HITS).is_empty());
        assert!(fuzzy_search(&entries, "   ", SEARCH_MAX_HITS).is_empty());
        assert!(fuzzy_search(&entries, "main", 0).is_empty());
    }

    #[test]
    fn search_is_case_insensitive_and_reports_no_match() {
        let entries = ranking_corpus();
        let hits = fuzzy_search(&entries, "MAIN", SEARCH_MAX_HITS);
        assert_eq!(hit_paths(&hits).first(), Some(&"main.rs"));
        assert!(fuzzy_search(&entries, "zzz-no-such-file", SEARCH_MAX_HITS).is_empty());
    }

    #[test]
    fn search_respects_limit_best_first() {
        let entries = ranking_corpus();
        let hits = fuzzy_search(&entries, "main.rs", 1);
        assert_eq!(hit_paths(&hits), ["main.rs"]);
    }

    #[test]
    fn file_search_keeps_files_when_directories_outnumber_the_limit() {
        let mut entries = vec![search_entry("notes.md", false)];
        for index in 0..10 {
            entries.push(search_entry(&format!("notes-dir-{index}"), true));
        }
        let hits = fuzzy_search_files(&entries, "notes", 5);
        assert_eq!(hit_paths(&hits), ["notes.md"], "dir filtering precedes truncation");
    }
}
