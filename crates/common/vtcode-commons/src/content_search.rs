//! Native content search over a workspace tree.
//!
//! This replaces the external `ripgrep` binary that VT Code used to download
//! and spawn. The engine walks the tree with the `ignore` crate (so
//! `.gitignore`, `.ignore`, `.git/exclude`, and `.vtcodegitignore` keep
//! applying), then scans each file's bytes with the `regex` crate.
//!
//! Two matching modes exist, mirroring ripgrep's:
//!
//! - **Literal**: the pattern is escaped and matched verbatim, so regex
//!   metacharacters in the pattern carry no meaning.
//! - **Regex**: the pattern is compiled as a regular expression.
//!
//! Matching is line-oriented: a match is reported with the line that contains
//! it, the byte column of the match within that line, and the file-absolute
//! byte offsets of the match and of the line it sits on. That keeps the
//! result shape identical to ripgrep's `--json` stream, which the tool layer
//! still consumes.

use std::path::{Path, PathBuf};
use std::result::Result;

use regex::Regex;

/// How the search pattern should be interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    /// The pattern is escaped and matched verbatim.
    Literal,
    /// The pattern is compiled as a regular expression.
    Regex,
}

/// Whether matching is case-sensitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseSensitivity {
    /// Match exactly as written.
    Sensitive,
    /// Match regardless of case.
    Insensitive,
    /// Case-insensitive when the pattern is all lower-case, case-sensitive once
    /// it contains an upper-case character.
    Smart,
}

/// A single matching line within a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineMatch {
    /// Path of the file the match was found in.
    pub path: PathBuf,
    /// One-based line number of the match.
    pub line: usize,
    /// One-based byte column of the match start, counted from the start of
    /// the line, matching `rg --column`.
    pub column: usize,
    /// Byte offset of the start of the matching line within the file.
    ///
    /// This is the anchor for the line-relative match offsets, exactly like
    /// ripgrep's `absolute_offset`.
    pub line_start: usize,
    /// Byte offset of the match start within the file.
    pub byte_start: usize,
    /// Byte offset of the match end within the file.
    pub byte_end: usize,
    /// The text of the matching line, without its trailing newline.
    pub line_text: String,
    /// The matched substring itself.
    pub matched_text: String,
    /// Whether the match is a whole word.
    ///
    /// A match is word-bounded when the characters immediately before and
    /// after it are not identifier characters, so `Widget` inside
    /// `WidgetExtra` is not word-bounded.
    pub word_bounded: bool,
}

/// Configuration for a content search.
#[derive(Debug, Clone)]
pub struct SearchRequest {
    /// The pattern to match.
    pub pattern: String,
    /// Root directory (or file) to search.
    pub root: PathBuf,
    /// How to interpret [`SearchRequest::pattern`].
    pub mode: MatchMode,
    /// Whether matching is case-sensitive.
    pub case: CaseSensitivity,
    /// Only report files whose path matches one of these globs.
    ///
    /// Matched case-sensitively, mirroring ripgrep's `--glob`.
    pub include_globs: Vec<String>,
    /// Only report files whose path matches one of these globs.
    ///
    /// Matched case-insensitively, mirroring ripgrep's `--iglob`, which is
    /// what the language filters use so `*.RS` still matches a Rust file.
    pub language_globs: Vec<String>,
    /// Never report files whose path matches one of these globs.
    pub exclude_globs: Vec<String>,
    /// Maximum number of matches to report.
    pub max_results: usize,
    /// Maximum number of matches to report per file.
    pub max_per_file: usize,
    /// Skip files larger than this many bytes.
    pub max_file_size: Option<u64>,
    /// Include files inside hidden directories.
    pub include_hidden: bool,
    /// Honour `.gitignore`, `.ignore`, and `.git/exclude` while walking.
    pub respect_ignore_files: bool,
    /// Report files that contain a match instead of the matching lines.
    pub files_with_matches: bool,
}

