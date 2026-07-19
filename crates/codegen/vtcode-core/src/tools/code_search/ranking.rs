use std::collections::HashMap;

use crate::tools::ast_grep_language::AstGrepLanguage;
use crate::tools::code_search::{CodeSearchResult, CodeSearchResultType, ResolvedSearchScope};
use crate::types::CompactStr;
use std::path::{Path, PathBuf};

pub struct RankedCandidate {
    pub result: CodeSearchResult,
    pub backend_ordinal: u8,
}

#[derive(Debug, Default)]
pub struct DeclarationInventory {
    pub complete: bool,
    pub exact_name_ranges: Vec<crate::tools::tree_sitter_runtime::SourceByteRange>,
}

pub fn deduplicate_and_order(mut candidates: Vec<RankedCandidate>) -> Vec<CodeSearchResult> {
    let mut source_locations: HashMap<(CompactStr, usize, usize), RankedCandidate> = HashMap::new();
    let mut path_only = Vec::new();
    for candidate in candidates.drain(..) {
        let (Some(line), Some(column)) = (candidate.result.line, candidate.result.column) else {
            path_only.push(candidate);
            continue;
        };
        let key = (candidate.result.path.clone(), line, column);
        match source_locations.get(&key) {
            Some(existing) if existing.result.result_type.precedence() <= candidate.result.result_type.precedence() => {
            }
            _ => {
                source_locations.insert(key, candidate);
            }
        }
    }
    let mut ordered = source_locations.into_values().chain(path_only).collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        left.result
            .result_type
            .precedence()
            .cmp(&right.result.result_type.precedence())
            .then_with(|| left.result.path.cmp(&right.result.path))
            .then_with(|| left.result.line.unwrap_or(0).cmp(&right.result.line.unwrap_or(0)))
            .then_with(|| left.result.column.unwrap_or(0).cmp(&right.result.column.unwrap_or(0)))
            .then_with(|| left.backend_ordinal.cmp(&right.backend_ordinal))
    });
    ordered.into_iter().map(|candidate| candidate.result).collect()
}

pub fn unavailable_hint(unavailable: &[CodeSearchResultType]) -> Option<CompactStr> {
    if unavailable.is_empty() {
        return None;
    }
    let names = unavailable
        .iter()
        .map(|kind| match kind {
            CodeSearchResultType::Definition => "definition",
            CodeSearchResultType::Usage => "usage",
            CodeSearchResultType::Text => "text",
            CodeSearchResultType::Path => "path",
        })
        .collect::<Vec<_>>()
        .join(", ");
    Some(CompactStr::from(format!("Some requested result categories were unavailable: {names}.")))
}

pub fn append_path_candidates(
    scope: &ResolvedSearchScope,
    base: &Path,
    paths: impl IntoIterator<Item = PathBuf>,
    languages: &[AstGrepLanguage],
    candidates: &mut Vec<RankedCandidate>,
) {
    use std::collections::HashSet;
    let mut seen_canonical_paths = HashSet::new();
    for path in paths {
        let Some((canonical, relative)) = super::scope::accepted_candidate_path(scope, base, &path, languages) else {
            continue;
        };
        if !seen_canonical_paths.insert(canonical) {
            continue;
        }
        candidates.push(RankedCandidate {
            result: CodeSearchResult {
                result_type: CodeSearchResultType::Path,
                path: relative,
                line: None,
                column: None,
                name: None,
                snippet: None,
            },
            backend_ordinal: 3,
        });
    }
}
