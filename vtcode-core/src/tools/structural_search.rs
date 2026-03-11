use anyhow::{Context, Result, anyhow, bail};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use tokio::process::Command;

use crate::tools::ast_grep_binary::AST_GREP_INSTALL_COMMAND;
use crate::tools::ast_grep_language::AstGrepLanguage;
use crate::tools::editing::patch::resolve_ast_grep_binary_path;
use crate::tools::tree_sitter_runtime::parse_source;
use crate::utils::path::resolve_workspace_path;

const DEFAULT_MAX_RESULTS: usize = 100;
const MAX_ALLOWED_RESULTS: usize = 10_000;
const AST_GREP_FAQ_HINT: &str = "Hints: patterns must be valid parseable code for the selected language; ast-grep matches CST structure, not raw text; if the target is only a fragment, retry with a larger parseable pattern and use `selector` when the real match is a subnode inside that pattern; `$VAR` matches named nodes by default and `$$VAR` includes unnamed nodes; if node role matters, make it explicit in the parseable pattern instead of guessing; structural search is syntax-aware, not scope/type/data-flow analysis.";
const AST_GREP_PROJECT_CONFIG_HINT: &str = "If the target language is not built into ast-grep, register it in workspace-local `sgconfig.yml` under `customLanguages` with a compiled tree-sitter dynamic library. If the parser exists but the extension is unusual, map it with `languageGlobs`. If the target syntax is embedded inside another host language, configure `languageInjections`. If `$VAR` is not valid syntax for that language, use its configured `expandoChar` instead.";
const DEBUG_QUERY_LANG_HINT: &str = "action='structural' requires an effective `lang` when `debug_query` is set. Inference only works for unambiguous file paths or single-language positive globs; narrow `path`, add a single-language glob, or set `lang` explicitly";
const STRUCTURAL_FORBIDDEN_KEYS: &[&str] = &[
    "rewrite",
    "interactive",
    "update_all",
    "rule",
    "inline_rules",
    "config",
];
static AST_GREP_METAVARIABLE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\$\$?[A-Za-z_][A-Za-z0-9_]*").expect("ast-grep metavariable regex must compile")
});

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StructuralStrictness {
    Cst,
    Smart,
    Ast,
    Relaxed,
    Signature,
    Template,
}