impl SearchRequest {
    /// Creates a literal, case-sensitive search request.
    #[must_use]
    pub fn literal(pattern: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        Self {
            pattern: pattern.into(),
            root: root.into(),
            mode: MatchMode::Literal,
            case: CaseSensitivity::Smart,
            include_globs: Vec::new(),
            language_globs: Vec::new(),
            exclude_globs: Vec::new(),
            max_results: 100,
            max_per_file: usize::MAX,
            max_file_size: None,
            include_hidden: false,
            respect_ignore_files: true,
            files_with_matches: false,
        }
    }

    /// Creates a regex, case-sensitive search request.
    #[must_use]
    pub fn regex(pattern: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        Self {
            mode: MatchMode::Regex,
            ..Self::literal(pattern, root)
        }
    }

    /// Sets the case-sensitivity of the search.
    #[must_use]
    pub fn with_case(mut self, case: CaseSensitivity) -> Self {
        self.case = case;
        self
    }

    /// Sets the maximum number of matches reported overall.
    #[must_use]
    pub fn with_max_results(mut self, max_results: usize) -> Self {
        self.max_results = max_results;
        self
    }

    /// Sets the maximum number of matches reported per file.
    #[must_use]
    pub fn with_max_per_file(mut self, max_per_file: usize) -> Self {
        self.max_per_file = max_per_file;
        self
    }

    /// Adds a glob that candidate file paths must match.
    ///
    /// The glob is matched case-sensitively, like ripgrep's `--glob`.
    #[must_use]
    pub fn with_include_glob(mut self, glob: impl Into<String>) -> Self {
        self.include_globs.push(glob.into());
        self
    }

    /// Adds a glob that candidate file paths must match, case-insensitively.
    ///
    /// This mirrors ripgrep's `--iglob`, which the language filters use so a
    /// `*.RS` file still counts as Rust.
    #[must_use]
    pub fn with_language_glob(mut self, glob: impl Into<String>) -> Self {
        self.language_globs.push(glob.into());
        self
    }

    /// Includes hidden files and directories in the walk.
    #[must_use]
    pub fn with_hidden(mut self) -> Self {
        self.include_hidden = true;
        self
    }

    /// Sets the maximum size of a file that may be scanned.
    #[must_use]
    pub fn with_max_file_size(mut self, max_file_size: u64) -> Self {
        self.max_file_size = Some(max_file_size);
        self
    }

    /// Reports only the files that contain a match, not the matching lines.
    #[must_use]
    pub fn with_files_with_matches(mut self) -> Self {
        self.files_with_matches = true;
        self
    }

    /// Excludes files whose path matches `glob`.
    ///
    /// Exclusions are applied after inclusions, so an excluded file never
    /// reaches the scanner even when it also matches an include glob.
    #[must_use]
    pub fn with_exclude_glob(mut self, glob: impl Into<String>) -> Self {
        self.exclude_globs.push(glob.into());
        self
    }

    /// Disables ignore-file handling, so every file in the tree is scanned.
    #[must_use]
    pub fn without_ignore_files(mut self) -> Self {
        self.respect_ignore_files = false;
        self
    }

    /// Compiles the request's pattern into a matcher.
    ///
    /// Returns `Err` when the pattern is not a valid regular expression. A
    /// literal pattern always compiles.
    pub fn compile(&self) -> Result<Regex, regex::Error> {
        let pattern = match self.mode {
            MatchMode::Literal => regex::escape(&self.pattern),
            MatchMode::Regex => self.pattern.clone(),
        };
        let pattern = match self.case {
            CaseSensitivity::Sensitive => pattern,
            CaseSensitivity::Insensitive => format!("(?i){pattern}"),
            // Smart case only relaxes the match when the pattern carries no
            // upper-case character, so an explicit `Foo` still matches `Foo`.
            CaseSensitivity::Smart => {
                if self.pattern.chars().any(char::is_uppercase) {
                    pattern
                } else {
                    format!("(?i){pattern}")
                }
            }
        };
        Regex::new(&pattern)
    }
}

