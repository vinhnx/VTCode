//! Minimal few-shot example infrastructure.
//!
//! Implements a keyword-based selector with token-budget enforcement
//! per Section 18.3.3 of the agentic-AI guide:
//!
//! > Few-shot examples improve reliability but consume tokens. The harness
//! > should select relevant examples using embedding similarity to the
//! > current query, rotate examples to avoid overfitting, budget examples
//! > within the model allocation, and cache embeddings of the example
//! > library to avoid recomputation.
//!
//! This module ships the keyword-based selector (no embedding provider
//! dependency) and the token-budget enforcement (via
//! [`vtcode_commons::tokens::estimate_tokens`]). Embedding-based selection
//! and embedding-cache can layer on top later without changing the API.
//!
//! ## Example file format
//!
//! Examples live under `<workspace>/.vtcode/prompts/examples/*.md` or
//! `<home>/.vtcode/prompts/examples/*.md`. The filename stem is the
//! stable id; the file body uses YAML frontmatter for metadata:
//!
//! ```markdown
//! ---
//! id: read-then-edit-large-file
//! tags: [read, edit, large-file, patch]
//! summary: Read a large file in chunks, then patch with apply_patch.
//! ---
//! # User
//! Refactor src/foo.rs to use the new API.
//!
//! # Assistant
//! (the example body showing expected tool sequence)
//! ```
//!
//! Both `tags` and `summary` are optional; the loader derives sensible
//! defaults when they are absent.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use tracing::warn;
use vtcode_commons::tokens::estimate_tokens;

const EXAMPLES_DIR: &str = "examples";
const PROMPTS_PARENT: &str = ".vtcode/prompts";
const MIN_TOKEN_BUDGET: usize = 16;

/// Default token budget for the `[Few-Shot Examples]` block of the system
/// prompt. ~10% of an 8K context window, leaving the remainder for the
/// base prompt, tools, history, and the model's response.
pub const DEFAULT_FEW_SHOT_BUDGET_TOKENS: usize = 800;

/// A single few-shot example loaded from disk.
///
/// The `id` field is the filename stem (e.g. `read-then-edit-large-file`).
/// `tags` drive keyword selection; `summary` is for debug logs; `body` is
/// the raw markdown body that flows verbatim into the prompt. `token_count`
/// is filled on load and refreshed lazily on first selection.
#[derive(Debug, Clone)]
pub struct FewShotExample {
    pub id: String,
    pub tags: Vec<String>,
    pub summary: String,
    pub body: String,
    pub token_count: usize,
    pub source_path: PathBuf,
}

/// YAML frontmatter schema. All fields are optional so authoring mistakes
/// fail soft (loader falls back to derived defaults).
#[derive(Debug, Clone, Default, Deserialize)]
struct FewShotFrontmatter {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    summary: Option<String>,
}

/// Library of few-shot examples discovered from disk.
///
/// Construct via [`FewShotStore::load`] to read from both the workspace
/// `.vtcode/prompts/examples/` and the user `~/.vtcode/prompts/examples/`
/// directories. The constructor is fail-soft: unreadable files or bad
/// frontmatter are skipped with a warning, not a panic.
#[derive(Debug, Clone, Default)]
pub struct FewShotStore {
    examples: Vec<FewShotExample>,
}

impl FewShotStore {
    /// Load all examples from the workspace + home directories. The
    /// workspace directory takes precedence on `id` collision (the home
    /// copy is skipped with a warning).
    ///
    /// Pass `workspace_root = Some("/path")` and `home_dir = None` to
    /// load only the workspace; pass `home_dir = Some("/home/user")` to
    /// also pull user-global examples.
    pub fn load(workspace_root: Option<&Path>, home_dir: Option<&Path>) -> Self {
        let mut by_id: HashMap<String, FewShotExample> = HashMap::new();

        if let Some(home) = home_dir {
            merge_from_dir(&mut by_id, &home.join(PROMPTS_PARENT).join(EXAMPLES_DIR));
        }

        if let Some(workspace) = workspace_root {
            merge_from_dir(&mut by_id, &workspace.join(PROMPTS_PARENT).join(EXAMPLES_DIR));
        }

        let mut examples: Vec<FewShotExample> = by_id.into_values().collect();
        // Stable ordering by id so the same library produces deterministic
        // selections across runs.
        examples.sort_by(|a, b| a.id.cmp(&b.id));
        Self { examples }
    }