impl StructuralStrictness {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Cst => "cst",
            Self::Smart => "smart",
            Self::Ast => "ast",
            Self::Relaxed => "relaxed",
            Self::Signature => "signature",
            Self::Template => "template",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DebugQueryFormat {
    Pattern,
    Ast,
    Cst,
    Sexp,
}

impl DebugQueryFormat {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Pattern => "pattern",
            Self::Ast => "ast",
            Self::Cst => "cst",
            Self::Sexp => "sexp",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum GlobInput {
    Single(String),
    Multiple(Vec<String>),
}

impl GlobInput {
    fn into_vec(self) -> Vec<String> {
        match self {
            Self::Single(glob) => vec![glob],
            Self::Multiple(globs) => globs,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct StructuralSearchRequest {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    lang: Option<String>,
    #[serde(default)]
    selector: Option<String>,
    #[serde(default)]
    strictness: Option<StructuralStrictness>,
    #[serde(default)]
    debug_query: Option<DebugQueryFormat>,
    #[serde(default)]
    globs: Option<GlobInput>,
    #[serde(default)]
    context_lines: Option<usize>,
    #[serde(default)]
    max_results: Option<usize>,
}

impl StructuralSearchRequest {
    fn from_args(args: &Value) -> Result<Self> {
        reject_forbidden_args(args)?;

        let mut request: Self =
            serde_json::from_value(args.clone()).context("invalid structural search args")?;
        request.normalize_language();

        if request.pattern.trim().is_empty() {
            bail!("action='structural' requires a non-empty `pattern`");
        }

        if request.debug_query.is_some() && request.lang.as_deref().is_none_or(str::is_empty) {
            bail!(DEBUG_QUERY_LANG_HINT);
        }

        Ok(request)
    }

    fn requested_path(&self) -> &str {
        self.path
            .as_deref()
            .filter(|path| !path.trim().is_empty())
            .unwrap_or(".")
    }

    fn effective_max_results(&self) -> usize {
        self.max_results
            .unwrap_or(DEFAULT_MAX_RESULTS)
            .clamp(1, MAX_ALLOWED_RESULTS)
    }

    fn normalize_language(&mut self) {
        self.lang = self.normalized_or_inferred_lang();
    }

    fn normalized_or_inferred_lang(&self) -> Option<String> {
        if let Some(lang) = self
            .lang
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(
                AstGrepLanguage::from_user_value(lang)
                    .map(|language| language.as_str().to_string())
                    .unwrap_or_else(|| lang.to_string()),
            );
        }

        if let Some(language) = AstGrepLanguage::infer_from_path_str(self.requested_path()) {
            return Some(language.as_str().to_string());
        }

        let inferred = match self.globs.as_ref() {
            Some(GlobInput::Single(glob)) => {
                AstGrepLanguage::infer_from_positive_globs([glob.as_str()])
            }
            Some(GlobInput::Multiple(globs)) => {
                AstGrepLanguage::infer_from_positive_globs(globs.iter().map(String::as_str))
            }
            None => None,
        };

        inferred.map(|language| language.as_str().to_string())
    }
}

#[derive(Debug, Clone, Deserialize)]
struct AstGrepMatch {
    file: String,
    text: String,
    #[serde(default)]
    lines: Option<String>,
    #[serde(default)]
    language: Option<String>,
    range: AstGrepRange,
}

#[derive(Debug, Clone, Deserialize)]
struct AstGrepRange {
    start: AstGrepPoint,
    end: AstGrepPoint,
}

#[derive(Debug, Clone, Deserialize)]
struct AstGrepPoint {
    line: usize,
    column: usize,
}

pub async fn execute_structural_search(workspace_root: &Path, args: Value) -> Result<Value> {
    let request = StructuralSearchRequest::from_args(&args)?;
    let ast_grep = resolve_ast_grep_binary_path().map_err(|reason| {
        anyhow!(
            "Structural search requires ast-grep (`sg`). {reason}. Install it with `{AST_GREP_INSTALL_COMMAND}`."
        )
    })?;
    let search_path = resolve_search_path(workspace_root, request.requested_path())?;
    if let Some(hint) = preflight_parseable_pattern(&request)? {
        return Ok(build_fragment_result(
            &request,
            &search_path.display_path,
            hint,
        ));
    }
    let command_path = search_path.command_arg.clone();

    if let Some(debug_query) = &request.debug_query {
        let mut command = Command::new(&ast_grep);
        command
            .current_dir(workspace_root)
            .arg("run")
            .arg(format!("--pattern={}", request.pattern))
            .arg("--lang")
            .arg(
                request
                    .lang
                    .as_deref()
                    .expect("validated lang for debug query"),
            )
            .arg(format!("--debug-query={}", debug_query.as_str()))
            .arg(&command_path);

        let output = command
            .output()
            .await
            .context("failed to run ast-grep debug query")?;

        if !output.status.success() {
            bail!(
                "{}",
                format_ast_grep_failure(
                    "ast-grep debug query failed",
                    stderr_or_stdout(&output.stderr, &output.stdout)
                )
            );
        }

        return Ok(json!({
            "backend": "ast-grep",
            "pattern": request.pattern,
            "path": search_path.display_path,
            "lang": request.lang,
            "debug_query": debug_query.as_str(),
            "debug_query_output": String::from_utf8_lossy(&output.stdout).trim(),
            "matches": [],
            "truncated": false,
        }));
    }

    let mut command = Command::new(&ast_grep);
    command
        .current_dir(workspace_root)
        .arg("run")
        .arg(format!("--pattern={}", request.pattern))
        .arg("--json=compact")
        .arg("--color=never");

    if let Some(lang) = request
        .lang
        .as_deref()
        .filter(|lang| !lang.trim().is_empty())
    {
        command.arg("--lang").arg(lang);
    }
    if let Some(selector) = request
        .selector
        .as_deref()
        .filter(|selector| !selector.trim().is_empty())
    {
        command.arg("--selector").arg(selector);
    }
    if let Some(strictness) = &request.strictness {
        command.arg("--strictness").arg(strictness.as_str());
    }
    if let Some(context_lines) = request.context_lines {
        command.arg("--context").arg(context_lines.to_string());
    }
    if let Some(globs) = request.globs.clone() {
        for glob in globs
            .into_vec()
            .into_iter()
            .filter(|glob| !glob.trim().is_empty())
        {
            command.arg("--globs").arg(glob);
        }
    }
    command.arg(&command_path);

    let output = command
        .output()
        .await
        .context("failed to run ast-grep structural search")?;

    if !output.status.success() {
        bail!(
            "{}",
            format_ast_grep_failure(
                "ast-grep structural search failed",
                stderr_or_stdout(&output.stderr, &output.stdout)
            )
        );
    }

    let matches = parse_compact_matches(&output.stdout)?;
    Ok(build_result(&request, &search_path.display_path, matches))
}

fn preflight_parseable_pattern(request: &StructuralSearchRequest) -> Result<Option<String>> {
    let Some(language) = request
        .lang
        .as_deref()
        .and_then(AstGrepLanguage::from_user_value)
    else {
        return Ok(None);
    };

    let (sanitized_pattern, contains_metavariables) =
        sanitize_pattern_for_tree_sitter(&request.pattern);
    let tree = match parse_source(language, &sanitized_pattern) {
        Ok(tree) => tree,
        Err(_) if contains_metavariables => {
            return Ok(Some(fragment_pattern_hint(request, language)));
        }
        Err(detail) => {
            bail!(
                "{}",
                format_ast_grep_failure(
                    "structural pattern preflight failed",
                    format!(
                        "pattern is not parseable as {} syntax ({detail})",
                        language.display_name()
                    ),
                )
            );
        }
    };

    if tree.root_node().has_error() {
        if contains_metavariables {
            return Ok(Some(fragment_pattern_hint(request, language)));
        }

        bail!(
            "{}",
            format_ast_grep_failure(
                "structural pattern preflight failed",
                format!(
                    "pattern is not parseable as {} syntax",
                    language.display_name()
                ),
            )
        );
    }

    Ok(None)
}

fn sanitize_pattern_for_tree_sitter(pattern: &str) -> (String, bool) {
    let mut contains_metavariables = false;
    let sanitized =
        AST_GREP_METAVARIABLE_RE.replace_all(pattern, |captures: &regex::Captures<'_>| {
            contains_metavariables = true;
            if captures
                .get(0)
                .is_some_and(|matched| matched.as_str().starts_with("$$"))
            {
                "placeholders"
            } else {
                "placeholder"
            }
        });

    (sanitized.into_owned(), contains_metavariables)
}

fn reject_forbidden_args(args: &Value) -> Result<()> {
    let Some(object) = args.as_object() else {
        return Ok(());
    };

    for key in STRUCTURAL_FORBIDDEN_KEYS {
        if object.get(*key).is_some() {
            bail!(
                "action='structural' is read-only; remove `{}`. If you need raw `sg scan`, `sg test`, or rewrite workflows, run ast-grep explicitly via `unified_exec`.",
                key
            );
        }
    }

    Ok(())
}

fn parse_compact_matches(stdout: &[u8]) -> Result<Vec<AstGrepMatch>> {
    serde_json::from_slice(stdout).context("failed to parse ast-grep JSON output")
}

fn build_result(
    request: &StructuralSearchRequest,
    display_path: &str,
    matches: Vec<AstGrepMatch>,
) -> Value {
    let max_results = request.effective_max_results();
    let truncated = matches.len() > max_results;
    let normalized_matches = matches
        .into_iter()
        .take(max_results)
        .map(normalize_match)
        .collect::<Vec<_>>();

    json!({
        "backend": "ast-grep",
        "pattern": request.pattern,
        "path": display_path,
        "matches": normalized_matches,
        "truncated": truncated,
    })
}

fn build_fragment_result(
    request: &StructuralSearchRequest,
    display_path: &str,
    hint: String,
) -> Value {
    json!({
        "backend": "ast-grep",
        "pattern": request.pattern,
        "path": display_path,
        "matches": [],
        "truncated": false,
        "hint": hint,
    })
}

fn normalize_match(entry: AstGrepMatch) -> Value {
    let mut match_object = Map::new();
    match_object.insert("file".to_string(), Value::String(entry.file));
    match_object.insert("line_number".to_string(), json!(entry.range.start.line));
    match_object.insert("text".to_string(), Value::String(entry.text.clone()));
    match_object.insert(
        "lines".to_string(),
        Value::String(entry.lines.unwrap_or(entry.text)),
    );
    if let Some(language) = entry.language {
        match_object.insert("language".to_string(), Value::String(language));
    }
    match_object.insert(
        "range".to_string(),
        json!({
            "start": {
                "line": entry.range.start.line,
                "column": entry.range.start.column,
            },
            "end": {
                "line": entry.range.end.line,
                "column": entry.range.end.column,
            },
        }),
    );
    Value::Object(match_object)
}

fn stderr_or_stdout(stderr: &[u8], stdout: &[u8]) -> String {
    let stderr_text = String::from_utf8_lossy(stderr).trim().to_string();
    if !stderr_text.is_empty() {
        return stderr_text;
    }

    let stdout_text = String::from_utf8_lossy(stdout).trim().to_string();
    if !stdout_text.is_empty() {
        return stdout_text;
    }

    "ast-grep exited without output".to_string()
}

fn format_ast_grep_failure(prefix: &str, detail: String) -> String {
    let needs_project_config_hint = looks_like_language_support_issue(&detail);
    let mut message = format!("{prefix}: {detail}. {AST_GREP_FAQ_HINT}");
    if needs_project_config_hint {
        message.push(' ');
        message.push_str(AST_GREP_PROJECT_CONFIG_HINT);
    }
    message.push_str(
        " Retry `unified_search` with a refined structural pattern before switching tools. Use `unified_exec` only for raw `sg scan`, `sg test`, or rewrite workflows.",
    );
    if !detail.contains(AST_GREP_INSTALL_COMMAND) {
        message.push(' ');
        message.push_str(&format!(
            "If the binary is missing, install it with `{AST_GREP_INSTALL_COMMAND}`."
        ));
    }
    message
}

fn looks_like_language_support_issue(detail: &str) -> bool {
    let detail = detail.to_ascii_lowercase();
    (detail.contains("lang") || detail.contains("language") || detail.contains("extension"))
        && (detail.contains("unsupported")
            || detail.contains("invalid value")
            || detail.contains("unknown")
            || detail.contains("unrecognized")
            || detail.contains("not built in")
            || detail.contains("not supported"))
}

fn fragment_pattern_hint(request: &StructuralSearchRequest, language: AstGrepLanguage) -> String {
    let trimmed = request.pattern.trim();
    let mut message = format!(
        "Pattern looks like a code fragment, not standalone parseable {} syntax for `action='structural'`.",
        language.display_name()
    );

    if language == AstGrepLanguage::Rust
        && (trimmed.starts_with("Result<")
            || trimmed.starts_with("-> Result<")
            || trimmed.contains("-> Result<"))
    {
        message.push_str(
            " For Result return-type queries, anchor it in a full signature like `fn $NAME($$ARGS) -> Result<$T> { $$BODY }`.",
        );
    } else {
        message.push_str(
            " Wrap the target in surrounding parseable code, then use `selector` only to focus the real subnode inside that larger pattern.",
        );
    }

    message.push_str(" Retry `unified_search` with `action='structural'` using a larger parseable pattern before switching tools. Do not retry the same fragment with grep if syntax matters.");
    message
}

struct ResolvedSearchPath {
    command_arg: String,
    display_path: String,
}

fn resolve_search_path(workspace_root: &Path, requested_path: &str) -> Result<ResolvedSearchPath> {
    let resolved = resolve_workspace_path(workspace_root, PathBuf::from(requested_path).as_path())
        .with_context(|| format!("Failed to resolve structural search path: {requested_path}"))?;

    let display_path = if let Ok(relative) = resolved.strip_prefix(workspace_root) {
        if relative.as_os_str().is_empty() {
            ".".to_string()
        } else {
            relative.to_string_lossy().replace('\\', "/")
        }
    } else {
        resolved.to_string_lossy().replace('\\', "/")
    };

    let command_arg = if display_path == "." {
        ".".to_string()
    } else if Path::new(&display_path).is_relative() {
        display_path.clone()
    } else {
        resolved.to_string_lossy().to_string()
    };

    Ok(ResolvedSearchPath {
        command_arg,
        display_path,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        StructuralSearchRequest, build_result, execute_structural_search, format_ast_grep_failure,
        normalize_match, preflight_parseable_pattern, sanitize_pattern_for_tree_sitter,
    };
    use crate::tools::ast_grep_binary::AST_GREP_INSTALL_COMMAND;
    use crate::tools::editing::patch::set_ast_grep_binary_override_for_tests;
    use serde_json::json;
    use serial_test::serial;
    use std::{fs, path::PathBuf};
    use tempfile::TempDir;

    fn request() -> StructuralSearchRequest {
        StructuralSearchRequest::from_args(&json!({
            "action": "structural",
            "pattern": "fn $NAME() {}",
            "path": "src",
            "max_results": 2
        }))
        .expect("valid request")
    }

    fn write_fake_sg(script_body: &str) -> (TempDir, PathBuf) {
        let script_dir = TempDir::new().expect("script tempdir");
        let script_path = script_dir.path().join("sg");
        fs::write(&script_path, script_body).expect("write fake sg");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&script_path).expect("metadata").permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script_path, perms).expect("chmod");
        }
        (script_dir, script_path)
    }

