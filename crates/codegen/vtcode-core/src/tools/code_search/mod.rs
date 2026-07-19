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

use crate::tools::ast_grep_language::AstGrepLanguage;
use crate::tools::file_search_bridge::BoundedPathSearch;
use crate::tools::grep_file::search_literal_bounded;
use crate::tools::outline_search::search_declarations_bounded;
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
    let literal_outcome = if text_enabled || usage_backend_needed {
        search_literal_bounded(&request.query, &scope.requested_path, &languages, candidate_cap)
            .await
            .map(Some)
    } else {
        Ok(None)
    };
    let declaration_outcome = if definition_enabled || usage_backend_needed {
        search_declarations_bounded(
            &scope.workspace_root,
            &scope.requested_path,
            &request.query,
            &languages,
            candidate_cap,
        )
        .await
        .map(Some)
    } else {
        Ok(None)
    };
    let path_outcome = if path_enabled {
        let query = request.query.to_string();
        if scope.requested_is_file {
            let relative =
                scope::workspace_relative(&scope, &scope.requested_path).unwrap_or_else(|| CompactStr::from("."));
            let matches = relative.to_lowercase().contains(&query.to_lowercase());
            Ok(Some(BoundedPathSearch {
                paths: matches.then(|| scope.requested_path.clone()).into_iter().collect(),
                truncated: false,
            }))
        } else {
            let search_root = scope.requested_path.clone();
            spawn_blocking(move || {
                crate::tools::file_search_bridge::search_paths_bounded_no_follow(&query, search_root, candidate_cap)
            })
            .await
            .context("path search task failed")?
            .map(Some)
        }
    } else {
        Ok(None)
    };

    let mut unavailable = Vec::new();
    let mut candidates = Vec::new();
    let mut truncated = false;
    let mut inventories: HashMap<PathBuf, DeclarationInventory> = HashMap::new();
    let mut source_cache: HashMap<PathBuf, String> = HashMap::new();
    let mut outline_stream_complete = false;

    if usage_enabled
        && !usage_backend_needed
        && !usage_support.has_unsupported_files
        && !usage_support.requested_unsupported_file_types
    {
        unavailable.push(CodeSearchResultType::Usage);
    }

    match &declaration_outcome {
        Ok(Some(outcome)) => {
            truncated |= outcome.truncated;
            outline_stream_complete = outcome.stream_complete;
            for file in &outcome.files {
                backends::process_declaration_file(
                    &scope,
                    &languages,
                    &request.query,
                    file,
                    definition_enabled,
                    &mut source_cache,
                    &mut inventories,
                    &mut candidates,
                );
            }
        }
        Err(_) => {
            if definition_enabled {
                unavailable.push(CodeSearchResultType::Definition);
            }
            if usage_backend_needed {
                unavailable.push(CodeSearchResultType::Usage);
            }
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
                &mut source_cache,
                &inventories,
                &mut candidates,
            );
        }
        Err(_) => {
            if text_enabled {
                unavailable.push(CodeSearchResultType::Text);
            }
            if usage_backend_needed && !unavailable.contains(&CodeSearchResultType::Usage) {
                unavailable.push(CodeSearchResultType::Usage);
            }
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
        Err(_) => unavailable.push(CodeSearchResultType::Path),
        Ok(None) => {}
    }

    unavailable.sort_by_key(|kind| kind.precedence());
    unavailable.dedup();
    let successful_count = request.filters.result_types.len().saturating_sub(unavailable.len());
    if successful_count == 0 {
        bail!("all requested code_search result categories are unavailable");
    }

    let mut results = ranking::deduplicate_and_order(candidates);
    if results.len() > request.filters.max_results {
        results.truncate(request.filters.max_results);
        truncated = true;
    }
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

fn normalised_snippet(text: &str) -> CompactStr {
    let compact = collapse_whitespace(text);
    CompactStr::from(truncate_byte_budget(&compact, SNIPPET_BYTE_CAP, ""))
}