/// Errors that can occur while running a content search.
#[derive(Debug)]
pub enum SearchError {
    /// The pattern was not a valid regular expression.
    InvalidPattern(regex::Error),
}

impl std::fmt::Display for SearchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPattern(error) => write!(formatter, "invalid search pattern: {error}"),
        }
    }
}

impl std::error::Error for SearchError {}

impl From<regex::Error> for SearchError {
    fn from(error: regex::Error) -> Self {
        Self::InvalidPattern(error)
    }
}

/// Runs `request` against the tree and returns the matching lines.
///
/// The walk stops once `request.max_results` matches have been collected, so a
/// broad pattern over a large tree does not scan the whole repository.
pub fn search(request: &SearchRequest) -> Result<Vec<LineMatch>, SearchError> {
    let matcher = request.compile()?;
    let mut matches = Vec::new();

    // The candidate files are sorted before scanning so that a capped result
    // set is a stable, path-ordered prefix rather than whichever files the
    // directory walk happened to reach first.
    let mut candidates = candidate_files(request);
    candidates.sort();

    for path in candidates {
        // The remaining budget bounds both the per-file cap and the total, so
        // one large file cannot push the result set past `max_results`.
        let remaining = request.max_results.saturating_sub(matches.len());
        if remaining == 0 {
            break;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        // In path-only mode one hit per file is enough, so the scan stops at
        // the first match instead of collecting every hit in the file.
        let per_file_cap = if request.files_with_matches {
            1
        } else {
            request.max_per_file.min(remaining)
        };
        let file_matches = scan_file(&path, &content, &matcher, per_file_cap);
        if file_matches.is_empty() {
            continue;
        }
        if request.files_with_matches {
            // Only the path matters in this mode, so one record per file is
            // kept. The record is the file's first real match, so every field
            // stays truthful instead of pointing at a fabricated offset.
            matches.extend(file_matches.into_iter().next());
        } else {
            matches.extend(file_matches);
        }
    }

    // Sorting by path makes a capped result set a stable prefix rather than
    // whatever order the directory walk happened to produce.
    matches.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.line.cmp(&right.line))
            .then_with(|| left.column.cmp(&right.column))
    });
    Ok(matches)
}

/// Walks the request's root and yields the files eligible for scanning.
fn candidate_files(request: &SearchRequest) -> Vec<PathBuf> {
    // A search root that is a single file is scanned directly: the walker
    // only yields directories, so a file root would otherwise produce nothing.
    if request.root.is_file() {
        return vec![request.root.clone()];
    }
    let mut builder = crate::walk::build_default_walker(&request.root);
    let _ = builder.hidden(!request.include_hidden);
    let _ = builder.filter_entry(|entry| !crate::walk::is_excluded_dir(entry));
    if !request.respect_ignore_files {
        let _ = builder.git_ignore(false);
        let _ = builder.git_global(false);
        let _ = builder.git_exclude(false);
        let _ = builder.ignore(false);
        let _ = builder.parents(false);
    }

    // The language globs are compared case-insensitively, so they are
    // lower-cased once here instead of once per candidate file.
    let language_globs = lowercase_globs(&request.language_globs);

    builder
        .build()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
        .map(|entry| entry.into_path())
        .filter(|path| {
            // The relative path is empty when the search root is the file
            // itself, so the file name is used instead: a glob must still be
            // able to select the file that was searched directly.
            let relative = path.strip_prefix(&request.root).unwrap_or(path);
            let relative = if relative.as_os_str().is_empty() {
                path
            } else {
                relative
            };
            path_matches_include_globs(relative, &request.include_globs)
                && path_matches_language_globs(relative, &language_globs)
                && !path_matches_exclude_globs(relative, &request.exclude_globs)
        })
        .filter(|path| {
            request
                .max_file_size
                .is_none_or(|limit| std::fs::metadata(path).map(|meta| meta.len() <= limit).unwrap_or(false))
        })
        .collect()
}