    #[test]
    fn normalize_match_emits_vtcode_shape() {
        let match_value = normalize_match(super::AstGrepMatch {
            file: "src/lib.rs".to_string(),
            text: "fn alpha() {}".to_string(),
            lines: Some("12: fn alpha() {}".to_string()),
            language: Some("Rust".to_string()),
            range: super::AstGrepRange {
                start: super::AstGrepPoint {
                    line: 12,
                    column: 0,
                },
                end: super::AstGrepPoint {
                    line: 12,
                    column: 13,
                },
            },
        });

        assert_eq!(match_value["file"], "src/lib.rs");
        assert_eq!(match_value["line_number"], 12);
        assert_eq!(match_value["text"], "fn alpha() {}");
        assert_eq!(match_value["lines"], "12: fn alpha() {}");
        assert_eq!(match_value["language"], "Rust");
        assert_eq!(match_value["range"]["start"]["column"], 0);
        assert_eq!(match_value["range"]["end"]["column"], 13);
    }

    #[test]
    fn build_result_truncates_matches() {
        let result = build_result(
            &request(),
            "src",
            vec![
                super::AstGrepMatch {
                    file: "src/lib.rs".to_string(),
                    text: "fn alpha() {}".to_string(),
                    lines: None,
                    language: Some("Rust".to_string()),
                    range: super::AstGrepRange {
                        start: super::AstGrepPoint {
                            line: 10,
                            column: 0,
                        },
                        end: super::AstGrepPoint {
                            line: 10,
                            column: 13,
                        },
                    },
                },
                super::AstGrepMatch {
                    file: "src/lib.rs".to_string(),
                    text: "fn beta() {}".to_string(),
                    lines: None,
                    language: Some("Rust".to_string()),
                    range: super::AstGrepRange {
                        start: super::AstGrepPoint {
                            line: 20,
                            column: 0,
                        },
                        end: super::AstGrepPoint {
                            line: 20,
                            column: 12,
                        },
                    },
                },
                super::AstGrepMatch {
                    file: "src/lib.rs".to_string(),
                    text: "fn gamma() {}".to_string(),
                    lines: None,
                    language: Some("Rust".to_string()),
                    range: super::AstGrepRange {
                        start: super::AstGrepPoint {
                            line: 30,
                            column: 0,
                        },
                        end: super::AstGrepPoint {
                            line: 30,
                            column: 13,
                        },
                    },
                },
            ],
        );

        assert_eq!(result["backend"], "ast-grep");
        assert_eq!(result["pattern"], "fn $NAME() {}");
        assert_eq!(result["path"], "src");
        assert_eq!(result["matches"].as_array().expect("matches").len(), 2);
        assert_eq!(result["truncated"], true);
    }