    /// Construct a store from an in-memory list. Useful for tests and for
    /// loading examples that come from somewhere other than disk.
    pub fn from_examples(examples: Vec<FewShotExample>) -> Self {
        let mut sorted = examples;
        sorted.sort_by(|a, b| a.id.cmp(&b.id));
        Self { examples: sorted }
    }

    /// Number of loaded examples.
    pub fn len(&self) -> usize {
        self.examples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.examples.is_empty()
    }

    /// Iterate over all loaded examples in deterministic id order.
    pub fn iter(&self) -> impl Iterator<Item = &FewShotExample> {
        self.examples.iter()
    }

    /// Select examples whose tags overlap with the query, capped at
    /// `budget_tokens`. Returns examples in score-descending order
    /// (ties broken by id ascending).
    ///
    /// Scoring rule:
    /// - +1.0 for each exact match between a tag and a query word
    /// - +0.5 for each tag that appears as a substring of the query
    ///
    /// `budget_tokens` of 0 (or any value smaller than the smallest
    /// example) returns an empty selection. Examples whose token_count
    /// alone exceeds the budget are skipped.
    pub fn select(&self, query: &str, budget_tokens: usize) -> Vec<&FewShotExample> {
        if self.examples.is_empty() || budget_tokens < MIN_TOKEN_BUDGET {
            return Vec::new();
        }

        let query_words = tokenize(query);
        if query_words.is_empty() {
            return Vec::new();
        }

        let mut scored: Vec<(f64, &FewShotExample)> = self
            .examples
            .iter()
            .filter_map(|example| {
                let score = score_example(example, &query_words, query);
                if score <= 0.0 {
                    return None;
                }
                Some((score, example))
            })
            .collect();

        // Stable ordering: score desc, then id asc.
        scored.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.1.id.cmp(&b.1.id))
        });

        let mut chosen: Vec<&FewShotExample> = Vec::new();
        let mut used = 0usize;
        for (_, example) in scored {
            if example.token_count == 0 {
                // Defensive: skip examples that failed to size on load.
                continue;
            }
            if used.saturating_add(example.token_count) > budget_tokens {
                continue;
            }
            chosen.push(example);
            used = used.saturating_add(example.token_count);
        }

        chosen
    }
}

/// Render selected examples into the prompt section body. Returns an
/// empty string when `examples` is empty so callers can `writeln!` it
/// unconditionally.
pub fn render_few_shot_section(examples: &[&FewShotExample]) -> String {
    if examples.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    out.push_str(
        "[Few-Shot Examples]\n\
         The following examples illustrate expected behavior for similar tasks.\n\n",
    );
    for example in examples {
        let _ = std::fmt::Write::write_fmt(&mut out, format_args!("### {}\n", example.id));
        if !example.summary.is_empty() {
            let _ = std::fmt::Write::write_fmt(&mut out, format_args!("_{}_\n\n", example.summary));
        }
        let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{}\n\n", example.body.trim_end()));
    }
    out
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn merge_from_dir(by_id: &mut HashMap<String, FewShotExample>, dir: &Path) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
        Err(err) => {
            warn!("few_shot: failed to read directory {}: {err}", dir.display());
            return;
        }
    };

    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("md"))
                    .unwrap_or(false)
        })
        .collect();
    paths.sort();

    for path in paths {
        let Some(stem) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };

        let Ok(raw) = std::fs::read_to_string(&path) else {
            warn!("few_shot: could not read {}", path.display());
            continue;
        };

        if let Some(existing) = by_id.get(stem) {
            warn!(
                "few_shot: duplicate id '{stem}' ({}); keeping earlier {}",
                path.display(),
                existing.source_path.display()
            );
            continue;
        }

        match parse_example(stem, &raw, &path) {
            Ok(example) => {
                by_id.insert(stem.to_string(), example);
            }
            Err(err) => {
                warn!("few_shot: failed to parse {}: {err}", path.display());
            }
        }
    }
}

