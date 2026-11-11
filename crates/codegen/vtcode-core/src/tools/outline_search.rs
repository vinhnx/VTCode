//! Bounded, read-only declaration discovery for `code_search`.
//!
//! This replaces the external `ast-grep` binary that VT Code used to
//! download and spawn for declaration outlines. Declarations are recognised
//! with per-language regular expressions over the source text, which keeps the
//! tool self-contained without giving up the `definition` result category.
//!
//! A declaration is a line that *declares* a name — a function, type, class,
//! or constant definition — as opposed to merely mentioning one. The patterns
//! are deliberately conservative: they match declaration keywords rather than
//! any occurrence of the name, so a call site is never reported as a
//! definition.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use anyhow::Result;
use regex::Regex;
use vtcode_commons::canonicalize;
use vtcode_commons::exclusions::is_sensitive_file;
use vtcode_commons::walk::{build_walker_single_threaded, is_excluded_dir};

use super::code_language::CodeLanguage;

/// Maximum number of files scanned for declarations.
const CODE_SEARCH_OUTLINE_FILE_CAP: usize = 512;

/// Maximum number of declarations collected across all files.
const CODE_SEARCH_OUTLINE_DECLARATION_CAP: usize = 1024;

/// Maximum size of a file that may be scanned for declarations.
const CODE_SEARCH_OUTLINE_MAX_FILE_SIZE: u64 = 4 * 1024 * 1024;

/// A byte range within a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeclarationRange {
    /// Byte offset of the range start.
    pub byte_start: usize,
    /// Byte offset of the range end.
    pub byte_end: usize,
}

/// A recognised declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeclarationRecord {
    /// The declared name.
    pub name: String,
    /// The byte range the declaration covers.
    pub range: DeclarationRange,
    /// One-based line the declared name starts on.
    pub line: usize,
    /// One-based byte column the declared name starts at.
    pub column: usize,
}

/// The declarations recognised in one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeclarationFileRecord {
    /// Path of the file.
    pub path: PathBuf,
    /// Language the file was recognised as.
    pub language: CodeLanguage,
    /// The declarations found.
    pub declarations: Vec<DeclarationRecord>,
    /// Whether the file was scanned completely.
    pub complete: bool,
}

/// The outcome of a bounded declaration search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeclarationSearchOutcome {
    /// Per-file declaration records.
    pub files: Vec<DeclarationFileRecord>,
    /// Whether the scan covered every eligible file.
    pub stream_complete: bool,
    /// Whether the scan stopped early.
    pub truncated: bool,
}

/// Whether two names match under smart-case rules.
///
/// A query with an upper-case character matches exactly; an all-lower-case
/// query matches case-insensitively.
fn smart_case_eq(left: &str, query: &str) -> bool {
    if query.chars().any(char::is_uppercase) {
        left == query
    } else {
        left.to_lowercase() == query.to_lowercase()
    }
}

