//! Typed public boundary for the focused `code_search` tool.

pub mod backends;
pub mod identity;
pub mod ranking;
pub mod scope;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;

use crate::tools::code_language::CodeLanguage;
use crate::tools::file_search_bridge::BoundedPathSearch;
use crate::tools::grep_backend::{LiteralSearchOutcome, search_literal_bounded};
use crate::tools::outline_search::{DeclarationSearchOutcome, search_declarations_bounded};
use crate::types::CompactStr;
use vtcode_commons::formatting::{collapse_whitespace, truncate_byte_budget};

const DEFAULT_MAX_RESULTS: usize = 20;
const SNIPPET_BYTE_CAP: usize = 240;

fn backend_candidate_cap(max_results: usize) -> usize {
    max_results.saturating_mul(4).min(200)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeSearchRequest {
    pub query: CompactStr,
    #[serde(default)]
    pub path: Option<CompactStr>,
    #[serde(default)]
    pub file_types: Option<Vec<CompactStr>>,
    #[serde(default)]
    pub result_types: Option<Vec<CodeSearchResultType>>,
    #[serde(default)]
    pub max_results: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeSearchResultType {
    Definition,
    Usage,
    Text,
    Path,
}

impl CodeSearchResultType {
    const ALL: [Self; 4] = [Self::Definition, Self::Usage, Self::Text, Self::Path];

    const fn precedence(self) -> u8 {
        match self {
            Self::Definition => 0,
            Self::Usage => 1,
            Self::Text => 2,
            Self::Path => 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeSearchFilters {
    pub path: CompactStr,
    pub file_types: Vec<CompactStr>,
    pub result_types: Vec<CodeSearchResultType>,
    pub max_results: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeSearchResult {
    pub result_type: CodeSearchResultType,
    pub path: CompactStr,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<CompactStr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<CompactStr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeSearchResponse {
    pub query: CompactStr,
    pub filters: CodeSearchFilters,
    pub results: Vec<CodeSearchResult>,
    pub returned: usize,
    pub truncated: bool,
    pub hints: Vec<CompactStr>,
}

#[derive(Debug)]
struct NormalisedCodeSearchRequest {
    query: CompactStr,
    filters: CodeSearchFilters,
}

impl CodeSearchRequest {
    fn normalise(self) -> Result<NormalisedCodeSearchRequest> {
        let query = self.query.trim();
        if query.is_empty() {
            bail!("code_search query must contain at least one non-whitespace character");
        }

        let path = match self.path {
            Some(path) => {
                let trimmed = path.trim();
                if trimmed.is_empty() {
                    bail!("code_search path must contain at least one non-whitespace character");
                }
                CompactStr::from(trimmed)
            }
            None => CompactStr::from("."),
        };

        let file_types = normalise_file_types(self.file_types)?;
        let result_types = normalise_result_types(self.result_types)?;
        let max_results = self.max_results.unwrap_or(DEFAULT_MAX_RESULTS);
        if !(1..=100).contains(&max_results) {
            bail!("code_search max_results must be between 1 and 100 inclusive");
        }

        Ok(NormalisedCodeSearchRequest {
            query: CompactStr::from(query),
            filters: CodeSearchFilters { path, file_types, result_types, max_results },
        })
    }
}

fn normalise_file_types(file_types: Option<Vec<CompactStr>>) -> Result<Vec<CompactStr>> {
    let Some(file_types) = file_types else {
        return Ok(Vec::new());
    };
    if file_types.is_empty() {
        bail!("code_search file_types must not be empty when supplied");
    }

    let mut normalised = Vec::with_capacity(file_types.len());
    for value in file_types {
        let trimmed = value.trim();
        let without_dot = trimmed.strip_prefix('.').unwrap_or(trimmed);
        if without_dot.is_empty() {
            bail!("code_search file_types entries must not be empty");
        }
        let language = CodeLanguage::from_user_value(without_dot)
            .or_else(|| CodeLanguage::from_extension(without_dot))
            .ok_or_else(|| anyhow!("unknown code_search file type '{trimmed}'"))?;
        let canonical = CompactStr::from(language.as_str());
        if !normalised.contains(&canonical) {
            normalised.push(canonical);
        }
    }
    Ok(normalised)
}

fn normalise_result_types(result_types: Option<Vec<CodeSearchResultType>>) -> Result<Vec<CodeSearchResultType>> {
    let Some(result_types) = result_types else {
        return Ok(CodeSearchResultType::ALL.to_vec());
    };
    if result_types.is_empty() {
        bail!("code_search result_types must not be empty when supplied");
    }

    Ok(CodeSearchResultType::ALL
        .into_iter()
        .filter(|result_type| result_types.contains(result_type))
        .collect())
}

pub async fn execute(workspace_root: &Path, request: CodeSearchRequest) -> Result<CodeSearchResponse> {
    let mut request = request.normalise()?;
    let scope = spawn_blocking({
        let workspace_root = workspace_root.to_path_buf();
        let path = request.filters.path.clone();
        move || scope::resolve_scope(&workspace_root, &path)
    })
    .await
    .context("Failed to resolve search scope")??;
    request.filters.path = scope::response_path(&scope);
    let languages = scope::requested_languages(&request.filters);
    let candidate_cap = backend_candidate_cap(request.filters.max_results);
    let definition_enabled = request.filters.result_types.contains(&CodeSearchResultType::Definition);
    let usage_enabled = request.filters.result_types.contains(&CodeSearchResultType::Usage);
    let text_enabled = request.filters.result_types.contains(&CodeSearchResultType::Text);
    let path_enabled = request.filters.result_types.contains(&CodeSearchResultType::Path);

    let usage_support = scope::usage_scope_support(&scope, &languages);
    let usage_backend_needed = usage_enabled && usage_support.has_supported_files;
    let literal_query = request.query.clone();
    let literal_path = scope.requested_path.clone();
    let literal_languages = languages.clone();
    let literal_future = async move {
        if text_enabled || usage_backend_needed {
            search_literal_bounded(&literal_query, &literal_path, &literal_languages, candidate_cap)
                .await
                .map(Some)
        } else {
            Ok(None)
        }
    };

    let declaration_root = scope.workspace_root.clone();
    let declaration_path = scope.requested_path.clone();
    let declaration_query = request.query.clone();
    let declaration_languages = languages.clone();
    let declaration_future = async move {
        if definition_enabled || usage_backend_needed {
            search_declarations_bounded(
                &declaration_root,
                &declaration_path,
                &declaration_query,
                &declaration_languages,
                candidate_cap,
            )
            .await
            .map(Some)
        } else {
            Ok(None)
        }
    };

    let path_requested_is_file = scope.requested_is_file;
    let path_requested_path = scope.requested_path.clone();
    let path_workspace_root = scope.workspace_root.clone();
    let path_query = request.query.to_string();
    let path_future = async move {
        if !path_enabled {
            return Ok(None);
        }
        let query = path_query;
        if path_requested_is_file {
            let relative = path_requested_path
                .strip_prefix(&path_workspace_root)
                .ok()
                .map(|relative| {
                    if relative.as_os_str().is_empty() {
                        CompactStr::from(".")
                    } else {
                        CompactStr::from(relative.to_string_lossy().replace('\\', "/"))
                    }
                })
                .unwrap_or_else(|| CompactStr::from("."));
            let matches = relative.to_lowercase().contains(&query.to_lowercase());
            Ok(Some(BoundedPathSearch {
                paths: matches.then(|| path_requested_path.clone()).into_iter().collect(),
                truncated: false,
            }))
        } else {
            let search_root = path_requested_path.clone();
            spawn_blocking(move || {
                crate::tools::file_search_bridge::search_paths_bounded_no_follow(&query, search_root, candidate_cap)
            })
            .await
            .context("path search task failed")?
            .map(Some)
        }
    };

    let (literal_outcome, declaration_outcome, path_outcome) =
        tokio::join!(literal_future, declaration_future, path_future);
    let result_types = request.filters.result_types.clone();
    let max_results = request.filters.max_results;
    let aggregation = spawn_blocking(move || {
        aggregate_search_results(SearchAggregationInputs {
            scope,
            languages,
            result_types,
            max_results,
            definition_enabled,
            text_enabled,
            usage_enabled,
            usage_backend_needed,
            has_unsupported_files: usage_support.has_unsupported_files,
            requested_unsupported_file_types: usage_support.requested_unsupported_file_types,
            literal_outcome,
            declaration_outcome,
            path_outcome,
        })
    })
    .await
    .context("code search aggregation task failed")??;
    let unavailable = aggregation.unavailable;
    let results = aggregation.results;
    let truncated = aggregation.truncated;

    let mut hints = Vec::new();
    if truncated {
        hints.push(CompactStr::from("Narrow path, file_types, or result_types to refine truncated results."));
    }
    let usage_limited =
        usage_enabled && (usage_support.has_unsupported_files || usage_support.requested_unsupported_file_types);
    if usage_limited {
        hints.push(CompactStr::from("Usage results are unavailable for some requested file types."));
    }
    if results.iter().any(|result| result.result_type == CodeSearchResultType::Usage) {
        hints.push(CompactStr::from(
            "Usage results are syntactic same-spelling identifiers and may refer to different symbols.",
        ));
    }
    if let Some(hint) = ranking::unavailable_hint(&unavailable) {
        hints.push(hint);
    }
    hints.dedup();
    let returned = results.len();
    Ok(CodeSearchResponse {
        query: request.query,
        filters: request.filters,
        results,
        returned,
        truncated,
        hints,
    })
}

#[derive(Debug)]
struct AggregatedCodeSearch {
    results: Vec<CodeSearchResult>,
    unavailable: Vec<CodeSearchResultType>,
    truncated: bool,
}

struct SearchAggregationInputs {
    scope: ResolvedSearchScope,
    languages: Vec<CodeLanguage>,
    result_types: Vec<CodeSearchResultType>,
    max_results: usize,
    definition_enabled: bool,
    text_enabled: bool,
    usage_enabled: bool,
    usage_backend_needed: bool,
    has_unsupported_files: bool,
    requested_unsupported_file_types: bool,
    literal_outcome: Result<Option<LiteralSearchOutcome>>,
    declaration_outcome: Result<Option<DeclarationSearchOutcome>>,
    path_outcome: Result<Option<BoundedPathSearch>>,
}

fn aggregate_search_results(inputs: SearchAggregationInputs) -> Result<AggregatedCodeSearch> {
    let SearchAggregationInputs {
        scope,
        languages,
        result_types,
        max_results,
        definition_enabled,
        text_enabled,
        usage_enabled,
        usage_backend_needed,
        has_unsupported_files,
        requested_unsupported_file_types,
        literal_outcome,
        declaration_outcome,
        path_outcome,
    } = inputs;
    let mut unavailable = Vec::new();
    let mut backend_errors = Vec::new();
    let mut candidates = Vec::new();
    let mut truncated = false;
    let mut inventories: HashMap<PathBuf, DeclarationInventory> = HashMap::new();
    let mut caches = backends::SourceTreeCache::default();
    let mut outline_stream_complete = false;

    if usage_enabled && !usage_backend_needed && !has_unsupported_files && !requested_unsupported_file_types {
        unavailable.push(CodeSearchResultType::Usage);
    }

    match declaration_outcome {
        Ok(Some(outcome)) => {
            truncated |= outcome.truncated;
            outline_stream_complete = outcome.stream_complete;
            for file in &outcome.files {
                // Filter first: an empty-declaration file yields no definition
                // candidates, and its inventory would have no exact name
                // ranges, so the request-path `is_definition` dedup is
                // unchanged. When `usage_backend_needed` is false, inventories
                // are never consulted for completeness either, so skip the
                // source read + parse entirely.
                if !usage_backend_needed && file.declarations.is_empty() {
                    continue;
                }
                backends::process_declaration_file(
                    &scope,
                    &languages,
                    file,
                    definition_enabled,
                    &mut caches.sources,
                    &mut inventories,
                    &mut candidates,
                );
            }
        }
        Err(err) => {
            backend_errors.push(format!("declaration: {}", format!("{err:#}").chars().take(240).collect::<String>()));
            if definition_enabled {
                unavailable.push(CodeSearchResultType::Definition);
            }
            if usage_backend_needed {
                unavailable.push(CodeSearchResultType::Usage);
            }
            tracing::debug!(target: "vtcode.code_search", error = format!("{err:#}"), "declaration backend failed");
        }
        Ok(None) => {}
    }

    if usage_backend_needed
        && !unavailable.contains(&CodeSearchResultType::Usage)
        && !scope::has_complete_supported_inventory(&scope, &languages, outline_stream_complete, &inventories)
    {
        unavailable.push(CodeSearchResultType::Usage);
    }

    match literal_outcome {
        Ok(Some(outcome)) => {
            truncated |= outcome.truncated;
            backends::classify_literal_candidates(
                &scope,
                &languages,
                outcome.candidates,
                usage_backend_needed && !unavailable.contains(&CodeSearchResultType::Usage),
                text_enabled,
                outline_stream_complete,
                &mut caches,
                &inventories,
                &mut candidates,
            );
        }
        Err(err) => {
            backend_errors.push(format!("literal: {}", format!("{err:#}").chars().take(240).collect::<String>()));
            if text_enabled {
                unavailable.push(CodeSearchResultType::Text);
            }
            if usage_backend_needed && !unavailable.contains(&CodeSearchResultType::Usage) {
                unavailable.push(CodeSearchResultType::Usage);
            }
            tracing::debug!(target: "vtcode.code_search", error = format!("{err:#}"), "literal backend failed");
        }
        Ok(None) => {}
    }

    match path_outcome {
        Ok(Some(outcome)) => {
            truncated |= outcome.truncated;
            let base = if scope.requested_is_file {
                scope.requested_path.parent().unwrap_or(&scope.workspace_root)
            } else {
                &scope.requested_path
            };
            ranking::append_path_candidates(&scope, base, outcome.paths, &languages, &mut candidates);
        }
        Err(err) => {
            backend_errors.push(format!("path: {}", format!("{err:#}").chars().take(240).collect::<String>()));
            unavailable.push(CodeSearchResultType::Path);
            tracing::debug!(target: "vtcode.code_search", error = format!("{err:#}"), "path backend failed");
        }
        Ok(None) => {}
    }

    unavailable.sort_by_key(|kind| kind.precedence());
    unavailable.dedup();
    let successful_count = result_types.len().saturating_sub(unavailable.len());
    if successful_count == 0 {
        let requested = result_types
            .iter()
            .map(|kind| match kind {
                CodeSearchResultType::Definition => "definition",
                CodeSearchResultType::Usage => "usage",
                CodeSearchResultType::Text => "text",
                CodeSearchResultType::Path => "path",
            })
            .collect::<Vec<_>>()
            .join(", ");
        bail!(
            "all requested code_search result categories are unavailable (requested: {requested}; backend context: {}). Do not retry the identical query/path; drop file_types, broaden path, or use exec_command rg for literal text",
            backend_errors.join("; ")
        );
    }

    let mut results = ranking::deduplicate_and_order(candidates);
    if results.len() > max_results {
        results.truncate(max_results);
        truncated = true;
    }

    Ok(AggregatedCodeSearch { results, unavailable, truncated })
}

fn normalised_snippet(text: &str) -> CompactStr {
    let compact = collapse_whitespace(text);
    CompactStr::from(truncate_byte_budget(&compact, SNIPPET_BYTE_CAP, ""))
}

fn source_line(source: &str, line: usize) -> &str {
    source.lines().nth(line.saturating_sub(1)).unwrap_or("")
}

pub fn validate_args(args: &serde_json::Value) -> Result<()> {
    let request: CodeSearchRequest = serde_json::from_value(args.clone())?;
    request.normalise().map(|_| ())
}

pub use identity::normalised_identity;
pub use identity::normalised_loop_identity;
pub use identity::normalised_path;
pub use identity::scope_contains_mutated_path;
pub use ranking::DeclarationInventory;
pub use scope::ResolvedSearchScope;

#[cfg(test)]
mod tests;