fn parse_example(fallback_id: &str, raw: &str, source_path: &Path) -> Result<FewShotExample, String> {
    let (frontmatter, body) = parse_frontmatter(raw);
    let id = frontmatter
        .id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback_id.to_string());
    let tags = frontmatter.tags.unwrap_or_default();
    let summary = frontmatter.summary.map(|value| value.trim().to_string()).unwrap_or_default();
    let body = body.trim().to_string();
    let token_count = estimate_tokens(&body);

    Ok(FewShotExample {
        id,
        tags,
        summary,
        body,
        token_count,
        source_path: source_path.to_path_buf(),
    })
}

fn parse_frontmatter(raw: &str) -> (FewShotFrontmatter, String) {
    let trimmed = raw.trim_start_matches('\u{feff}');
    if !trimmed.starts_with("---\n") {
        return (FewShotFrontmatter::default(), raw.to_string());
    }
    let Some(end_rel) = trimmed[4..].find("\n---") else {
        return (FewShotFrontmatter::default(), raw.to_string());
    };
    let yaml = &trimmed[4..4 + end_rel];
    let body_start = 4 + end_rel + "\n---".len();
    let body = trimmed[body_start..].trim_start_matches('\n').to_string();

    let frontmatter = match serde_saphyr::from_str::<FewShotFrontmatter>(yaml) {
        Ok(value) => value,
        Err(err) => {
            warn!("few_shot: frontmatter parse failed: {err}");
            FewShotFrontmatter::default()
        }
    };

    (frontmatter, body)
}