/// Whether `relative_path` matches any of the include `globs`.
///
/// An empty list includes everything. Globs follow gitignore-style rules: a
/// pattern without a slash matches the file name at any depth (`*.rs`), while
/// a pattern with a slash matches the whole relative path (`src/**`).
fn path_matches_include_globs(relative_path: &Path, globs: &[String]) -> bool {
    if globs.is_empty() {
        return true;
    }
    globs.iter().any(|glob| glob_matches_path(relative_path, glob))
}

/// Lower-cases a list of globs so they can be matched case-insensitively.
fn lowercase_globs(globs: &[String]) -> Vec<String> {
    globs.iter().map(|glob| glob.to_ascii_lowercase()).collect()
}

/// Whether `relative_path` matches any of the case-insensitive language
/// globs.
///
/// Both sides are lower-cased — the globs by [`lowercase_globs`], the path by
/// [`lowercase_relative_path`] — so `*.RS` matches `main.rs`.
fn path_matches_language_globs(relative_path: &Path, globs: &[String]) -> bool {
    if globs.is_empty() {
        return true;
    }
    let Some(lowered) = lowercase_relative_path(relative_path) else {
        return false;
    };
    let lowered = Path::new(&lowered);
    globs.iter().any(|glob| {
        glob_matches_path(lowered, glob)
            // A language glob is an extension filter, so it must also match
            // the file name when the relative path carries directories.
            || lowered.file_name().is_some_and(|name| glob_matches_path(Path::new(name), glob))
    })
}

/// Lower-cases a relative path so it can be matched case-insensitively.
///
/// The result keeps the directory separators so it can be re-parsed as a
/// path, and it is empty only when the relative path itself is empty.
fn lowercase_relative_path(relative_path: &Path) -> Option<String> {
    let mut lowered = String::new();
    for component in relative_path.components() {
        let std::path::Component::Normal(name) = component else {
            return None;
        };
        if !lowered.is_empty() {
            lowered.push('/');
        }
        lowered.push_str(&name.to_string_lossy().to_ascii_lowercase());
    }
    Some(lowered)
}
/// Whether `relative_path` is excluded by any of the exclude `globs`.
///
/// An empty list excludes nothing. Globs follow the same rules as includes, so
/// `**/target/**` excludes a `target` directory at any depth.
fn path_matches_exclude_globs(relative_path: &Path, globs: &[String]) -> bool {
    globs.iter().any(|glob| glob_matches_path(relative_path, glob))
}

/// Whether a character can appear inside an identifier.
fn is_identifier_char(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

/// Whether `relative_path` matches a single gitignore-style glob.
///
/// `**` matches any number of path components, `*` matches any run of
/// characters within one component, and `?` matches exactly one character. A
/// pattern without a slash is matched against the file name alone.
fn glob_matches_path(relative_path: &Path, glob: &str) -> bool {
    let glob = glob.trim_start_matches('/');
    let pattern_components: Vec<&str> = glob.split('/').collect();
    let path_components: Vec<String> = relative_path
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(name) => name.to_str().map(str::to_owned),
            _ => None,
        })
        .collect();

    if !glob.contains('/') {
        let Some(name) = path_components.last() else {
            return false;
        };
        return match_components(&pattern_components, &[name.as_str()]);
    }

    let path_components: Vec<&str> = path_components.iter().map(String::as_str).collect();
    match_components(&pattern_components, &path_components)
}

/// Matches `pattern` against `path` component by component.
fn match_components(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => {
            // `**` matches zero or more components.
            (0..=path.len()).any(|skip| match_components(rest, path.get(skip..).unwrap_or_default()))
        }
        Some((&head, rest)) => match path.split_first() {
            Some((&first, tail)) if component_matches(head, first) => match_components(rest, tail),
            _ => false,
        },
    }
}