    #[test]
    fn structural_request_requires_pattern() {
        let err = StructuralSearchRequest::from_args(&json!({
            "action": "structural",
            "pattern": "   "
        }))
        .expect_err("pattern required");

        assert!(err.to_string().contains("requires a non-empty `pattern`"));
    }

    #[test]
    fn structural_request_requires_lang_for_debug_query() {
        let err = StructuralSearchRequest::from_args(&json!({
            "action": "structural",
            "pattern": "fn $NAME() {}",
            "debug_query": "ast"
        }))
        .expect_err("lang required");

        assert!(err.to_string().contains(
            "Inference only works for unambiguous file paths or single-language positive globs"
        ));
    }

    #[test]
    fn structural_request_canonicalizes_known_lang_alias() {
        let request = StructuralSearchRequest::from_args(&json!({
            "action": "structural",
            "pattern": "fn $NAME() {}",
            "lang": "ts"
        }))
        .expect("valid request");

        assert_eq!(request.lang.as_deref(), Some("typescript"));
    }

    #[test]
    fn structural_request_infers_lang_from_unambiguous_path() {
        let request = StructuralSearchRequest::from_args(&json!({
            "action": "structural",
            "pattern": "fn $NAME() {}",
            "path": "src/lib.rs",
            "debug_query": "ast"
        }))
        .expect("path inference should satisfy debug query");

        assert_eq!(request.lang.as_deref(), Some("rust"));
    }

