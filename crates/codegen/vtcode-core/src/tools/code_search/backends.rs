use std::collections::HashMap;

use crate::tools::code_language::CodeLanguage;
use crate::tools::code_search::{CodeSearchResult, CodeSearchResultType, ResolvedSearchScope};
use crate::tools::grep_file::LiteralSearchCandidate;
use crate::tools::outline_search::DeclarationFileRecord;
use crate::types::CompactStr;
use std::path::PathBuf;

use super::ranking::{DeclarationInventory, RankedCandidate};

/// A byte range within a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceByteRange {
    /// Byte offset of the range start.
    pub start: usize,
    /// Byte offset of the range end.
    pub end: usize,
}

/// Turns the declarations found in one file into `definition` results.
///
/// A declaration is only reported when its *name* matches the query under
/// smart-case rules, so a declaration of a different symbol never appears as
/// a definition of the requested one.
pub fn process_declaration_file(
    scope: &ResolvedSearchScope,
    languages: &[CodeLanguage],
    file: &DeclarationFileRecord,
    definition_enabled: bool,
    source_cache: &mut HashMap<PathBuf, String>,
    inventories: &mut HashMap<PathBuf, DeclarationInventory>,
    candidates: &mut Vec<RankedCandidate>,
) {
    let Some((canonical, relative)) =
        super::scope::accepted_candidate_path(scope, &scope.workspace_root, &file.path, languages)
    else {
        return;
    };
    let Ok(source) = std::fs::read_to_string(&canonical) else {
        inventories.insert(canonical, DeclarationInventory::default());
        return;
    };
    let mut inventory = DeclarationInventory {
        complete: file.complete,
        exact_name_ranges: Vec::new(),
    };

    for declaration in &file.declarations {
        // The declaration range covers the declared name itself, so it is the
        // exact name range the caller needs.
        let name_range = SourceByteRange {
            start: declaration.range.byte_start,
            end: declaration.range.byte_end,
        };
        inventory.exact_name_ranges.push(name_range);
        if definition_enabled {
            // The scanner is the single source of truth for positions, so the
            // declaration's line and byte column are used as they are.
            candidates.push(RankedCandidate {
                result: CodeSearchResult {
                    result_type: CodeSearchResultType::Definition,
                    path: relative.clone(),
                    line: Some(declaration.line),
                    column: Some(declaration.column),
                    name: Some(CompactStr::from(declaration.name.as_str())),
                    snippet: Some(super::normalised_snippet(super::source_line(&source, declaration.line))),
                },
                backend_ordinal: 0,
            });
        }
    }

    // Move the source into the shared cache instead of cloning it upfront.
    // The literal-classification phase reads from this cache; moving avoids a
    // full file-content copy per indexed declaration file.
    inventories.insert(canonical.clone(), inventory);
    source_cache.insert(canonical, source);
}

/// Per-file caches shared across a code-search aggregation pass.
///
/// `sources` memoizes file text, so a file with many literal hits is read
/// once rather than once per hit.
#[derive(Default)]
pub struct SourceTreeCache {
    /// Canonical-path-keyed file text, shared with `process_declaration_file`.
    pub sources: HashMap<PathBuf, String>,
    /// Canonical-path-keyed lexical mask, marking which bytes are code.
    pub masks: HashMap<PathBuf, Vec<bool>>,
}

/// Classifies literal search hits into `usage` and `text` results.
///
/// A hit is a *usage* when the matched text is exactly the queried symbol and
/// it is not the declaration of that symbol. Everything else — a mention in a
/// comment, inside a string, or a longer identifier that merely contains the
/// query — stays a `text` result, so the model can still see it.
pub fn classify_literal_candidates(
    scope: &ResolvedSearchScope,
    languages: &[CodeLanguage],
    literals: Vec<LiteralSearchCandidate>,
    usage_enabled: bool,
    text_enabled: bool,
    outline_stream_complete: bool,
    caches: &mut SourceTreeCache,
    inventories: &HashMap<PathBuf, DeclarationInventory>,
    candidates: &mut Vec<RankedCandidate>,
) {
    for literal in literals {
        let Some((canonical, relative)) =
            super::scope::accepted_candidate_path(scope, &scope.workspace_root, &literal.path, languages)
        else {
            continue;
        };
        let range = SourceByteRange { start: literal.byte_start, end: literal.byte_end };
        let language = CodeLanguage::from_path(&canonical);
        let inventory = inventories.get(&canonical);
        let is_definition = inventory.is_some_and(|inventory| inventory.exact_name_ranges.contains(&range));
        if is_definition {
            continue;
        }
        let can_classify_usage = usage_enabled
            && language.is_some_and(|language| language.supports_usage_classification())
            && inventory.map_or(outline_stream_complete, |inventory| inventory.complete);
        let is_usage = if can_classify_usage {
            // A usage is an exact identifier occurrence in code. The
            // declaration name ranges are already excluded above, so what
            // remains is a hit that matches the query exactly rather than as
            // part of a longer word, and that is not inside a string literal
            // or a comment.
            let source = caches
                .sources
                .entry(canonical.clone())
                .or_insert_with(|| std::fs::read_to_string(&canonical).unwrap_or_default())
                .clone();
            is_exact_identifier_hit(&literal, &source, language, caches)
        } else {
            false
        };
        if is_usage {
            candidates.push(RankedCandidate {
                result: CodeSearchResult {
                    result_type: CodeSearchResultType::Usage,
                    path: relative,
                    line: Some(literal.line),
                    column: Some(literal.column),
                    name: Some(CompactStr::from(literal.matched_text.as_str())),
                    snippet: Some(super::normalised_snippet(&literal.snippet)),
                },
                backend_ordinal: 1,
            });
        } else if text_enabled {
            candidates.push(RankedCandidate {
                result: CodeSearchResult {
                    result_type: CodeSearchResultType::Text,
                    path: relative,
                    line: Some(literal.line),
                    column: Some(literal.column),
                    name: None,
                    snippet: Some(super::normalised_snippet(&literal.snippet)),
                },
                backend_ordinal: 2,
            });
        }
    }
}

/// Whether a literal hit is an exact identifier occurrence in code.
///
/// The hit must cover a whole word — `Widget` inside `WidgetExtra` is a text
/// match, not a usage of `Widget` — and it must sit in code rather than in a
/// string literal or a comment.
fn is_exact_identifier_hit(
    literal: &LiteralSearchCandidate,
    source: &str,
    language: Option<CodeLanguage>,
    caches: &mut SourceTreeCache,
) -> bool {
    if !literal.word_bounded || literal.matched_text.is_empty() {
        return false;
    }
    let Some(language) = language else {
        return false;
    };
    let mask = caches
        .masks
        .entry(literal.path.clone())
        .or_insert_with(|| vtcode_commons::lexical::code_mask(source, &language.lexical_syntax()));
    // A hit spans `byte_start..byte_end`; it is in code when every byte it
    // covers is code.
    mask.get(literal.byte_start..literal.byte_end)
        .is_some_and(|bytes| bytes.iter().all(|is_code| *is_code))
}