/// Whether one path component matches one pattern component.
fn component_matches(pattern: &str, component: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let component: Vec<char> = component.chars().collect();
    component_glob_matches(&pattern, &component)
}

/// Matches a single-component glob against a single-component name.
fn component_glob_matches(pattern: &[char], component: &[char]) -> bool {
    match pattern.split_first() {
        None => component.is_empty(),
        Some((&'*', rest)) => {
            // `*` matches zero or more characters.
            (0..=component.len()).any(|skip| component_glob_matches(rest, component.get(skip..).unwrap_or_default()))
        }
        Some((&'?', rest)) => match component.split_first() {
            Some((_, tail)) => component_glob_matches(rest, tail),
            None => false,
        },
        Some((&head, rest)) => match component.split_first() {
            Some((&first, tail)) if head == first => component_glob_matches(rest, tail),
            _ => false,
        },
    }
}

/// Scans one file's content, returning its matching lines.
fn scan_file(path: &Path, content: &str, matcher: &Regex, max_per_file: usize) -> Vec<LineMatch> {
    let mut matches = Vec::new();
    // The offsets are reported relative to the file, not the line, so a caller
    // can compare a hit against a declaration range in the same file.
    let mut line_start = 0usize;

    for (index, line) in content.split_inclusive('\n').enumerate() {
        let line_without_newline = line.trim_end_matches(['\n', '\r']);
        for found in matcher.find_iter(line_without_newline) {
            let matched_text = found.as_str();
            // The column counts bytes from the start of the line, matching
            // `rg --column`, so the column indexes into the line text.
            let column = found.start() + 1;
            let before = line_without_newline
                .get(..found.start())
                .and_then(|text| text.chars().next_back());
            let after = line_without_newline.get(found.end()..).and_then(|text| text.chars().next());
            let word_bounded = !before.is_some_and(is_identifier_char) && !after.is_some_and(is_identifier_char);
            matches.push(LineMatch {
                path: path.to_path_buf(),
                line: index + 1,
                column,
                line_start,
                byte_start: line_start + found.start(),
                byte_end: line_start + found.end(),
                line_text: line_without_newline.to_string(),
                matched_text: matched_text.to_string(),
                word_bounded,
            });
            if matches.len() >= max_per_file {
                return matches;
            }
        }
        line_start += line.len();
    }

    matches
}