    #[test]
    fn structural_request_infers_lang_from_unambiguous_globs() {
        let request = StructuralSearchRequest::from_args(&json!({
            "action": "structural",
            "pattern": "fn $NAME() {}",
            "globs": ["**/*.rs", "!target/**"],
            "debug_query": "ast"
        }))
        .expect("glob inference should satisfy debug query");

        assert_eq!(request.lang.as_deref(), Some("rust"));
    }

    #[test]
    fn structural_request_rejects_rewrite_keys() {
        let err = StructuralSearchRequest::from_args(&json!({
            "action": "structural",
            "pattern": "fn $NAME() {}",
            "rewrite": "fn renamed() {}"
        }))
        .expect_err("rewrite rejected");

        assert!(err.to_string().contains("read-only"));
        assert!(err.to_string().contains("ast-grep"));
    }

    #[test]
    fn sanitize_pattern_for_tree_sitter_rewrites_ast_grep_metavariables() {
        let (sanitized, contains_metavariables) =
            sanitize_pattern_for_tree_sitter("fn $NAME($$ARGS) { $BODY }");

        assert!(contains_metavariables);
        assert_eq!(sanitized, "fn placeholder(placeholders) { placeholder }");
    }

    #[test]
    fn structural_pattern_preflight_accepts_supported_metavariable_patterns() {
        let request = StructuralSearchRequest::from_args(&json!({
            "action": "structural",
            "pattern": "fn $NAME($$ARGS) {}",
            "lang": "rust"
        }))
        .expect("valid request");

        assert!(
            preflight_parseable_pattern(&request)
                .expect("metavariable pattern should preflight")
                .is_none()
        );
    }