/// The declaration patterns for one language.
///
/// Each pattern captures the declared name in group 1. A language without
/// patterns has no declaration support, which the caller reports as an
/// unavailable result category rather than fabricating definitions.
fn declaration_patterns(language: CodeLanguage) -> &'static [&'static str] {
    match language {
        CodeLanguage::Rust => &[
            r#"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?(?:default\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?(?:extern\s+"[^"]*"\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)"#,
            r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?(?:struct|enum|trait|union)\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?(?:type|const|static)\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"(?m)^\s*impl(?:<[^>]*>)?\s+(?:[A-Za-z_][A-Za-z0-9_:<>, ]*\s+for\s+)?([A-Za-z_][A-Za-z0-9_]*)",
        ],
        CodeLanguage::Python => &[
            r"(?m)^\s*(?:async\s+)?def\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"(?m)^\s*class\s+([A-Za-z_][A-Za-z0-9_]*)",
        ],
        CodeLanguage::JavaScript | CodeLanguage::TypeScript | CodeLanguage::Tsx => &[
            r"(?m)^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s*\*?\s+([A-Za-z_$][A-Za-z0-9_$]*)",
            r"(?m)^\s*(?:export\s+)?(?:abstract\s+)?class\s+([A-Za-z_$][A-Za-z0-9_$]*)",
            r"(?m)^\s*(?:export\s+)?(?:declare\s+)?(?:const|let|var)\s+([A-Za-z_$][A-Za-z0-9_$]*)",
            r"(?m)^\s*(?:export\s+)?(?:declare\s+)?interface\s+([A-Za-z_$][A-Za-z0-9_$]*)",
            r"(?m)^\s*(?:export\s+)?(?:declare\s+)?(?:type|enum|namespace)\s+([A-Za-z_$][A-Za-z0-9_$]*)",
        ],
        CodeLanguage::Go => &[
            r"(?m)^\s*func\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"(?m)^\s*func\s+\([^)]*\)\s*([A-Za-z_][A-Za-z0-9_]*)",
            r"(?m)^\s*type\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"(?m)^\s*(?:const|var)\s+([A-Za-z_][A-Za-z0-9_]*)",
        ],
        CodeLanguage::Java => &[
            r"(?m)^\s*(?:public\s+|private\s+|protected\s+)?(?:static\s+|final\s+|abstract\s+)*class\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"(?m)^\s*(?:public\s+|private\s+|protected\s+)?(?:static\s+|final\s+|abstract\s+)*interface\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"(?m)^\s*(?:public\s+|private\s+|protected\s+)?(?:static\s+|final\s+|abstract\s+)*enum\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"(?m)^\s*(?:public\s+|private\s+|protected\s+)?(?:static\s+|final\s+|abstract\s+|synchronized\s+|native\s+)*[A-Za-z_][A-Za-z0-9_<>\[\], ]*\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(",
        ],
        CodeLanguage::C | CodeLanguage::Cpp => &[
            r"(?m)^\s*(?:struct|union|enum|class)\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"(?m)^\s*typedef\s+.*\b([A-Za-z_][A-Za-z0-9_]*)\s*;",
            r"(?m)^\s*(?:static\s+|inline\s+|extern\s+|const\s+|unsigned\s+|signed\s+)*[A-Za-z_][A-Za-z0-9_]*\s*\*?\s*([A-Za-z_][A-Za-z0-9_]*)\s*\([^;]*\)\s*\{",
        ],
        CodeLanguage::Csharp => &[
            r"(?m)^\s*(?:public\s+|private\s+|protected\s+|internal\s+|static\s+|sealed\s+|abstract\s+|partial\s+)*(?:class|interface|struct|enum|record)\s+([A-Za-z_][A-Za-z0-9_]*)",
        ],
        CodeLanguage::Ruby => &[
            r"(?m)^\s*def\s+(?:self\.)?([A-Za-z_][A-Za-z0-9_]*[?!=]?)",
            r"(?m)^\s*class\s+([A-Za-z_][A-Za-z0-9_:]*)",
            r"(?m)^\s*module\s+([A-Za-z_][A-Za-z0-9_:]*)",
        ],
        CodeLanguage::Php => &[
            r"(?m)^\s*(?:(?:public|private|protected|static|final|abstract)\s+)*function\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"(?m)^\s*(?:abstract\s+|final\s+)*class\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"(?m)^\s*(?:abstract\s+|final\s+)?interface\s+([A-Za-z_][A-Za-z0-9_]*)",
        ],
        CodeLanguage::Kotlin => &[
            r"(?m)^\s*(?:public\s+|private\s+|internal\s+|protected\s+|open\s+|abstract\s+|final\s+|suspend\s+|data\s+|sealed\s+)*(?:class|interface|object|enum)\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"(?m)^\s*(?:public\s+|private\s+|internal\s+|protected\s+|open\s+|override\s+|suspend\s+)*fun\s+([A-Za-z_][A-Za-z0-9_]*)",
        ],
        CodeLanguage::Swift => &[
            r"(?m)^\s*(?:public\s+|private\s+|internal\s+|fileprivate\s+|open\s+|final\s+)*(?:class|struct|enum|protocol|extension)\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"(?m)^\s*(?:public\s+|private\s+|internal\s+|fileprivate\s+|open\s+|static\s+|class\s+|override\s+)*func\s+([A-Za-z_][A-Za-z0-9_]*)",
        ],
        CodeLanguage::Scala => &[
            r"(?m)^\s*(?:final\s+|sealed\s+|abstract\s+|case\s+)*(?:class|object|trait)\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"(?m)^\s*(?:final\s+|implicit\s+|override\s+)*def\s+([A-Za-z_][A-Za-z0-9_]*)",
        ],
        CodeLanguage::Elixir => &[
            r"(?m)^\s*def(?:p)?\s+([A-Za-z_][A-Za-z0-9_]*[?!]?)",
            r"(?m)^\s*defmodule\s+([A-Za-z_][A-Za-z0-9_.:]*)",
        ],
        CodeLanguage::Dart => &[
            r"(?m)^\s*(?:abstract\s+)?class\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"(?m)^\s*(?:enum|mixin|extension)\s+([A-Za-z_][A-Za-z0-9_]*)",
        ],
        CodeLanguage::Lua => &[r"(?m)^\s*(?:local\s+)?function\s+([A-Za-z_][A-Za-z0-9_.:]*)"],
        CodeLanguage::Zig => &[r"(?m)^\s*(?:pub\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)"],
        CodeLanguage::Haskell => &[r"(?m)^([A-Za-z_][A-Za-z0-9_']*)\s*::"],
        CodeLanguage::Solidity => &[
            r"(?m)^\s*(?:abstract\s+|contract\s+|interface\s+|library\s+)([A-Za-z_][A-Za-z0-9_]*)",
            r"(?m)^\s*function\s+([A-Za-z_][A-Za-z0-9_]*)",
        ],
        CodeLanguage::Bash => &[r"(?m)^\s*(?:function\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*\(\)\s*\{"],
        CodeLanguage::Markdown
        | CodeLanguage::Css
        | CodeLanguage::Html
        | CodeLanguage::Json
        | CodeLanguage::Yaml
        | CodeLanguage::Sql
        | CodeLanguage::Dockerfile
        | CodeLanguage::Toml
        | CodeLanguage::Hcl
        | CodeLanguage::Nix
        | CodeLanguage::Protobuf => &[],
    }
}

