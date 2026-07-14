//! Typed public boundary for the focused `code_search` tool.

use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::tools::ast_grep_language::AstGrepLanguage;
use crate::types::CompactStr;

const DEFAULT_MAX_RESULTS: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodeSearchRequest {
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
pub(crate) enum CodeSearchResultType {
    Definition,
    Usage,
    Text,
    Path,
}

impl CodeSearchResultType {
    const ALL: [Self; 4] = [Self::Definition, Self::Usage, Self::Text, Self::Path];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CodeSearchFilters {
    pub path: CompactStr,
    pub file_types: Vec<CompactStr>,
    pub result_types: Vec<CodeSearchResultType>,
    pub max_results: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CodeSearchResult {
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
pub(crate) struct CodeSearchResponse {
    pub query: CompactStr,
    pub filters: CodeSearchFilters,
    pub results: Vec<CodeSearchResult>,
    pub returned: usize,
    pub truncated: bool,
    pub hints: Vec<CompactStr>,
}

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
            filters: CodeSearchFilters {
                path,
                file_types,
                result_types,
                max_results,
            },
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
        let language = AstGrepLanguage::from_user_value(without_dot)
            .or_else(|| AstGrepLanguage::from_extension(without_dot))
            .ok_or_else(|| anyhow!("unknown code_search file type '{trimmed}'"))?;
        let canonical = CompactStr::from(language.as_str());
        if !normalised.contains(&canonical) {
            normalised.push(canonical);
        }
    }
    Ok(normalised)
}

fn normalise_result_types(
    result_types: Option<Vec<CodeSearchResultType>>,
) -> Result<Vec<CodeSearchResultType>> {
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

/// Execute the typed boundary. Slice 2 composes the bounded search backends
/// behind this normalised request.
pub(crate) async fn execute(request: CodeSearchRequest) -> Result<CodeSearchResponse> {
    let request = request.normalise()?;
    Ok(CodeSearchResponse {
        query: request.query,
        filters: request.filters,
        results: Vec::new(),
        returned: 0,
        truncated: false,
        hints: Vec::new(),
    })
}

pub(crate) fn validate_args(args: &serde_json::Value) -> Result<()> {
    let request: CodeSearchRequest = serde_json::from_value(args.clone())?;
    request.normalise().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(value: serde_json::Value) -> CodeSearchRequest {
        serde_json::from_value(value).expect("valid request shape")
    }

    #[tokio::test]
    async fn code_search_defaults_normalise_to_locked_contract() {
        let response = execute(request(json!({"query": "  Widget  "})))
            .await
            .expect("request should normalise");

        assert_eq!(response.query, "Widget");
        assert_eq!(response.filters.path, ".");
        assert!(response.filters.file_types.is_empty());
        assert_eq!(response.filters.result_types, CodeSearchResultType::ALL);
        assert_eq!(response.filters.max_results, 20);
        assert!(response.results.is_empty());
        assert_eq!(response.returned, 0);
        assert!(!response.truncated);
        assert!(response.hints.is_empty());
    }

    #[tokio::test]
    async fn code_search_filters_normalise_and_deduplicate() {
        let response = execute(request(json!({
            "query": "Widget",
            "path": " src ",
            "file_types": [" rust ", ".rs", ".h", "C"],
            "result_types": ["path", "definition", "path"],
            "max_results": 7
        })))
        .await
        .expect("request should normalise");

        assert_eq!(response.filters.path, "src");
        assert_eq!(response.filters.file_types, ["rust", "c"]);
        assert_eq!(
            response.filters.result_types,
            [CodeSearchResultType::Definition, CodeSearchResultType::Path]
        );
        assert_eq!(response.filters.max_results, 7);
    }

    #[tokio::test]
    async fn code_search_semantic_validation_rejects_invalid_values() {
        for invalid in [
            json!({"query": "   "}),
            json!({"query": "Widget", "path": " "}),
            json!({"query": "Widget", "file_types": []}),
            json!({"query": "Widget", "file_types": ["unknown-language"]}),
            json!({"query": "Widget", "result_types": []}),
            json!({"query": "Widget", "max_results": 0}),
            json!({"query": "Widget", "max_results": 101}),
        ] {
            let error = execute(request(invalid))
                .await
                .expect_err("invalid value must fail");
            assert!(error.to_string().contains("code_search"));
        }
    }

    #[test]
    fn code_search_typed_boundary_rejects_old_and_unknown_fields() {
        for field in [
            "action",
            "workflow",
            "pattern",
            "kind",
            "config_path",
            "filter",
            "lang",
            "selector",
            "strictness",
            "view",
            "items",
            "type",
            "match",
            "pub_members",
            "follow",
            "debug_query",
            "globs",
            "skip_snapshot_tests",
            "context_lines",
            "severities",
            "no_ignore",
            "threads",
            "format",
            "report_style",
            "before_lines",
            "after_lines",
            "builtin_rules",
        ] {
            let mut payload = json!({"query": "Widget"});
            payload
                .as_object_mut()
                .expect("request object")
                .insert(field.to_string(), json!(true));
            let error = serde_json::from_value::<CodeSearchRequest>(payload)
                .expect_err("former field must fail deserialisation");
            assert!(
                error.to_string().contains("unknown field"),
                "{field}: {error}"
            );
        }
    }

    #[test]
    fn code_search_response_omits_optional_result_fields() {
        let value = serde_json::to_value(CodeSearchResponse {
            query: CompactStr::from("Widget"),
            filters: CodeSearchFilters {
                path: CompactStr::from("."),
                file_types: Vec::new(),
                result_types: CodeSearchResultType::ALL.to_vec(),
                max_results: 20,
            },
            results: vec![CodeSearchResult {
                result_type: CodeSearchResultType::Path,
                path: CompactStr::from("src/widget.rs"),
                line: None,
                column: None,
                name: None,
                snippet: None,
            }],
            returned: 1,
            truncated: false,
            hints: Vec::new(),
        })
        .expect("response should serialise");

        let result = value["results"][0].as_object().expect("result object");
        assert_eq!(result.len(), 2);
        assert_eq!(result["result_type"], "path");
        assert_eq!(result["path"], "src/widget.rs");
    }
}