#[cfg(test)]
mod tests {
    use super::{CaseSensitivity, MatchMode, SearchRequest, glob_matches_path, search};

    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    fn fixture() -> tempfile::TempDir {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("a.rs"), "fn main() {}\nlet needle = 1;\n").expect("write a.rs");
        fs::write(dir.path().join("b.txt"), "needle in a text file\n").expect("write b.txt");
        dir
    }

    #[test]
    fn literal_mode_matches_metacharacters_verbatim() {
        let dir = fixture();
        let request = SearchRequest::literal("needle =", dir.path().to_path_buf());
        let matches = search(&request).expect("search runs");

        assert_eq!(matches.len(), 1, "only the literal occurrence matches");
        assert_eq!(matches[0].path.file_name().and_then(|name| name.to_str()), Some("a.rs"));
        assert_eq!(matches[0].line, 2);
        assert_eq!(matches[0].matched_text, "needle =");
    }

    #[test]
    fn regex_mode_treats_pattern_as_a_regular_expression() {
        let dir = fixture();
        let request = SearchRequest::regex(r"needle\s+in", dir.path().to_path_buf());
        let matches = search(&request).expect("search runs");

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].path.file_name().and_then(|name| name.to_str()), Some("b.txt"));
    }

    #[test]
    fn smart_case_relaxes_lowercase_patterns_only() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("case.txt"), "Needle needle\n").expect("write");

        let lower = SearchRequest::literal("needle", dir.path().to_path_buf()).with_case(CaseSensitivity::Smart);
        assert_eq!(search(&lower).expect("search runs").len(), 2, "lower-case pattern matches both cases");

        let upper = SearchRequest::literal("Needle", dir.path().to_path_buf()).with_case(CaseSensitivity::Smart);
        assert_eq!(search(&upper).expect("search runs").len(), 1, "upper-case pattern stays case-sensitive");
    }

    #[test]
    fn explicit_case_sensitivity_overrides_smart_case() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("case.txt"), "Needle needle\n").expect("write");

        let request =
            SearchRequest::literal("Needle", dir.path().to_path_buf()).with_case(CaseSensitivity::Insensitive);
        assert_eq!(search(&request).expect("search runs").len(), 2);
    }

    #[test]
    fn language_globs_match_case_insensitively_in_both_directions() {
        // Both sides of the comparison are lower-cased, so the case of the
        // glob and the case of the file name must not decide the outcome.
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("main.rs"), "needle\n").expect("write lower-case file");
        fs::write(dir.path().join("OTHER.RS"), "needle\n").expect("write upper-case file");

        let upper_glob = SearchRequest::literal("needle", dir.path().to_path_buf()).with_language_glob("*.RS");
        assert_eq!(search(&upper_glob).expect("search runs").len(), 2, "an upper-case glob matches both files");

        let lower_glob = SearchRequest::literal("needle", dir.path().to_path_buf()).with_language_glob("*.rs");
        assert_eq!(search(&lower_glob).expect("search runs").len(), 2, "a lower-case glob matches both files");
    }

    #[test]
    fn include_globs_restrict_the_scanned_files() {
        let dir = fixture();
        let request = SearchRequest::literal("needle", dir.path().to_path_buf()).with_include_glob("*.rs");
        let matches = search(&request).expect("search runs");

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].path.file_name().and_then(|name| name.to_str()), Some("a.rs"));
    }

    #[test]
    fn max_results_stops_the_search_early() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("many.txt"), "needle\nneedle\nneedle\n").expect("write");

        let request = SearchRequest::literal("needle", dir.path().to_path_buf()).with_max_results(2);
        let matches = search(&request).expect("search runs");

        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn max_per_file_caps_matches_within_one_file() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("many.txt"), "needle\nneedle\nneedle\n").expect("write");

        let request = SearchRequest::literal("needle", dir.path().to_path_buf()).with_max_per_file(1);
        let matches = search(&request).expect("search runs");

        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn files_with_matches_reports_each_file_once() {
        let dir = fixture();
        let mut request = SearchRequest::literal("needle", dir.path().to_path_buf());
        request.files_with_matches = true;
        let matches = search(&request).expect("search runs");

        assert_eq!(matches.len(), 2, "one entry per matching file");
    }

    #[test]
    fn files_with_matches_honours_the_result_cap() {
        let dir = tempdir().expect("tempdir");
        for index in 0..4 {
            fs::write(dir.path().join(format!("file{index}.txt")), "needle\n").expect("write");
        }

        let request = SearchRequest::literal("needle", dir.path().to_path_buf())
            .with_max_results(2)
            .with_files_with_matches();
        let matches = search(&request).expect("search runs");

        assert_eq!(matches.len(), 2, "the cap bounds the number of reported files");
    }

    #[test]
    fn files_with_matches_reports_a_real_position_rather_than_a_placeholder() {
        let dir = tempdir().expect("tempdir");
        // The hit is on the second line, so a fabricated `line: 1` record would
        // disagree with the file the caller would open.
        fs::write(dir.path().join("a.txt"), "first\nsecond needle\n").expect("write");

        let mut request = SearchRequest::literal("needle", dir.path().to_path_buf());
        request.files_with_matches = true;
        let matches = search(&request).expect("search runs");

        assert_eq!(matches.len(), 1);
        let hit = &matches[0];
        assert_eq!(hit.line, 2, "the record points at the line that matched");
        assert_eq!(hit.column, 8, "and at the byte column of the match");
        assert_eq!(hit.line_text, "second needle");
        assert_eq!(hit.matched_text, "needle");
        assert_eq!(
            "first\nsecond needle\n".get(hit.byte_start..hit.byte_end),
            Some("needle"),
            "the offsets still index into the file"
        );
    }

    #[test]
    fn files_with_matches_reports_each_file_once_when_a_file_has_several_hits() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("many.txt"), "needle\nneedle\nneedle\n").expect("write");
        fs::write(dir.path().join("one.txt"), "needle\n").expect("write");

        let mut request = SearchRequest::literal("needle", dir.path().to_path_buf());
        request.files_with_matches = true;
        let matches = search(&request).expect("search runs");

        assert_eq!(matches.len(), 2, "one entry per matching file, not per hit");
    }

    #[test]
    fn word_bounded_marks_exact_identifier_occurrences() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("words.txt"), "Widget WidgetExtra\n").expect("write");

        let request = SearchRequest::literal("Widget", dir.path().to_path_buf());
        let matches = search(&request).expect("search runs");

        assert_eq!(matches.len(), 2);
        assert!(matches[0].word_bounded, "the standalone occurrence is word-bounded");
        assert!(!matches[1].word_bounded, "the occurrence inside a longer word is not");
    }

    #[test]
    fn byte_offsets_are_relative_to_the_file() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("offsets.txt"), "first\nsecond needle\n").expect("write");

        let request = SearchRequest::literal("needle", dir.path().to_path_buf());
        let matches = search(&request).expect("search runs");

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].line, 2);
        assert_eq!(
            "first\nsecond needle\n"
                .get(matches[0].byte_start..matches[0].byte_end)
                .expect("the offsets index into the file"),
            "needle",
            "the offsets must index into the file, not the line"
        );
    }

    #[test]
    fn column_is_counted_in_bytes() {
        let dir = tempdir().expect("tempdir");
        // "éé needle": the two `é` take four bytes and the space one more, so
        // the match starts at byte offset 5 and ripgrep reports column 6.
        fs::write(dir.path().join("unicode.txt"), "éé needle\n").expect("write");

        let request = SearchRequest::literal("needle", dir.path().to_path_buf());
        let matches = search(&request).expect("search runs");

        assert_eq!(matches[0].column, 6, "column counts bytes, not characters");
        assert_eq!(
            matches[0].line_text.as_bytes().get(matches[0].column - 1..),
            Some(b"needle".as_slice()),
            "the column must index into the line text"
        );
    }

    #[test]
    fn multibyte_column_matches_ripgrep() {
        let dir = tempdir().expect("tempdir");
        // "中文needle": the two CJK characters take six bytes, so the match
        // starts at byte offset 6 and ripgrep reports column 7.
        fs::write(dir.path().join("cjk.txt"), "中文needle\n").expect("write");

        let request = SearchRequest::literal("needle", dir.path().to_path_buf());
        let matches = search(&request).expect("search runs");

        assert_eq!(matches[0].column, 7, "column counts bytes, matching `rg --column`");
    }

    #[test]
    fn line_start_marks_the_start_of_the_matching_line() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("offsets.txt"), "first\nsecond needle\n").expect("write");

        let request = SearchRequest::literal("needle", dir.path().to_path_buf());
        let matches = search(&request).expect("search runs");

        assert_eq!(matches[0].line_start, 6, "the second line starts after \"first\\n\"");
        assert_eq!(
            matches[0].byte_start - matches[0].line_start,
            matches[0].column - 1,
            "the file offset and the line offset must agree"
        );
    }

    #[test]
    fn invalid_regex_is_reported_as_an_error() {
        let dir = fixture();
        let request = SearchRequest::regex("(unclosed", dir.path().to_path_buf());

        assert!(search(&request).is_err(), "an invalid regex must not silently match nothing");
    }

    #[test]
    fn glob_matching_handles_wildcards_and_prefixes() {
        assert!(glob_matches_path(Path::new("main.rs"), "*.rs"));
        assert!(!glob_matches_path(Path::new("main.txt"), "*.rs"));
        assert!(glob_matches_path(Path::new("main.rs"), "**/*.rs"));
        assert!(glob_matches_path(Path::new("main.rs"), "main.rs"));
        assert!(glob_matches_path(Path::new("a/b/main.rs"), "**/*.rs"));
        assert!(!glob_matches_path(Path::new("a/b/main.txt"), "**/*.rs"));
        assert!(glob_matches_path(Path::new("target/x/y.rs"), "**/target/**"));
    }

    #[test]
    fn exclude_globs_remove_matching_files() {
        let dir = fixture();
        let request = SearchRequest::literal("needle", dir.path().to_path_buf()).with_exclude_glob("*.txt");
        let matches = search(&request).expect("search runs");

        assert_eq!(matches.len(), 1, "the excluded text file must not be scanned");
        assert_eq!(matches[0].path.file_name().and_then(|name| name.to_str()), Some("a.rs"));
    }

    #[test]
    fn exclude_globs_match_directory_patterns_at_any_depth() {
        let dir = tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join("target/nested")).expect("nested directory");
        fs::write(dir.path().join("target/nested/build.rs"), "needle\n").expect("write nested");
        fs::write(dir.path().join("kept.rs"), "needle\n").expect("write kept");

        let request = SearchRequest::literal("needle", dir.path().to_path_buf()).with_exclude_glob("**/target/**");
        let matches = search(&request).expect("search runs");

        assert_eq!(matches.len(), 1, "the excluded directory must not be scanned");
        assert_eq!(matches[0].path.file_name().and_then(|name| name.to_str()), Some("kept.rs"));
    }

    #[test]
    fn gitignored_files_are_skipped_by_default() {
        let dir = tempdir().expect("tempdir");
        // The `ignore` crate only applies git rules inside a git repository,
        // so the fixture needs the same `.git` marker the workspace fixtures use.
        fs::create_dir(dir.path().join(".git")).expect("git marker");
        fs::write(dir.path().join(".gitignore"), "skipped.rs\n").expect("write ignore file");
        fs::write(dir.path().join("skipped.rs"), "needle\n").expect("write skipped");
        fs::write(dir.path().join("kept.rs"), "needle\n").expect("write kept");

        let request = SearchRequest::literal("needle", dir.path().to_path_buf());
        let matches = search(&request).expect("search runs");

        assert_eq!(matches.len(), 1, "the ignored file must not be scanned");
        assert_eq!(matches[0].path.file_name().and_then(|name| name.to_str()), Some("kept.rs"));
    }

    #[test]
    fn ignore_files_can_be_disabled() {
        let dir = tempdir().expect("tempdir");
        fs::create_dir(dir.path().join(".git")).expect("git marker");
        fs::write(dir.path().join(".gitignore"), "skipped.rs\n").expect("write ignore file");
        fs::write(dir.path().join("skipped.rs"), "needle\n").expect("write skipped");

        let request = SearchRequest::literal("needle", dir.path().to_path_buf()).without_ignore_files();
        let matches = search(&request).expect("search runs");

        assert_eq!(matches.len(), 1, "disabling ignore files scans the ignored file");
    }

    #[test]
    fn max_file_size_skips_large_files() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("big.txt"), "needle\n".repeat(100)).expect("write big");

        let request = SearchRequest::literal("needle", dir.path().to_path_buf()).with_max_file_size(16);
        let matches = search(&request).expect("search runs");

        assert!(matches.is_empty(), "a file over the size limit must not be scanned");
    }

    #[test]
    fn request_defaults_to_literal_mode() {
        let request = SearchRequest::literal("x", "/tmp");
        assert_eq!(request.mode, MatchMode::Literal);
    }
}
