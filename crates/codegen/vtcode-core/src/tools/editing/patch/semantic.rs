//! Native semantic `@@` anchor resolution for `apply_patch`.
//!
//! A patch hunk may carry a semantic anchor (`@@ fn deploy`) instead of
//! literal context lines. The anchor names a symbol, and the hunk is applied
//! relative to where that symbol is declared.
//!
//! This replaces the external `ast-grep` binary that VT Code used to
//! download and spawn for the same purpose. The declarations come from the
//! same scanner that backs `code_search`, so the two features agree on what a
//! declaration is and where it sits.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::tools::code_language::CodeLanguage;
use crate::tools::editing::patch::error::PatchError;
use crate::tools::outline_search;

use super::PatchChunk;
use super::matcher::{PatchContextMatcher, redacted_context_matches, seek_segment};

/// A resolved semantic match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SemanticMatch {
    /// Zero-based index of the first replaced line.
    pub start_idx: usize,
    /// The lines being replaced.
    pub old_segment: Vec<String>,
    /// The lines replacing them.
    pub new_segment: Vec<String>,
}

static IDENTIFIER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z_][A-Za-z0-9_]*").expect("identifier pattern"));

/// The declaration names a semantic anchor may refer to.
///
/// An anchor that reads like a declaration header (`@@ fn deploy`,
/// `@@ impl Trait for Type`) is read with the *same* grammar that recognises
/// declarations in source, so the anchor names exactly what the file-side
/// scanner records — no keyword list has to be kept in sync by hand.
///
/// Any other anchor (`@@ deploy`) falls back to the identifiers it contains.
/// A keyword such as `fn` is therefore only a candidate when the file really
/// declares something by that name, which keeps a keyword-only anchor from
/// resolving.
///
/// The names are lowercased because declarations are matched
/// case-insensitively.
pub(crate) fn semantic_anchor_terms(source_path: &Path, anchor: &str) -> Vec<String> {
    let terms = match declaration_language(source_path) {
        Some(language) => outline_search::declaration_names_in_anchor(anchor, language),
        None => Vec::new(),
    };
    let terms = if terms.is_empty() {
        identifier_terms(anchor)
    } else {
        terms
    };
    terms.into_iter().map(|term| term.to_ascii_lowercase()).collect()
}

/// The identifiers an anchor contains, in sorted order.
fn identifier_terms(anchor: &str) -> Vec<String> {
    let mut terms: Vec<String> = IDENTIFIER_RE
        .find_iter(anchor)
        .map(|matched| matched.as_str().to_ascii_lowercase())
        .collect();
    terms.sort();
    terms.dedup();
    terms
}

/// The language a semantic anchor can be resolved in.
///
/// A language without declaration support (markdown, yaml, plain text) has no
/// structural anchors, so the caller falls back to exact text search.
fn declaration_language(source_path: &Path) -> Option<CodeLanguage> {
    let language = CodeLanguage::from_path(source_path)?;
    outline_search::supports_declarations(language).then_some(language)
}