/// The declaration patterns, compiled once per language.
///
/// Compiling the patterns costs roughly a quarter of a millisecond, and the
/// scanner runs once per file in the walk, so recompiling per file dominated
/// a bounded search. The patterns are static literals, so they are compiled
/// on first use per language and reused afterwards.
static COMPILED_PATTERNS: LazyLock<Mutex<HashMap<CodeLanguage, Option<Vec<Regex>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Compiles the declaration patterns for a language.
///
/// Returns `None` when the language has no declaration patterns, which the
/// caller treats as "no definition support for this language".
fn compile_patterns(language: CodeLanguage) -> Option<Vec<Regex>> {
    // A poisoned cache only means a panicked compilation; the entries that
    // were written before it stay valid, so the guard is recovered rather
    // than propagated.
    let mut cache = COMPILED_PATTERNS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    cache
        .entry(language)
        .or_insert_with(|| compile_declaration_patterns(language))
        .clone()
}

/// Compiles the static declaration patterns for a language.
fn compile_declaration_patterns(language: CodeLanguage) -> Option<Vec<Regex>> {
    let patterns = declaration_patterns(language);
    if patterns.is_empty() {
        return None;
    }
    let compiled = patterns
        .iter()
        .filter_map(|pattern| Regex::new(pattern).ok())
        .collect::<Vec<_>>();
    if compiled.is_empty() { None } else { Some(compiled) }
}

/// Whether a language has declaration support.
///
/// A language without declaration patterns (markdown, yaml, plain text) has
/// no structural anchors, so a semantic anchor falls back to exact text
/// search.
pub(crate) fn supports_declarations(language: CodeLanguage) -> bool {
    !declaration_patterns(language).is_empty()
}

/// The declaration names an anchor refers to.
///
/// The anchor is read with the same grammar that recognises declarations in
/// source, so `@@ fn deploy` names `deploy` and `@@ impl Trait for Type`
/// names `Type` — exactly the name the file-side scanner records for that
/// declaration. An anchor that is not a declaration header names nothing.
pub(crate) fn declaration_names_in_anchor(anchor: &str, language: CodeLanguage) -> Vec<String> {
    let Some(patterns) = compile_patterns(language) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for pattern in &patterns {
        for capture in pattern.captures_iter(anchor) {
            let Some(name) = capture.get(1) else {
                continue;
            };
            names.push(name.as_str().to_string());
        }
    }
    names.sort();
    names.dedup();
    names
}

/// A declaration matched in a source file, before its position is resolved.
struct RawDeclaration {
    /// The declared name.
    name: String,
    /// The byte range the declared name covers.
    range: DeclarationRange,
}