    #[test]
    fn structural_pattern_preflight_guides_result_type_fragments() {
        let request = StructuralSearchRequest::from_args(&json!({
            "action": "structural",
            "pattern": "Result<$T>",
            "lang": "rust"
        }))
        .expect("valid request");

        let hint = preflight_parseable_pattern(&request)
            .expect("fragment hint should be returned")
            .expect("expected guidance");
        assert!(hint.contains("Result return-type queries"), "{hint}");
        assert!(
            hint.contains("fn $NAME($$ARGS) -> Result<$T> { $$BODY }"),
            "{hint}"
        );
        assert!(
            hint.contains("Do not retry the same fragment with grep"),
            "{hint}"
        );
    }

    #[test]
    fn structural_pattern_preflight_guides_return_arrow_fragments() {
        let request = StructuralSearchRequest::from_args(&json!({
            "action": "structural",
            "pattern": "-> Result<$T>",
            "lang": "rust"
        }))
        .expect("valid request");

        let hint = preflight_parseable_pattern(&request)
            .expect("fragment hint should be returned")
            .expect("expected guidance");
        assert!(hint.contains("Result return-type queries"), "{hint}");
        assert!(
            hint.contains("fn $NAME($$ARGS) -> Result<$T> { $$BODY }"),
            "{hint}"
        );
    }

    #[tokio::test]
    #[serial]
    async fn structural_search_reports_missing_ast_grep() {
        let temp = TempDir::new().expect("workspace tempdir");
        let _override = set_ast_grep_binary_override_for_tests(None);

        let err = execute_structural_search(
            temp.path(),
            json!({
                "action": "structural",
                "pattern": "fn $NAME() {}",
                "path": "."
            }),
        )
        .await
        .expect_err("missing ast-grep");

        let text = err.to_string();
        assert!(text.contains("ast-grep"));
        assert!(text.contains(AST_GREP_INSTALL_COMMAND));
    }