/// Resolves a semantic anchor and locates the hunk relative to it.
///
/// The anchor names the symbol the hunk belongs to. The declaration of that
/// symbol gives the position the hunk is applied from, exactly like the
/// numeric `@@ -12` hunk header gives a starting line. A hunk that matches
/// once resolves; a hunk that matches in several places is ambiguous.
pub(crate) fn resolve_semantic_match(
    source_path: &Path,
    display_path: &str,
    original_lines: &[String],
    chunk: &PatchChunk,
    terms: &[String],
    old_segment: Vec<String>,
    new_segment: Vec<String>,
) -> Result<SemanticMatch, PatchError> {
    let anchor = chunk
        .change_context()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default();

    let Some(language) = declaration_language(source_path) else {
        return Err(PatchError::SemanticResolutionFailed {
            path: display_path.to_string(),
            anchor: anchor.to_string(),
            reason: "unsupported language for semantic fallback".to_string(),
        });
    };

    if terms.is_empty() {
        return Err(PatchError::SemanticResolutionFailed {
            path: display_path.to_string(),
            anchor: anchor.to_string(),
            reason: "anchor does not contain a usable symbol name".to_string(),
        });
    }

    // The declarations come from the same scanner that backs `code_search`, so
    // both features agree on what a declaration is and where it sits.
    let source = original_lines.join("\n");
    let candidates = outline_search::declarations_named(&source, language, terms);
    if candidates.is_empty() {
        return Err(PatchError::SemanticResolutionFailed {
            path: display_path.to_string(),
            anchor: anchor.to_string(),
            reason: "no structural candidates matched the semantic anchor".to_string(),
        });
    }

    // The anchor is a *hint*: the hunk is located from the declaration
    // onwards, exactly like a numeric `@@ -12` hunk header.
    let matcher = PatchContextMatcher::new(original_lines);
    let mut resolved: BTreeMap<usize, SemanticMatch> = BTreeMap::new();
    let mut redacted_match = false;
    for candidate in &candidates {
        let start = candidate.line.saturating_sub(1);
        let mut candidate_old = old_segment.clone();
        let mut candidate_new = new_segment.clone();
        match seek_segment(&matcher, &mut candidate_old, &mut candidate_new, start, chunk.is_end_of_file()) {
            Some(start_idx) => {
                resolved.entry(start_idx).or_insert(SemanticMatch {
                    start_idx,
                    old_segment: candidate_old,
                    new_segment: candidate_new,
                });
            }
            None => {
                redacted_match |=
                    redacted_context_matches(original_lines, &candidate_old, start, chunk.is_end_of_file());
            }
        }
    }

    match resolved.len() {
        0 if redacted_match => Err(PatchError::RedactedContext { path: display_path.to_string() }),
        0 => Err(PatchError::SemanticResolutionFailed {
            path: display_path.to_string(),
            anchor: anchor.to_string(),
            reason:
                "anchor resolved to structural candidates, but removal/context lines were not found safely inside them"
                    .to_string(),
        }),
        1 => resolved
            .into_values()
            .next()
            .ok_or_else(|| PatchError::SemanticResolutionFailed {
                path: display_path.to_string(),
                anchor: anchor.to_string(),
                reason: "internal error: single semantic match vanished during resolution".to_string(),
            }),
        candidate_count => Err(PatchError::SemanticAmbiguous {
            path: display_path.to_string(),
            anchor: anchor.to_string(),
            candidate_count,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{identifier_terms, semantic_anchor_terms};
    use crate::tools::code_language::CodeLanguage;
    use crate::tools::outline_search;
    use std::path::Path;

    fn terms(anchor: &str) -> Vec<String> {
        semantic_anchor_terms(Path::new("lib.rs"), anchor)
    }

    #[test]
    fn anchor_terms_read_a_rust_function_header() {
        assert_eq!(
            terms("pub(crate) fn second() -> usize"),
            vec!["second".to_string()],
            "the return type must not be mistaken for the declared name"
        );
    }

    #[test]
    fn anchor_terms_read_a_rust_impl_header() {
        assert_eq!(
            terms("impl ToolDefinition for ProviderTool"),
            vec!["providertool".to_string()],
            "the impl names the type it implements, like the file-side scanner"
        );
    }

    #[test]
    fn anchor_terms_read_a_python_class_header() {
        assert_eq!(semantic_anchor_terms(Path::new("lib.py"), "class Widget"), vec!["widget".to_string()]);
    }

    #[test]
    fn anchor_terms_fall_back_to_the_identifiers_of_a_bare_anchor() {
        assert_eq!(terms("deploy"), vec!["deploy".to_string()]);
    }

    #[test]
    fn a_keyword_only_anchor_matches_no_declaration() {
        let candidates = outline_search::declarations_named("fn deploy() {}\n", CodeLanguage::Rust, &terms("fn"));
        assert!(candidates.is_empty(), "a keyword names no declaration");
    }

    #[test]
    fn anchor_terms_are_stable_across_repeated_resolution() {
        // The declaration patterns are memoized per language, so a second
        // resolution must not observe a poisoned or empty cache entry.
        assert_eq!(terms("impl ToolDefinition for ProviderTool"), terms("impl ToolDefinition for ProviderTool"));
        assert_eq!(
            semantic_anchor_terms(Path::new("lib.py"), "class Widget"),
            semantic_anchor_terms(Path::new("lib.py"), "class Widget")
        );
    }

    #[test]
    fn identifier_terms_split_on_non_identifier_characters() {
        assert_eq!(identifier_terms("ProviderTool::run"), vec!["providertool".to_string(), "run".to_string()]);
    }
}