/// Collects every declaration of `language` declared in `source`.
///
/// Returns `None` when the language has no declaration patterns, which the
/// caller reports as "no declaration support" rather than an empty result.
fn collect_raw_declarations(source: &str, language: CodeLanguage) -> Option<Vec<RawDeclaration>> {
    let patterns = compile_patterns(language)?;

    let mut declarations = Vec::new();
    for pattern in &patterns {
        for capture in pattern.captures_iter(source) {
            let Some(name_match) = capture.get(1) else {
                continue;
            };
            declarations.push(RawDeclaration {
                name: name_match.as_str().to_string(),
                // The range covers the declared name itself, so a literal hit
                // elsewhere in the file can be told apart from the declaration.
                range: DeclarationRange {
                    byte_start: name_match.start(),
                    byte_end: name_match.end(),
                },
            });
        }
    }

    declarations.sort_by_key(|declaration| (declaration.range.byte_start, declaration.range.byte_end));
    declarations.dedup_by(|left, right| left.range == right.range);
    Some(declarations)
}

/// Resolves the one-based line and byte column of each declared name.
///
/// The scanner is the single place that maps byte offsets to positions, so
/// every consumer of a declaration agrees on the line and column base.
fn with_positions(source: &str, declarations: Vec<RawDeclaration>) -> Vec<DeclarationRecord> {
    let index = LineIndex::new(source);
    declarations
        .into_iter()
        .map(|declaration| {
            let (line, column) = index.position(declaration.range.byte_start);
            DeclarationRecord {
                name: declaration.name,
                range: declaration.range,
                line,
                column,
            }
        })
        .collect()
}

/// Maps byte offsets in a source file to one-based line and byte column.
///
/// Lines are one-based, and the column counts bytes from the start of the
/// line, matching `rg --column`.
struct LineIndex {
    /// Byte offset of the start of each line.
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(source: &str) -> Self {
        let mut starts = vec![0usize];
        starts.extend(source.match_indices('\n').map(|(index, _)| index + 1));
        Self { starts }
    }

    /// Returns the one-based line and byte column of `offset`.
    fn position(&self, offset: usize) -> (usize, usize) {
        // The number of line starts at or before `offset` is the one-based
        // line number, because `starts[0]` is the first line.
        let line = self.starts.partition_point(|start| *start <= offset).max(1);
        let column = offset - self.starts[line - 1] + 1;
        (line, column)
    }
}

/// Recognises the declarations in `source` that match `query`.
///
/// Returns the matching declarations and whether the scan covered the whole
/// file.
fn scan_declarations(source: &str, language: CodeLanguage, query: &str) -> Option<(Vec<DeclarationRecord>, bool)> {
    let mut matching: Vec<RawDeclaration> = collect_raw_declarations(source, language)?
        .into_iter()
        .filter(|declaration| smart_case_eq(&declaration.name, query))
        .collect();

    let mut complete = true;
    if matching.len() > CODE_SEARCH_OUTLINE_DECLARATION_CAP {
        matching.truncate(CODE_SEARCH_OUTLINE_DECLARATION_CAP);
        complete = false;
    }

    Some((with_positions(source, matching), complete))
}

/// Returns the declarations in `source` whose name matches one of `names`.
///
/// Names are matched case-insensitively, so an anchor like `@@ fn deploy`
/// resolves to a declaration named `deploy` regardless of case.
pub(crate) fn declarations_named(source: &str, language: CodeLanguage, names: &[String]) -> Vec<DeclarationRecord> {
    let Some(raw) = collect_raw_declarations(source, language) else {
        return Vec::new();
    };
    let matching: Vec<RawDeclaration> = raw
        .into_iter()
        .filter(|declaration| names.iter().any(|name| declaration.name.eq_ignore_ascii_case(name)))
        .collect();
    with_positions(source, matching)
}