    #[tokio::test]
    #[serial]
    async fn structural_search_preflight_rejects_invalid_literal_pattern_before_ast_grep_runs() {
        let temp = TempDir::new().expect("workspace tempdir");
        let invoked_marker = temp.path().join("sg_invoked");
        let script = format!(
            "#!/bin/sh\ntouch \"{}\"\nprintf '[]'\n",
            invoked_marker.display()
        );
        let (_script_dir, script_path) = write_fake_sg(&script);

        let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
        let err = execute_structural_search(
            temp.path(),
            json!({
                "action": "structural",
                "pattern": "fn alpha( {}",
                "lang": "rust",
                "path": "."
            }),
        )
        .await
        .expect_err("invalid literal pattern should fail in preflight");

        let text = err.to_string();
        assert!(
            text.contains("structural pattern preflight failed"),
            "{text}"
        );
        assert!(text.contains("valid parseable code"), "{text}");
        assert!(!invoked_marker.exists(), "ast-grep should not be invoked");
    }

    #[tokio::test]
    #[serial]
    async fn structural_search_returns_fragment_guidance_without_running_ast_grep() {
        let temp = TempDir::new().expect("workspace tempdir");
        let invoked_marker = temp.path().join("sg_invoked");
        let script = format!(
            "#!/bin/sh\ntouch \"{}\"\nprintf '[]'\n",
            invoked_marker.display()
        );
        let (_script_dir, script_path) = write_fake_sg(&script);

        let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
        let result = execute_structural_search(
            temp.path(),
            json!({
                "action": "structural",
                "pattern": "Result<$T>",
                "lang": "rust",
                "path": "."
            }),
        )
        .await
        .expect("fragment guidance should be returned");

        assert_eq!(result["matches"], json!([]));
        let hint = result["hint"].as_str().expect("hint");
        assert!(hint.contains("Result return-type queries"), "{hint}");
        assert!(
            hint.contains("fn $NAME($$ARGS) -> Result<$T> { $$BODY }"),
            "{hint}"
        );
        assert!(
            hint.contains("Retry `unified_search` with `action='structural'`"),
            "{hint}"
        );
        assert!(!hint.contains("load_skill"), "{hint}");
        assert!(!invoked_marker.exists(), "ast-grep should not be invoked");
    }

    #[tokio::test]
    #[serial]
    async fn structural_search_arrow_fragment_guidance_prefers_direct_retry() {
        let temp = TempDir::new().expect("workspace tempdir");
        let invoked_marker = temp.path().join("sg_invoked");
        let script = format!(
            "#!/bin/sh\ntouch \"{}\"\nprintf '[]'\n",
            invoked_marker.display()
        );
        let (_script_dir, script_path) = write_fake_sg(&script);

        let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
        let result = execute_structural_search(
            temp.path(),
            json!({
                "action": "structural",
                "pattern": "-> Result<$T>",
                "lang": "rust",
                "path": "."
            }),
        )
        .await
        .expect("fragment guidance should be returned");

        let hint = result["hint"].as_str().expect("hint");
        assert!(hint.contains("Result return-type queries"), "{hint}");
        assert!(
            hint.contains("Retry `unified_search` with `action='structural'`"),
            "{hint}"
        );
        assert!(!hint.contains("load_skill"), "{hint}");
        assert!(!invoked_marker.exists(), "ast-grep should not be invoked");
    }

    #[tokio::test]
    #[serial]
    async fn structural_search_passes_leading_dash_patterns_with_equals_syntax() {
        let temp = TempDir::new().expect("workspace tempdir");
        let args_path = temp.path().join("sg_args.txt");
        let script = format!(
            "#!/bin/sh\nprintf '%s\n' \"$@\" > \"{}\"\nprintf '[]'\n",
            args_path.display()
        );
        let (_script_dir, script_path) = write_fake_sg(&script);

        let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
        let result = execute_structural_search(
            temp.path(),
            json!({
                "action": "structural",
                "pattern": "const X: i32 = -1;",
                "lang": "rust",
                "path": "."
            }),
        )
        .await
        .expect("search should run");

        assert_eq!(result["matches"], json!([]));
        let args = fs::read_to_string(args_path).expect("read sg args");
        assert!(
            args.lines()
                .any(|line| line == "--pattern=const X: i32 = -1;")
        );
    }