fn byte_position(source: &str, offset: usize) -> (usize, usize) {
    let bounded = offset.min(source.len());
    let before = &source[..bounded];
    let line = before.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = before.rsplit_once('\n').map_or(before.len(), |(_, tail)| tail.len()) + 1;
    (line, column)
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
pub use identity::scope_contains_mutated_path;
pub use ranking::DeclarationInventory;
pub use scope::ResolvedSearchScope;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ast_grep_binary::set_ast_grep_binary_override_for_tests;
    use crate::tools::ast_grep_language::AstGrepLanguage;
    use crate::tools::grep_file::LiteralSearchCandidate;
    use crate::tools::outline_search::{DeclarationFileRecord, DeclarationRange, DeclarationRecord};
    use serde_json::json;
    use serial_test::serial;
    use std::collections::HashSet;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn result_identity_includes_effective_max_results() {
        let one = normalised_identity(&serde_json::json!({
            "query": "Widget",
            "max_results": 1
        }));
        let hundred = normalised_identity(&serde_json::json!({
            "query": "Widget",
            "max_results": 100
        }));
        assert_ne!(one, hundred);
    }

    #[test]
    fn result_identity_normalises_omitted_max_results_to_default() {
        let omitted = normalised_identity(&serde_json::json!({"query": " Widget "}));
        let explicit = normalised_identity(&serde_json::json!({
            "query": "Widget",
            "max_results": DEFAULT_MAX_RESULTS
        }));
        assert_eq!(omitted, explicit);
    }

    #[test]
    fn loop_identity_deliberately_ignores_max_results() {
        let one = normalised_loop_identity(&serde_json::json!({
            "query": "Widget",
            "max_results": 1
        }));
        let hundred = normalised_loop_identity(&serde_json::json!({
            "query": "Widget",
            "max_results": 100
        }));
        assert_eq!(one, hundred);
    }

    fn request(value: serde_json::Value) -> CodeSearchRequest {
        serde_json::from_value(value).expect("valid request shape")
    }

    struct CodeSearchFixture {
        workspace: TempDir,
        fake_outline: TempDir,
        fake_outline_path: PathBuf,
    }

    fn code_search_fixture() -> CodeSearchFixture {
        let workspace = TempDir::new().expect("fixture workspace");
        fs::create_dir_all(workspace.path().join("src")).expect("src directory");
        fs::create_dir_all(workspace.path().join(".git")).expect("git marker");
        let rust_source = concat!(
            "fn Widget() {\n",
            "    Widget();\n",
            "}\n",
            "fn other() {\n",
            "    Widget();\n",
            "    let Widget = 1;\n",
            "    let WidgetExtra = \"Widget\";\n",
            "}\n",
            "// Widget comment\n",
            "// widget lower-case comment\n",
            "const LABEL: &str = \"Widget\";\n",
        );
        fs::write(workspace.path().join("src/widget.rs"), rust_source).expect("Rust fixture");
        fs::write(workspace.path().join("src/WidgetConfig.rs"), "pub const VALUE: &str = \"other\";\n")
            .expect("matching path fixture");
        fs::write(workspace.path().join("src/long.rs"), format!("// Widget {}\n", "こ".repeat(200)))
            .expect("long UTF-8 fixture");
        fs::write(workspace.path().join("src/widget.py"), "class Widget:\n    pass\nprint(Widget)\n")
            .expect("Python fixture");
        let bash_source = "function Widget() { echo Widget; }\n";
        fs::write(workspace.path().join("src/widget.sh"), bash_source).expect("Bash fixture");
        fs::write(workspace.path().join("src/readme.mdx"), "Widget prose\n").expect("unsupported parser fixture");
        fs::write(workspace.path().join("ignored.rs"), "fn Widget() {}\n").expect("ignored fixture");
        fs::write(workspace.path().join(".env"), "Widget=secret\n").expect("sensitive fixture");
        fs::write(workspace.path().join(".gitignore"), "ignored.rs\n").expect("ignore file");

        let declaration_end = rust_source.find("}\n").expect("function end") + 1;
        let outline_record = json!({
            "path": "src/widget.rs",
            "language": "Rust",
            "items": [{
                "name": "Widget",
                "symbolType": "function",
                "astKind": "function_item",
                "isImport": false,
                "range": {
                    "byteOffset": {"start": 0, "end": declaration_end},
                    "start": {"line": 0, "column": 0},
                    "end": {"line": 2, "column": 1}
                }
            }]
        });
        let bash_outline_record = json!({
            "path": "src/widget.sh",
            "language": "Bash",
            "items": [{
                "name": "Widget",
                "symbolType": "function",
                "astKind": "function_definition",
                "isImport": false,
                "range": {
                    "byteOffset": {"start": 0, "end": bash_source.len()},
                    "start": {"line": 0, "column": 0},
                    "end": {"line": 0, "column": bash_source.len()}
                }
            }]
        });
        let fake_outline = TempDir::new().expect("fake outline directory");
        let fake_outline_path = fake_outline.path().join("ast-grep");
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then printf 'ast-grep 0.44.0\\n'; exit 0; fi\nprintf '%s\\n' '{}' '{}'\n",
            outline_record.to_string().replace('\'', "'\\''"),
            bash_outline_record.to_string().replace('\'', "'\\''")
        );
        fs::write(&fake_outline_path, script).expect("fake outline executable");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&fake_outline_path)
                .expect("fake executable metadata")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&fake_outline_path, permissions).expect("fake executable permissions");
        }

        CodeSearchFixture { workspace, fake_outline, fake_outline_path }
    }

    #[test]
    fn code_search_defaults_normalise_to_locked_contract() {
        let request = request(json!({"query": "  Widget  "}))
            .normalise()
            .expect("request should normalise");

        assert_eq!(request.query, "Widget");
        assert_eq!(request.filters.path, ".");
        assert!(request.filters.file_types.is_empty());
        assert_eq!(request.filters.result_types, CodeSearchResultType::ALL);
        assert_eq!(request.filters.max_results, 20);
        assert_eq!(backend_candidate_cap(1), 4);
        assert_eq!(backend_candidate_cap(50), 200);
        assert_eq!(backend_candidate_cap(100), 200);
    }

    #[test]
    fn code_search_filters_normalise_and_deduplicate() {
        let request = request(json!({
            "query": "Widget",
            "path": " src ",
            "file_types": [" rust ", ".rs", ".h", "C"],
            "result_types": ["path", "definition", "path"],
            "max_results": 7
        }))
        .normalise()
        .expect("request should normalise");

        assert_eq!(request.filters.path, "src");
        assert_eq!(request.filters.file_types, ["rust", "c"]);
        assert_eq!(request.filters.result_types, [CodeSearchResultType::Definition, CodeSearchResultType::Path]);
        assert_eq!(request.filters.max_results, 7);
    }

    #[test]
    fn replay_scope_overlap_is_symmetric_and_component_aware() {
        let workspace = TempDir::new().expect("workspace");
        fs::create_dir_all(workspace.path().join("src/module")).expect("source fixture");
        let args = json!({"query": "Widget", "path": "src/module"});
        let absolute_child = workspace.path().join("src/module/widget.rs");
        let canonical_child = identity::canonicalize_existing_prefix(workspace.path()).join("src/module/widget.rs");

        for mutation in [
            PathBuf::from("src/module/widget.rs"),
            absolute_child,
            canonical_child,
            PathBuf::from("src"),
        ] {
            assert!(
                scope_contains_mutated_path(&args, &mutation, workspace.path()),
                "expected overlap for {}",
                mutation.display()
            );
        }
        assert!(!scope_contains_mutated_path(&args, Path::new("src/module_two/widget.rs"), workspace.path(),));
        assert!(!scope_contains_mutated_path(&args, Path::new("tests/widget.rs"), workspace.path(),));
    }

    #[cfg(unix)]
    #[test]
    fn replay_scope_overlap_resolves_symlinked_workspace_roots() {
        use std::os::unix::fs::symlink;

        let real_workspace = TempDir::new().expect("real workspace");
        fs::create_dir_all(real_workspace.path().join("src")).expect("source fixture");
        let link_parent = TempDir::new().expect("link parent");
        let linked_workspace = link_parent.path().join("workspace-link");
        symlink(real_workspace.path(), &linked_workspace).expect("workspace symlink");
        let args = json!({"query": "Widget", "path": "src"});

        assert!(scope_contains_mutated_path(&args, &linked_workspace.join("src/widget.rs"), &linked_workspace,));
        assert!(scope_contains_mutated_path(&args, &real_workspace.path().join("src/widget.rs"), &linked_workspace,));
    }

    #[test]
    fn complete_stream_does_not_override_explicitly_incomplete_inventory() {
        let workspace = TempDir::new().expect("workspace");
        let source = workspace.path().join("widget.rs");
        fs::write(&source, "fn Widget() {}\n").expect("source fixture");
        let canonical_root = identity::canonicalize_existing_prefix(workspace.path());
        let canonical_source = identity::canonicalize_existing_prefix(&source);
        let scope = ResolvedSearchScope {
            workspace_root: canonical_root.clone(),
            requested_path: canonical_root,
            requested_is_file: false,
            allowed_files: HashSet::from([canonical_source.clone()]),
        };
        let inventories = HashMap::from([(
            canonical_source,
            DeclarationInventory { complete: false, exact_name_ranges: Vec::new() },
        )]);

        assert!(!scope::has_complete_supported_inventory(&scope, &[AstGrepLanguage::Rust], true, &inventories,));
    }

    #[test]
    fn code_search_semantic_validation_rejects_invalid_values() {
        for invalid in [
            json!({"query": "   "}),
            json!({"query": "Widget", "path": " "}),
            json!({"query": "Widget", "file_types": []}),
            json!({"query": "Widget", "file_types": ["unknown-language"]}),
            json!({"query": "Widget", "result_types": []}),
            json!({"query": "Widget", "max_results": 0}),
            json!({"query": "Widget", "max_results": 101}),
        ] {
            let error = request(invalid).normalise().expect_err("invalid value must fail");
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
            assert!(error.to_string().contains("unknown field"), "{field}: {error}");
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

    #[test]
    fn code_search_snippets_use_exact_utf8_safe_byte_cap() {
        let ascii = normalised_snippet(&format!("Widget {}", "x".repeat(300)));
        assert_eq!(ascii.len(), SNIPPET_BYTE_CAP);

        let unicode = normalised_snippet(&format!("Widget {}", "こ".repeat(200)));
        assert!(unicode.len() <= SNIPPET_BYTE_CAP);
        assert!(unicode.len() > SNIPPET_BYTE_CAP - 4);
        assert!(std::str::from_utf8(unicode.as_bytes()).is_ok());
    }

    #[cfg(unix)]
    #[tokio::test]
    #[serial]
    async fn code_search_fixture_composes_exact_classifications_deterministically() {
        let fixture = code_search_fixture();
        let _keep_fake_outline_alive = &fixture.fake_outline;
        let _override = set_ast_grep_binary_override_for_tests(Some(fixture.fake_outline_path.clone()));
        let payload = json!({
            "query": "Widget",
            "path": "src",
            "file_types": ["rust"],
            "max_results": 100
        });
        let first = execute(fixture.workspace.path(), request(payload.clone()))
            .await
            .expect("composed search");
        let second = execute(fixture.workspace.path(), request(payload))
            .await
            .expect("repeat composed search");
        assert_eq!(
            serde_json::to_value(&first).expect("first response"),
            serde_json::to_value(&second).expect("second response")
        );
        assert_eq!(first.results[0].result_type, CodeSearchResultType::Definition);
        assert!(
            first
                .results
                .iter()
                .any(|result| result.result_type == CodeSearchResultType::Usage && result.line == Some(2))
        );
        assert!(
            first
                .results
                .iter()
                .any(|result| result.result_type == CodeSearchResultType::Usage && result.line == Some(6))
        );
        assert!(first.results.iter().any(|result| {
            result.result_type == CodeSearchResultType::Text
                && result.snippet.as_deref().is_some_and(|line| line.contains("WidgetExtra"))
        }));
        assert!(
            first
                .results
                .iter()
                .any(|result| { result.result_type == CodeSearchResultType::Text && result.line == Some(9) })
        );
        assert!(
            first
                .results
                .iter()
                .any(|result| { result.result_type == CodeSearchResultType::Text && result.line == Some(11) })
        );
        assert!(first.results.iter().any(|result| {
            result.result_type == CodeSearchResultType::Path && result.path == "src/WidgetConfig.rs"
        }));
        assert!(first.hints.iter().any(|hint| hint.contains("syntactic")));

        let lower_case_usage = execute(
            fixture.workspace.path(),
            request(json!({
                "query": "widget",
                "path": "src/widget.rs",
                "file_types": ["rust"],
                "result_types": ["usage"],
                "max_results": 100
            })),
        )
        .await
        .expect("lower-case usage search");
        assert!(!lower_case_usage.results.is_empty());
        assert!(
            lower_case_usage
                .results
                .iter()
                .filter(|result| result.result_type == CodeSearchResultType::Usage)
                .all(|result| result.name.as_deref() == Some("Widget"))
        );

        for result_type in ["definition", "usage", "text", "path"] {
            let response = execute(
                fixture.workspace.path(),
                request(json!({
                    "query": "Widget",
                    "path": "src",
                    "file_types": ["rust"],
                    "result_types": [result_type],
                    "max_results": 100
                })),
            )
            .await
            .expect("result subset");
            assert!(response.results.iter().all(|result| {
                serde_json::to_value(result.result_type).expect("result type").as_str() == Some(result_type)
            }));
        }

        let unfiltered = execute(
            fixture.workspace.path(),
            request(json!({
                "query": "Widget",
                "path": "src",
                "result_types": ["usage", "text"],
                "max_results": 100
            })),
        )
        .await
        .expect("mixed-language usage search");
        assert!(
            unfiltered
                .results
                .iter()
                .any(|result| { result.result_type == CodeSearchResultType::Text && result.path == "src/widget.sh" })
        );
        assert!(unfiltered.hints.iter().any(|hint| hint.contains("file types")));
    }

    #[tokio::test]
    async fn code_search_literal_smart_case_paths_and_policy_are_bounded() {
        let fixture = code_search_fixture();
        let lower = execute(
            fixture.workspace.path(),
            request(json!({
                "query": "widget",
                "path": "src/widget.rs",
                "result_types": ["text"],
                "max_results": 100
            })),
        )
        .await
        .expect("lower-case literal search");
        let mixed = execute(
            fixture.workspace.path(),
            request(json!({
                "query": "Widget",
                "path": "src/widget.rs",
                "result_types": ["text"],
                "max_results": 100
            })),
        )
        .await
        .expect("mixed-case literal search");
        assert!(lower.returned > mixed.returned);

        let punctuation = execute(
            fixture.workspace.path(),
            request(json!({
                "query": "Widget()",
                "path": "src/widget.rs",
                "result_types": ["text"]
            })),
        )
        .await
        .expect("regex punctuation is literal");
        assert!(!punctuation.results.is_empty());

        let truncated = execute(
            fixture.workspace.path(),
            request(json!({
                "query": "Widget",
                "path": "src/widget.rs",
                "result_types": ["text"],
                "max_results": 1
            })),
        )
        .await
        .expect("bounded result");
        assert_eq!(truncated.returned, 1);
        assert!(truncated.truncated);
        assert!(truncated.hints[0].contains("Narrow path"));
        assert!(
            truncated
                .results
                .iter()
                .all(|result| { result.snippet.as_ref().is_none_or(|snippet| snippet.len() <= SNIPPET_BYTE_CAP) })
        );

        let long_snippet = execute(
            fixture.workspace.path(),
            request(json!({
                "query": "Widget",
                "path": "src/long.rs",
                "result_types": ["text"]
            })),
        )
        .await
        .expect("long snippet search");
        let snippet = long_snippet.results[0].snippet.as_ref().expect("source snippet");
        assert!(snippet.len() <= SNIPPET_BYTE_CAP);
        assert!(snippet.len() > 230);
        assert!(std::str::from_utf8(snippet.as_bytes()).is_ok());

        for rejected_path in ["../outside", ".env", "ignored.rs"] {
            assert!(
                execute(
                    fixture.workspace.path(),
                    request(json!({
                        "query": "Widget",
                        "path": rejected_path,
                        "result_types": ["text"]
                    }))
                )
                .await
                .is_err(),
                "{rejected_path} must be rejected"
            );
        }
    }

    #[tokio::test]
    async fn code_search_direct_file_path_search_never_returns_siblings() {
        let fixture = code_search_fixture();
        let response = execute(
            fixture.workspace.path(),
            request(json!({
                "query": "widget",
                "path": "src/widget.rs",
                "result_types": ["path"]
            })),
        )
        .await
        .expect("direct file path search");

        assert_eq!(response.returned, 1);
        assert_eq!(response.results[0].path, "src/widget.rs");
        assert!(response.results.iter().all(|result| result.path == "src/widget.rs"));
    }

    #[tokio::test]
    async fn code_search_bash_usage_is_successfully_unsupported() {
        let fixture = code_search_fixture();
        let response = execute(
            fixture.workspace.path(),
            request(json!({
                "query": "Widget",
                "path": "src/widget.sh",
                "result_types": ["usage"]
            })),
        )
        .await
        .expect("unsupported Bash usage is a successful empty component");
        assert!(response.results.is_empty());
        assert_eq!(response.returned, 0);
        assert!(!response.truncated);
        assert_eq!(response.hints.len(), 1);
        assert!(response.hints[0].contains("file types"));
    }

    #[tokio::test]
    #[serial]
    async fn code_search_bash_definition_remains_available() {
        let fixture = code_search_fixture();
        let _fake_outline_dir = &fixture.fake_outline;
        let _override = set_ast_grep_binary_override_for_tests(Some(fixture.fake_outline_path.clone()));
        let response = execute(
            fixture.workspace.path(),
            request(json!({
                "query": "Widget",
                "path": "src/widget.sh",
                "result_types": ["definition"]
            })),
        )
        .await
        .expect("Bash definition search");

        assert_eq!(response.returned, 1);
        assert_eq!(response.results[0].result_type, CodeSearchResultType::Definition);
        assert_eq!(response.results[0].name.as_deref(), Some("Widget"));
    }

    #[tokio::test]
    async fn code_search_usage_only_requires_an_eligible_supported_inventory() {
        let workspace = TempDir::new().expect("workspace");
        fs::create_dir_all(workspace.path().join("empty")).expect("empty directory");
        fs::write(workspace.path().join("notes.txt"), "Widget\n").expect("unsupported fixture");

        for path in ["empty", "notes.txt"] {
            let outcome = execute(
                workspace.path(),
                request(json!({
                    "query": "Widget",
                    "path": path,
                    "result_types": ["usage"]
                })),
            )
            .await;
            assert!(outcome.is_err(), "{path} has no supported inventory");
        }

        let unsupported = execute(
            workspace.path(),
            request(json!({
                "query": "Widget",
                "path": "empty",
                "file_types": ["bash"],
                "result_types": ["usage"]
            })),
        )
        .await
        .expect("explicit unsupported usage language remains successful");
        assert!(unsupported.results.is_empty());
        assert!(unsupported.hints[0].contains("file types"));
    }

    #[test]
    fn code_search_file_type_aliases_share_canonical_mapping() {
        let request = request(json!({
            "query": "Widget",
            "file_types": [".h", "c", ".jsx", "javascript", ".mdx", "md", ".proto", "protobuf", "Dockerfile", "docker"]
        }))
        .normalise()
        .expect("aliases normalise");
        assert_eq!(request.filters.file_types, ["c", "javascript", "md", "proto", "dockerfile"]);
        for (path, language) in [
            ("include/widget.h", AstGrepLanguage::C),
            ("web/widget.jsx", AstGrepLanguage::JavaScript),
            ("docs/widget.mdx", AstGrepLanguage::Markdown),
            ("api/widget.proto", AstGrepLanguage::Protobuf),
            ("Dockerfile", AstGrepLanguage::Dockerfile),
        ] {
            assert_eq!(AstGrepLanguage::from_path(Path::new(path)), Some(language));
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn code_search_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let fixture = code_search_fixture();
        let outside = TempDir::new().expect("outside directory");
        fs::write(outside.path().join("Widget.rs"), "fn Widget() {}\n").expect("outside source");
        symlink(outside.path().join("Widget.rs"), fixture.workspace.path().join("escape.rs")).expect("escape symlink");
        assert!(
            execute(
                fixture.workspace.path(),
                request(json!({
                    "query": "Widget",
                    "path": "escape.rs",
                    "result_types": ["text"]
                }))
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    #[serial]
    async fn code_search_contains_definition_failure_by_requested_category() {
        let fixture = code_search_fixture();
        let _override = set_ast_grep_binary_override_for_tests(None);
        let definition_only = execute(
            fixture.workspace.path(),
            request(json!({
                "query": "Widget",
                "path": "src/widget.rs",
                "result_types": ["definition"]
            })),
        )
        .await;
        assert!(definition_only.is_err());

        let usage_only = execute(
            fixture.workspace.path(),
            request(json!({
                "query": "Widget",
                "path": "src/widget.rs",
                "result_types": ["usage"]
            })),
        )
        .await;
        assert!(usage_only.is_err());

        let mixed = execute(
            fixture.workspace.path(),
            request(json!({
                "query": "Widget",
                "path": "src/widget.rs",
                "result_types": ["definition", "text"]
            })),
        )
        .await
        .expect("text keeps mixed request successful");
        assert!(
            mixed
                .results
                .iter()
                .all(|result| result.result_type == CodeSearchResultType::Text)
        );
        assert!(mixed.hints.iter().any(|hint| hint.contains("definition")));
    }

    #[test]
    fn code_search_incomplete_declaration_inventory_preserves_text() {
        let fixture = code_search_fixture();
        let scope = scope::resolve_scope(fixture.workspace.path(), "src/widget.rs").expect("scope");
        let canonical = identity::canonicalize_existing_prefix(&fixture.workspace.path().join("src/widget.rs"));
        let mut inventories = HashMap::new();
        inventories.insert(canonical, DeclarationInventory { complete: false, exact_name_ranges: Vec::new() });
        let mut candidates = Vec::new();
        backends::classify_literal_candidates(
            &scope,
            &[AstGrepLanguage::Rust],
            vec![LiteralSearchCandidate {
                path: fixture.workspace.path().join("src/widget.rs"),
                line: 1,
                column: 4,
                byte_start: 3,
                byte_end: 9,
                matched_text: "Widget".to_string(),
                snippet: "fn Widget() {\n".to_string(),
            }],
            true,
            true,
            false,
            &mut HashMap::new(),
            &inventories,
            &mut candidates,
        );

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].result.result_type, CodeSearchResultType::Text);
    }

    #[test]
    fn code_search_definitions_require_exact_name_ranges() {
        let fixture = code_search_fixture();
        let scope = scope::resolve_scope(fixture.workspace.path(), "src/widget.rs").expect("scope");
        let canonical = identity::canonicalize_existing_prefix(&fixture.workspace.path().join("src/widget.rs"));
        let source = fs::read_to_string(&canonical).expect("fixture source");
        let exact_end = source.find("}\n").expect("exact declaration end") + 1;
        let missing_name_start = source.find("Widget();").expect("body call");
        let file = DeclarationFileRecord {
            path: PathBuf::from("src/widget.rs"),
            language: AstGrepLanguage::Rust,
            complete: true,
            declarations: vec![
                DeclarationRecord {
                    name: "Widget".to_string(),
                    range: DeclarationRange { byte_start: 0, byte_end: exact_end },
                },
                DeclarationRecord {
                    name: "Different".to_string(),
                    range: DeclarationRange { byte_start: 0, byte_end: exact_end },
                },
                DeclarationRecord {
                    name: "Widget".to_string(),
                    range: DeclarationRange {
                        byte_start: missing_name_start,
                        byte_end: missing_name_start + "Widget()".len(),
                    },
                },
            ],
        };
        let mut inventories = HashMap::new();
        let mut candidates = Vec::new();
        backends::process_declaration_file(
            &scope,
            &[AstGrepLanguage::Rust],
            "Widget",
            &file,
            true,
            &mut HashMap::new(),
            &mut inventories,
            &mut candidates,
        );

        assert_eq!(candidates.len(), 1, "only the exact definition is emitted");
        assert_eq!(candidates[0].result.line, Some(1));
        assert_eq!(candidates[0].result.column, Some(4));
        let inventory = inventories.get(&canonical).expect("inventory");
        assert!(!inventory.complete, "any failed exact range suppresses usage");
        assert_eq!(inventory.exact_name_ranges.len(), 1);

        let mut mixed = candidates;
        backends::classify_literal_candidates(
            &scope,
            &[AstGrepLanguage::Rust],
            vec![
                LiteralSearchCandidate {
                    path: canonical.clone(),
                    line: 1,
                    column: 4,
                    byte_start: 3,
                    byte_end: 9,
                    matched_text: "Widget".to_string(),
                    snippet: "fn Widget() {".to_string(),
                },
                LiteralSearchCandidate {
                    path: canonical,
                    line: 2,
                    column: 5,
                    byte_start: missing_name_start,
                    byte_end: missing_name_start + 6,
                    matched_text: "Widget".to_string(),
                    snippet: "Widget();".to_string(),
                },
            ],
            true,
            true,
            false,
            &mut HashMap::new(),
            &inventories,
            &mut mixed,
        );
        let results = ranking::deduplicate_and_order(mixed);
        assert_eq!(
            results
                .iter()
                .filter(|result| result.result_type == CodeSearchResultType::Definition)
                .count(),
            1
        );
        assert!(
            results
                .iter()
                .any(|result| { result.result_type == CodeSearchResultType::Text && result.line == Some(2) })
        );
    }

    #[test]
    fn code_search_scope_inventory_is_limited_to_requested_path() {
        let fixture = code_search_fixture();
        let direct = scope::resolve_scope(fixture.workspace.path(), "src/widget.rs").expect("file scope");
        assert_eq!(direct.allowed_files.len(), 1);
        assert!(direct.allowed_files.contains(&direct.requested_path));

        let subtree = scope::resolve_scope(fixture.workspace.path(), "src").expect("directory scope");
        assert!(
            subtree
                .allowed_files
                .iter()
                .all(|path| path.starts_with(&subtree.requested_path))
        );
        assert!(
            !subtree
                .allowed_files
                .contains(&identity::canonicalize_existing_prefix(&fixture.workspace.path().join("ignored.rs")))
        );
    }

    #[test]
    fn code_search_deduplication_keeps_strongest_location() {
        let candidate = |result_type, backend_ordinal| ranking::RankedCandidate {
            result: CodeSearchResult {
                result_type,
                path: CompactStr::from("src/widget.rs"),
                line: Some(1),
                column: Some(4),
                name: None,
                snippet: Some(CompactStr::from("fn Widget() {}")),
            },
            backend_ordinal,
        };

        let results = ranking::deduplicate_and_order(vec![
            candidate(CodeSearchResultType::Text, 2),
            candidate(CodeSearchResultType::Usage, 1),
            candidate(CodeSearchResultType::Definition, 0),
        ]);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].result_type, CodeSearchResultType::Definition);
    }

    #[test]
    fn code_search_path_candidates_deduplicate_canonical_aliases_before_merge() {
        let fixture = code_search_fixture();
        let scope = scope::resolve_scope(fixture.workspace.path(), "src").expect("scope");
        let mut candidates = Vec::new();
        ranking::append_path_candidates(
            &scope,
            &scope.requested_path,
            [PathBuf::from("WidgetConfig.rs"), PathBuf::from("./WidgetConfig.rs")],
            &[AstGrepLanguage::Rust],
            &mut candidates,
        );

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].result.path, "src/WidgetConfig.rs");
    }
}