/// Finds the declarations matching `query` under `resolved_path`.
///
/// The scan is bounded: at most [`CODE_SEARCH_OUTLINE_FILE_CAP`] files are
/// read and at most [`CODE_SEARCH_OUTLINE_DECLARATION_CAP`] declarations are
/// collected.
pub(crate) async fn search_declarations_bounded(
    workspace_root: &Path,
    resolved_path: &Path,
    query: &str,
    languages: &[CodeLanguage],
    candidate_cap: usize,
) -> Result<DeclarationSearchOutcome> {
    let walk_root = if resolved_path.is_file() {
        resolved_path.parent().unwrap_or(workspace_root).to_path_buf()
    } else {
        resolved_path.to_path_buf()
    };

    let mut builder = build_walker_single_threaded(&walk_root);
    builder.filter_entry(|entry| !is_excluded_dir(entry));

    let mut files = Vec::new();
    let mut scanned = 0usize;
    let mut truncated = false;
    let mut stream_complete = true;

    for entry in builder.build().filter_map(std::result::Result::ok) {
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.into_path();
        if path.file_name().and_then(|name| name.to_str()).is_some_and(is_sensitive_file) {
            continue;
        }
        let Some(language) = CodeLanguage::from_path(&path) else {
            continue;
        };
        if !languages.is_empty() && !languages.contains(&language) {
            continue;
        }
        if scanned >= CODE_SEARCH_OUTLINE_FILE_CAP {
            truncated = true;
            stream_complete = false;
            break;
        }
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        if metadata.len() > CODE_SEARCH_OUTLINE_MAX_FILE_SIZE {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some((declarations, complete)) = scan_declarations(&source, language, query) else {
            // The language has no declaration support, so it contributes
            // nothing but also costs no budget.
            continue;
        };
        scanned += 1;
        if declarations.len() > candidate_cap {
            truncated = true;
            stream_complete = false;
        }
        let Ok(canonical) = canonicalize(&path) else {
            continue;
        };
        files.push(DeclarationFileRecord {
            path: canonical,
            language,
            declarations: declarations.into_iter().take(candidate_cap).collect(),
            complete,
        });
        if files.len() >= candidate_cap {
            truncated = true;
            stream_complete = false;
            break;
        }
    }

    Ok(DeclarationSearchOutcome { files, stream_complete, truncated })
}

#[cfg(test)]
mod tests {
    use super::{scan_declarations, search_declarations_bounded};

    use std::fs;

    use tempfile::tempdir;

    use crate::tools::code_language::CodeLanguage;

    #[test]
    fn rust_function_declarations_are_recognised() {
        let source = "fn Widget() {\n    Widget();\n}\nfn other() {}\n";
        let (declarations, complete) = scan_declarations(source, CodeLanguage::Rust, "Widget").expect("rust");

        assert!(complete);
        assert_eq!(declarations.len(), 1, "only the declaration matches, not the call site");
        assert_eq!(declarations[0].name, "Widget");
        // The range covers the declared name, not the whole declaration.
        assert_eq!(declarations[0].range.byte_start, 3);
        assert_eq!(&source[declarations[0].range.byte_start..declarations[0].range.byte_end], "Widget");
    }

    #[test]
    fn smart_case_matching_is_applied_to_declaration_names() {
        let source = "fn Widget() {}\n";
        let (declarations, _) = scan_declarations(source, CodeLanguage::Rust, "widget").expect("rust");
        assert_eq!(declarations.len(), 1, "an all-lower-case query matches case-insensitively");

        let (declarations, _) = scan_declarations(source, CodeLanguage::Rust, "other").expect("rust");
        assert!(declarations.is_empty(), "a different name does not match");
    }

    #[test]
    fn python_and_javascript_declarations_are_recognised() {
        let (declarations, _) =
            scan_declarations("class Widget:\n    pass\n", CodeLanguage::Python, "Widget").expect("python");
        assert_eq!(declarations.len(), 1);

        let (declarations, _) =
            scan_declarations("export function Widget() {}\n", CodeLanguage::JavaScript, "Widget").expect("js");
        assert_eq!(declarations.len(), 1);
    }

    #[test]
    fn languages_without_declaration_support_report_none() {
        assert!(scan_declarations("# Widget\n", CodeLanguage::Markdown, "Widget").is_none());
    }

    #[tokio::test]
    async fn bounded_search_reports_declarations_per_file() {
        let dir = tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join("src")).expect("src directory");
        fs::write(dir.path().join("src/widget.rs"), "fn Widget() {}\n").expect("write source");

        let outcome = search_declarations_bounded(dir.path(), dir.path(), "Widget", &[], 20)
            .await
            .expect("search runs");

        assert_eq!(outcome.files.len(), 1);
        assert_eq!(outcome.files[0].declarations.len(), 1);
        assert_eq!(outcome.files[0].declarations[0].name, "Widget");
    }
}