    #[tokio::test]
    #[serial]
    async fn structural_search_debug_query_uses_inferred_path_language() {
        let temp = TempDir::new().expect("workspace tempdir");
        let src_dir = temp.path().join("src");
        fs::create_dir_all(&src_dir).expect("create src dir");
        fs::write(src_dir.join("lib.rs"), "fn alpha() {}\n").expect("write rust file");
        let (_script_dir, script_path) = write_fake_sg("#!/bin/sh\nprintf 'query-ast'\n");

        let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
        let result = execute_structural_search(
            temp.path(),
            json!({
                "action": "structural",
                "pattern": "fn $NAME() {}",
                "path": "src/lib.rs",
                "debug_query": "ast"
            }),
        )
        .await
        .expect("debug query should succeed");

        assert_eq!(result["lang"], "rust");
        assert_eq!(result["debug_query"], "ast");
        assert_eq!(result["debug_query_output"], "query-ast");
    }

    #[test]
    fn structural_path_defaults_to_workspace_root() {
        let request = StructuralSearchRequest::from_args(&json!({
            "action": "structural",
            "pattern": "fn $NAME() {}"
        }))
        .expect("valid request");

        assert_eq!(request.requested_path(), ".");
    }

    #[test]
    fn structural_failure_message_includes_faq_guidance() {
        let text = format_ast_grep_failure(
            "ast-grep structural search failed",
            "parse error".to_string(),
        );

        assert!(text.contains("valid parseable code"));
        assert!(text.contains("use `selector`"));
        assert!(text.contains("`$$VAR`"));
        assert!(text.contains("not scope/type/data-flow analysis"));
        assert!(text.contains("Retry `unified_search`"));
        assert!(text.contains("`unified_exec`"));
    }

    #[test]
    fn structural_failure_message_skips_project_config_hint_for_parse_errors() {
        let text = format_ast_grep_failure(
            "ast-grep structural search failed",
            "parse error near pattern".to_string(),
        );

        assert!(!text.contains("customLanguages"));
        assert!(!text.contains("languageGlobs"));
        assert!(!text.contains("languageInjections"));
    }

    #[test]
    fn structural_failure_message_includes_custom_language_guidance() {
        let text = format_ast_grep_failure(
            "ast-grep structural search failed",
            "unsupported language: mojo".to_string(),
        );

        assert!(text.contains("customLanguages"));
        assert!(text.contains("tree-sitter dynamic library"));
        assert!(text.contains("languageGlobs"));
        assert!(text.contains("languageInjections"));
        assert!(text.contains("expandoChar"));
        assert!(text.contains("Retry `unified_search`"));
        assert!(text.contains("`unified_exec`"));
    }

    #[tokio::test]
    #[serial]
    async fn structural_search_failure_surfaces_faq_guidance() {
        let temp = TempDir::new().expect("workspace tempdir");
        let (_script_dir, script_path) =
            write_fake_sg("#!/bin/sh\nprintf 'parse error near pattern' >&2\nexit 1\n");

        let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
        let err = execute_structural_search(
            temp.path(),
            json!({
                "action": "structural",
                "pattern": "\"key\": \"$VAL\"",
                "lang": "json",
                "path": "."
            }),
        )
        .await
        .expect_err("structural search should fail");

        let text = err.to_string();
        assert!(text.contains("valid parseable code"));
        assert!(text.contains("use `selector`"));
        assert!(!text.contains("customLanguages"));
        assert!(text.contains("Retry `unified_search`"));
    }

    #[tokio::test]
    #[serial]
    async fn structural_search_failure_surfaces_custom_language_guidance() {
        let temp = TempDir::new().expect("workspace tempdir");
        let (_script_dir, script_path) =
            write_fake_sg("#!/bin/sh\nprintf 'unsupported language: mojo' >&2\nexit 1\n");

        let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
        let err = execute_structural_search(
            temp.path(),
            json!({
                "action": "structural",
                "pattern": "target($A)",
                "lang": "mojo",
                "path": "."
            }),
        )
        .await
        .expect_err("structural search should fail");

        let text = err.to_string();
        assert!(text.contains("customLanguages"), "{text}");
        assert!(text.contains("languageGlobs"), "{text}");
        assert!(text.contains("languageInjections"), "{text}");
        assert!(text.contains("expandoChar"), "{text}");
        assert!(text.contains("Retry `unified_search`"), "{text}");
        assert!(text.contains("`unified_exec`"), "{text}");
    }
}
