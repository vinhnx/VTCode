use std::collections::HashMap;

use crate::tools::ast_grep_language::AstGrepLanguage;
use crate::tools::code_search::{CodeSearchResult, CodeSearchResultType, ResolvedSearchScope};
use crate::tools::grep_file::LiteralSearchCandidate;
use crate::tools::outline_search::DeclarationFileRecord;
use crate::tools::tree_sitter_runtime::{
    SourceByteRange, exact_declaration_name_range, is_exact_usage_identifier, parse_source, usage_node_kind_allowlist,
};
use crate::types::CompactStr;
use std::path::PathBuf;

use super::ranking::{DeclarationInventory, RankedCandidate};

pub fn process_declaration_file(
    scope: &ResolvedSearchScope,
    languages: &[AstGrepLanguage],
    query: &str,
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
    source_cache.insert(canonical.clone(), source.clone());
    let language = AstGrepLanguage::from_path(&canonical).unwrap_or(file.language);
    let tree = parse_source(language, &source).ok();
    let mut inventory = DeclarationInventory {
        complete: file.complete && tree.is_some() && usage_node_kind_allowlist(language).is_some(),
        exact_name_ranges: Vec::new(),
    };
    for declaration in &file.declarations {
        let full_range = SourceByteRange {
            start: declaration.range.byte_start,
            end: declaration.range.byte_end,
        };
        let exact_range = tree.as_ref().and_then(|tree| {
            exact_declaration_name_range(tree, &source, language, full_range, &declaration.name, query)
        });
        if let Some(range) = exact_range {
            inventory.exact_name_ranges.push(range);
        } else {
            inventory.complete = false;
        }
        if definition_enabled && let Some(position_range) = exact_range {
            let (line, column) = super::byte_position(&source, position_range.start);
            candidates.push(RankedCandidate {
                result: CodeSearchResult {
                    result_type: CodeSearchResultType::Definition,
                    path: relative.clone(),
                    line: Some(line),
                    column: Some(column),
                    name: Some(CompactStr::from(declaration.name.as_str())),
                    snippet: Some(super::normalised_snippet(super::source_line(&source, line))),
                },
                backend_ordinal: 0,
            });
        }
    }
    inventories.insert(canonical, inventory);
}

pub fn classify_literal_candidates(
    scope: &ResolvedSearchScope,
    languages: &[AstGrepLanguage],
    literals: Vec<LiteralSearchCandidate>,
    usage_enabled: bool,
    text_enabled: bool,
    outline_stream_complete: bool,
    source_cache: &mut HashMap<PathBuf, String>,
    inventories: &HashMap<PathBuf, DeclarationInventory>,
    candidates: &mut Vec<RankedCandidate>,
) {
    for literal in literals {
        let Some((canonical, relative)) =
            super::scope::accepted_candidate_path(scope, &scope.workspace_root, &literal.path, languages)
        else {
            continue;
        };
        let language = AstGrepLanguage::from_path(&canonical);
        let range = SourceByteRange { start: literal.byte_start, end: literal.byte_end };
        let inventory = inventories.get(&canonical);
        let is_definition = inventory.is_some_and(|inventory| inventory.exact_name_ranges.contains(&range));
        if is_definition {
            continue;
        }
        let can_classify_usage = usage_enabled
            && language.is_some_and(|language| usage_node_kind_allowlist(language).is_some())
            && inventory.map_or(outline_stream_complete, |inventory| inventory.complete);
        let is_usage = if can_classify_usage {
            let source = source_cache
                .entry(canonical.clone())
                .or_insert_with(|| std::fs::read_to_string(&canonical).unwrap_or_default());
            language
                .and_then(|language| parse_source(language, source).ok().map(|tree| (language, tree)))
                .is_some_and(|(language, tree)| is_exact_usage_identifier(&tree, language, range))
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