fn tokenize(query: &str) -> Vec<String> {
    query
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

fn score_example(example: &FewShotExample, query_words: &[String], query_raw: &str) -> f64 {
    let query_lower = query_raw.to_ascii_lowercase();
    let mut score = 0.0;
    for tag in &example.tags {
        let tag_lower = tag.to_ascii_lowercase();
        if query_words.iter().any(|word| word == &tag_lower) {
            score += 1.0;
            continue;
        }
        if query_lower.contains(&tag_lower) {
            score += 0.5;
        }
    }
    score
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn example(id: &str, tags: &[&str], body: &str) -> FewShotExample {
        let token_count = estimate_tokens(body);
        FewShotExample {
            id: id.to_string(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            summary: String::new(),
            body: body.to_string(),
            token_count,
            source_path: PathBuf::from(format!("/tmp/{id}.md")),
        }
    }

    #[test]
    fn empty_store_returns_empty_selection() {
        let store = FewShotStore::default();
        assert!(store.select("anything", 1000).is_empty());
    }

    #[test]
    fn zero_budget_returns_empty() {
        let store = FewShotStore::from_examples(vec![example("edit", &["edit"], "do an edit")]);
        assert!(store.select("edit a file", 0).is_empty());
        assert!(store.select("edit a file", 8).is_empty());
    }

    #[test]
    fn matching_tag_is_selected() {
        let store = FewShotStore::from_examples(vec![
            example("edit", &["edit"], "an edit example"),
            example("search", &["search"], "a search example"),
        ]);
        let chosen = store.select("please edit foo.rs", 1000);
        assert_eq!(chosen.len(), 1);
        assert_eq!(chosen[0].id, "edit");
    }

    #[test]
    fn no_overlap_returns_empty() {
        let store = FewShotStore::from_examples(vec![example("deploy", &["deploy", "kubernetes"], "a deploy example")]);
        let chosen = store.select("read a local file", 1000);
        assert!(chosen.is_empty());
    }

    #[test]
    fn example_exceeding_budget_is_skipped() {
        // Two examples, both 50 tokens. Budget = 60 keeps only one.
        let store = FewShotStore::from_examples(vec![
            example("a", &["foo"], &"foo ".repeat(50)),
            example("b", &["foo"], &"foo ".repeat(50)),
        ]);
        let chosen = store.select("foo bar", 60);
        assert_eq!(chosen.len(), 1);
        assert_eq!(chosen[0].id, "a"); // id-asc tiebreak
    }

    #[test]
    fn multiple_matches_sorted_by_score_then_id() {
        let store = FewShotStore::from_examples(vec![
            // "git" matches as both a word and substring, plus "commit".
            example("git-commit", &["git", "commit"], "git commit example"),
            // Only one tag matches.
            example("git-only", &["git"], "git only example"),
            // Different id but same score as git-only.
            example("zzz-other", &["git"], "git other example"),
        ]);
        let chosen = store.select("git commit message", 5000);
        // git-commit scores highest (git + commit).
        assert_eq!(chosen[0].id, "git-commit");
        // The remaining two tie on score, sorted by id asc.
        let rest: Vec<&str> = chosen.iter().skip(1).map(|e| e.id.as_str()).collect();
        assert_eq!(rest, vec!["git-only", "zzz-other"]);
    }

    #[test]
    fn parse_frontmatter_extracts_metadata() {
        let raw = "---\nid: read-then-edit\ntags: [read, edit]\nsummary: read first\n---\nbody here\n";
        let (fm, body) = parse_frontmatter(raw);
        assert_eq!(fm.id.as_deref(), Some("read-then-edit"));
        assert_eq!(fm.tags, Some(vec!["read".to_string(), "edit".to_string()]));
        assert_eq!(fm.summary.as_deref(), Some("read first"));
        // Body keeps a trailing newline (callers should .trim() if needed);
        // parse_example does that for FewShotExample.body.
        assert_eq!(body, "body here\n");
    }

    #[test]
    fn parse_frontmatter_without_metadata_returns_body() {
        let raw = "no frontmatter here\nbody text\n";
        let (fm, body) = parse_frontmatter(raw);
        assert!(fm.id.is_none());
        assert_eq!(body, raw);
    }

    #[test]
    fn render_section_is_empty_when_no_examples() {
        assert!(render_few_shot_section(&[]).is_empty());
    }

    #[test]
    fn render_section_includes_id_and_body() {
        let store =
            FewShotStore::from_examples(vec![example("demo", &["hi", "demo"], "user said hi\nassistant said hello")]);
        let chosen = store.select("hi there", 1000);
        let rendered = render_few_shot_section(&chosen);
        assert!(rendered.contains("[Few-Shot Examples]"));
        assert!(rendered.contains("### demo"));
        assert!(rendered.contains("user said hi"));
    }

    #[test]
    fn load_discovers_examples_from_disk() {
        let workspace = tempfile::tempdir().expect("workspace");
        let examples_dir = workspace.path().join(".vtcode/prompts/examples");
        std::fs::create_dir_all(&examples_dir).expect("mkdir");
        std::fs::write(examples_dir.join("demo.md"), "---\ntags: [demo]\nsummary: a demo\n---\nDemo body\n")
            .expect("write");

        let store = FewShotStore::load(Some(workspace.path()), None);
        assert_eq!(store.len(), 1);
        let only = &store.examples[0];
        assert_eq!(only.id, "demo");
        assert_eq!(only.tags, vec!["demo".to_string()]);
        assert_eq!(only.summary, "a demo");
        assert!(only.body.contains("Demo body"));
        assert!(only.token_count > 0);
    }

    #[test]
    fn load_silently_skips_missing_directories() {
        // No directories exist; load must succeed with zero examples.
        let store = FewShotStore::load(Some(Path::new("/does/not/exist/anywhere")), None);
        assert!(store.is_empty());
    }
}
