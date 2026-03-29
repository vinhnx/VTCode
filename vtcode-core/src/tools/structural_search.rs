use anyhow::{Context, Result, anyhow, bail};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tokio::process::Command;

use crate::tools::ast_grep_binary::AST_GREP_INSTALL_COMMAND;
use crate::tools::ast_grep_language::AstGrepLanguage;
use crate::tools::editing::patch::resolve_ast_grep_binary_path;
use crate::tools::tree_sitter_runtime::parse_source;
use crate::utils::path::{canonicalize_allow_missing, normalize_path, resolve_workspace_path};

const DEFAULT_MAX_RESULTS: usize = 100;
const MAX_ALLOWED_RESULTS: usize = 10_000;
const DEFAULT_AST_GREP_CONFIG_PATH: &str = "sgconfig.yml";
const AST_GREP_FAQ_HINT: &str = "Hints: patterns must be valid parseable code for the selected language; ast-grep matches CST structure, not raw text; if the target is only a fragment, retry with a larger parseable pattern and use `selector` when the real match is a subnode inside that pattern; `$VAR` matches named nodes by default, `$$VAR` includes unnamed nodes, and `$$$ARGS` matches zero or more nodes such as arguments, parameters, or statements; repeat captured names only when the syntax must match exactly, and prefix with `_` to disable capture when equality is not required; if node role matters, make it explicit in the parseable pattern instead of guessing; structural search is syntax-aware, not scope/type/data-flow analysis.";
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
static ANSI_ESCAPE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").expect("ansi escape regex must compile")
});
static AST_GREP_TEST_RESULT_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"test result:\s*(ok|failed)\.\s*(\d+)\s+passed;\s*(\d+)\s+failed;")
        .expect("ast-grep test summary regex must compile")
});

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
enum StructuralWorkflow {
    #[default]
    Query,
    Scan,
    Test,
}

impl StructuralWorkflow {
    fn as_str(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Scan => "scan",
            Self::Test => "test",
        }
    }
}

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
    #[serde(default)]
    workflow: StructuralWorkflow,
    #[serde(default)]
    pattern: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    config_path: Option<String>,
    #[serde(default)]
    filter: Option<String>,
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
    #[serde(default)]
    skip_snapshot_tests: Option<bool>,
}

impl StructuralSearchRequest {
    fn from_args(args: &Value) -> Result<Self> {
        reject_forbidden_args(args)?;

        let mut request: Self =
            serde_json::from_value(args.clone()).context("invalid structural search args")?;
        request.normalize();
        request.validate()?;

        Ok(request)
    }

    fn normalize(&mut self) {
        if self.workflow == StructuralWorkflow::Query {
            self.lang = self.normalized_or_inferred_lang();
        }
    }

    fn validate(&self) -> Result<()> {
        match self.workflow {
            StructuralWorkflow::Query => self.validate_query(),
            StructuralWorkflow::Scan => self.validate_scan(),
            StructuralWorkflow::Test => self.validate_test(),
        }
    }

    fn validate_query(&self) -> Result<()> {
        if self.pattern().is_none() {
            bail!("action='structural' workflow='query' requires a non-empty `pattern`");
        }

        self.reject_present("config_path", self.config_path.as_deref())?;
        self.reject_present("filter", self.filter.as_deref())?;
        self.reject_flag("skip_snapshot_tests", self.skip_snapshot_tests)?;

        if self.debug_query.is_some() && self.lang.as_deref().is_none_or(str::is_empty) {
            bail!(DEBUG_QUERY_LANG_HINT);
        }

        Ok(())
    }

    fn validate_scan(&self) -> Result<()> {
        self.reject_present("pattern", self.pattern.as_deref())?;
        self.reject_present("lang", self.lang.as_deref())?;
        self.reject_present("selector", self.selector.as_deref())?;
        self.reject_present(
            "strictness",
            self.strictness.as_ref().map(StructuralStrictness::as_str),
        )?;
        self.reject_present(
            "debug_query",
            self.debug_query.as_ref().map(DebugQueryFormat::as_str),
        )?;
        self.reject_flag("skip_snapshot_tests", self.skip_snapshot_tests)?;
        Ok(())
    }

    fn validate_test(&self) -> Result<()> {
        self.reject_present("pattern", self.pattern.as_deref())?;
        self.reject_present("path", self.path.as_deref())?;
        self.reject_present("lang", self.lang.as_deref())?;
        self.reject_present("selector", self.selector.as_deref())?;
        self.reject_present(
            "strictness",
            self.strictness.as_ref().map(StructuralStrictness::as_str),
        )?;
        self.reject_present(
            "debug_query",
            self.debug_query.as_ref().map(DebugQueryFormat::as_str),
        )?;
        if self.globs.is_some() {
            bail!(
                "action='structural' workflow='test' does not accept `globs`; use `config_path`, `filter`, and `skip_snapshot_tests`."
            );
        }
        if self.context_lines.is_some() {
            bail!(
                "action='structural' workflow='test' does not accept `context_lines`; use `config_path`, `filter`, and `skip_snapshot_tests`."
            );
        }
        if self.max_results.is_some() {
            bail!(
                "action='structural' workflow='test' does not accept `max_results`; use `config_path`, `filter`, and `skip_snapshot_tests`."
            );
        }
        Ok(())
    }

    fn reject_present(&self, field: &str, value: Option<&str>) -> Result<()> {
        if value.is_some_and(|value| !value.trim().is_empty()) {
            bail!(
                "action='structural' workflow='{}' does not accept `{field}`.",
                self.workflow.as_str()
            );
        }
        Ok(())
    }

    fn reject_flag(&self, field: &str, value: Option<bool>) -> Result<()> {
        if value.is_some() {
            bail!(
                "action='structural' workflow='{}' does not accept `{field}`.",
                self.workflow.as_str()
            );
        }
        Ok(())
    }

    fn requested_path(&self) -> &str {
        self.path
            .as_deref()
            .filter(|path| !path.trim().is_empty())
            .unwrap_or(".")
    }

    fn requested_config_path(&self) -> &str {
        self.config_path
            .as_deref()
            .filter(|path| !path.trim().is_empty())
            .unwrap_or(DEFAULT_AST_GREP_CONFIG_PATH)
    }

    fn pattern(&self) -> Option<&str> {
        self.pattern
            .as_deref()
            .map(str::trim)
            .filter(|pattern| !pattern.is_empty())
    }

    fn filter(&self) -> Option<&str> {
        self.filter
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    fn effective_max_results(&self) -> usize {
        self.max_results
            .unwrap_or(DEFAULT_MAX_RESULTS)
            .clamp(1, MAX_ALLOWED_RESULTS)
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
struct AstGrepScanFinding {
    file: String,
    text: String,
    #[serde(default)]
    lines: Option<String>,
    #[serde(default)]
    language: Option<String>,
    range: AstGrepRange,
    #[serde(default, rename = "ruleId")]
    rule_id: Option<String>,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    metadata: Option<Value>,
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
    match request.workflow {
        StructuralWorkflow::Query => execute_structural_query(workspace_root, &request, &ast_grep).await,
        StructuralWorkflow::Scan => execute_structural_scan(workspace_root, &request, &ast_grep).await,
        StructuralWorkflow::Test => execute_structural_test(workspace_root, &request, &ast_grep).await,
    }
}

async fn execute_structural_query(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
) -> Result<Value> {
    let search_path = resolve_search_path(workspace_root, request.requested_path())?;
    if let Some(hint) = preflight_parseable_pattern(request)? {
        return Ok(build_fragment_result(request, &search_path.display_path, hint));
    }
    let command_path = search_path.command_arg.clone();

    if let Some(debug_query) = &request.debug_query {
        let mut command = Command::new(ast_grep);
        command
            .current_dir(workspace_root)
            .arg("run")
            .arg(format!(
                "--pattern={}",
                request.pattern().expect("query pattern validated")
            ))
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
            "pattern": request.pattern().expect("query pattern validated"),
            "path": search_path.display_path,
            "lang": request.lang,
            "debug_query": debug_query.as_str(),
            "debug_query_output": String::from_utf8_lossy(&output.stdout).trim(),
            "matches": [],
            "truncated": false,
        }));
    }

    let mut command = Command::new(ast_grep);
    command
        .current_dir(workspace_root)
        .arg("run")
        .arg(format!(
            "--pattern={}",
            request.pattern().expect("query pattern validated")
        ))
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
    Ok(build_query_result(request, &search_path.display_path, matches))
}

async fn execute_structural_scan(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
) -> Result<Value> {
    let search_path = resolve_search_path(workspace_root, request.requested_path())?;
    let config_path = resolve_config_path(workspace_root, request.requested_config_path()).await?;

    let mut command = Command::new(ast_grep);
    command
        .current_dir(workspace_root)
        .arg("scan")
        .arg("--config")
        .arg(&config_path.command_arg)
        .arg("--json=stream")
        .arg("--include-metadata")
        .arg("--color=never");

    if let Some(filter) = request.filter() {
        command.arg("--filter").arg(filter);
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
    command.arg(&search_path.command_arg);

    let output = command
        .output()
        .await
        .context("failed to run ast-grep structural scan")?;

    if !output.status.success() {
        bail!(
            "{}",
            format_ast_grep_failure(
                "ast-grep structural scan failed",
                stderr_or_stdout(&output.stderr, &output.stdout)
            )
        );
    }

    let findings = parse_stream_findings(&output.stdout)?;
    Ok(build_scan_result(
        request,
        &search_path.display_path,
        &config_path.display_path,
        findings,
    ))
}

async fn execute_structural_test(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
) -> Result<Value> {
    let config_path = resolve_config_path(workspace_root, request.requested_config_path()).await?;

    let mut command = Command::new(ast_grep);
    command
        .current_dir(workspace_root)
        .arg("test")
        .arg("--config")
        .arg(&config_path.command_arg);

    if let Some(filter) = request.filter() {
        command.arg("--filter").arg(filter);
    }
    if request.skip_snapshot_tests == Some(true) {
        command.arg("--skip-snapshot-tests");
    }

    let output = command
        .output()
        .await
        .context("failed to run ast-grep structural test")?;

    Ok(build_test_result(
        &config_path.display_path,
        output.status.success(),
        &output.stdout,
        &output.stderr,
    ))
}

fn preflight_parseable_pattern(request: &StructuralSearchRequest) -> Result<Option<String>> {
    let Some(language) = request
        .lang
        .as_deref()
        .and_then(AstGrepLanguage::from_user_value)
    else {
        return Ok(None);
    };

    let (sanitized_pattern, contains_metavariables) = sanitize_pattern_for_tree_sitter(
        request.pattern().expect("query pattern validated"),
    );
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
                "action='structural' is read-only; remove `{}`. For `sg scan`, `sg test`, `sg new`, `sgconfig.yml`, or rewrite-oriented ast-grep tasks, load the bundled `ast-grep` skill first and use `unified_exec` only when the public structural surface cannot express the needed CLI flow.",
                key
            );
        }
    }

    Ok(())
}

fn parse_compact_matches(stdout: &[u8]) -> Result<Vec<AstGrepMatch>> {
    serde_json::from_slice(stdout).context("failed to parse ast-grep JSON output")
}

fn parse_stream_findings(stdout: &[u8]) -> Result<Vec<AstGrepScanFinding>> {
    let stdout = String::from_utf8_lossy(stdout);
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line).with_context(|| {
                format!("failed to parse ast-grep JSON stream output line: {line}")
            })
        })
        .collect()
}

fn build_query_result(
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
        "pattern": request.pattern().expect("query pattern validated"),
        "path": display_path,
        "matches": normalized_matches,
        "truncated": truncated,
    })
}

fn build_scan_result(
    request: &StructuralSearchRequest,
    display_path: &str,
    config_display_path: &str,
    findings: Vec<AstGrepScanFinding>,
) -> Value {
    let max_results = request.effective_max_results();
    let truncated = findings.len() > max_results;
    let normalized_findings = findings
        .iter()
        .take(max_results)
        .map(normalize_scan_finding)
        .collect::<Vec<_>>();

    json!({
        "backend": "ast-grep",
        "workflow": "scan",
        "config_path": config_display_path,
        "path": display_path,
        "findings": normalized_findings,
        "summary": build_scan_summary(&findings, normalized_findings.len(), truncated),
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
        "pattern": request.pattern().expect("query pattern validated"),
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

fn normalize_scan_finding(entry: &AstGrepScanFinding) -> Value {
    let mut finding_object = Map::new();
    finding_object.insert("file".to_string(), Value::String(entry.file.clone()));
    finding_object.insert("line_number".to_string(), json!(entry.range.start.line));
    finding_object.insert("text".to_string(), Value::String(entry.text.clone()));
    finding_object.insert(
        "lines".to_string(),
        Value::String(entry.lines.clone().unwrap_or_else(|| entry.text.clone())),
    );
    if let Some(language) = &entry.language {
        finding_object.insert("language".to_string(), Value::String(language.clone()));
    }
    finding_object.insert(
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
    finding_object.insert("rule_id".to_string(), json!(entry.rule_id));
    finding_object.insert("severity".to_string(), json!(entry.severity));
    finding_object.insert("message".to_string(), json!(entry.message));
    finding_object.insert("note".to_string(), json!(entry.note));
    if let Some(metadata) = &entry.metadata {
        finding_object.insert("metadata".to_string(), metadata.clone());
    }
    Value::Object(finding_object)
}

fn build_scan_summary(findings: &[AstGrepScanFinding], returned: usize, truncated: bool) -> Value {
    let mut by_severity = BTreeMap::new();
    let mut by_rule = BTreeMap::new();

    for finding in findings {
        let severity = finding
            .severity
            .as_deref()
            .unwrap_or("unknown")
            .to_string();
        *by_severity.entry(severity).or_insert(0usize) += 1;

        let rule = finding
            .rule_id
            .as_deref()
            .unwrap_or("unknown")
            .to_string();
        *by_rule.entry(rule).or_insert(0usize) += 1;
    }

    json!({
        "total_findings": findings.len(),
        "returned_findings": returned,
        "truncated": truncated,
        "by_severity": by_severity,
        "by_rule": by_rule,
    })
}

fn build_test_result(
    config_display_path: &str,
    passed: bool,
    stdout: &[u8],
    stderr: &[u8],
) -> Value {
    let stdout = String::from_utf8_lossy(stdout).to_string();
    let stderr = String::from_utf8_lossy(stderr).to_string();

    json!({
        "backend": "ast-grep",
        "workflow": "test",
        "config_path": config_display_path,
        "passed": passed,
        "stdout": stdout,
        "stderr": stderr,
        "summary": summarize_test_output(&stdout, passed),
    })
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
        " Retry `unified_search` with a refined structural pattern before switching tools. For `sg scan`, `sg test`, `sg new`, `sgconfig.yml`, or rewrite-oriented ast-grep tasks, load the bundled `ast-grep` skill first and use `unified_exec` only when the public structural surface and skill guidance still cannot express the needed CLI flow.",
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
    let trimmed = request.pattern().expect("query pattern validated");
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
    let workspace_root = std::fs::canonicalize(workspace_root).with_context(|| {
        format!(
            "Failed to canonicalize workspace root {}",
            workspace_root.display()
        )
    })?;

    let display_path = if let Ok(relative) = resolved.strip_prefix(&workspace_root) {
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

async fn resolve_config_path(workspace_root: &Path, requested_path: &str) -> Result<ResolvedSearchPath> {
    let candidate = if Path::new(requested_path).is_absolute() {
        PathBuf::from(requested_path)
    } else {
        workspace_root.join(requested_path)
    };
    let normalized = normalize_path(&candidate);
    let resolved = canonicalize_allow_missing(&normalized)
        .await
        .with_context(|| format!("Failed to resolve structural config path: {requested_path}"))?;
    let workspace_root = std::fs::canonicalize(workspace_root).with_context(|| {
        format!(
            "Failed to canonicalize workspace root {}",
            workspace_root.display()
        )
    })?;
    if !resolved.starts_with(&workspace_root) {
        bail!(
            "Path {} escapes workspace root {}",
            resolved.display(),
            workspace_root.display()
        );
    }

    let display_path = if let Ok(relative) = resolved.strip_prefix(&workspace_root) {
        if relative.as_os_str().is_empty() {
            ".".to_string()
        } else {
            relative.to_string_lossy().replace('\\', "/")
        }
    } else {
        resolved.to_string_lossy().replace('\\', "/")
    };

    let command_arg = if Path::new(&display_path).is_relative() {
        display_path.clone()
    } else {
        resolved.to_string_lossy().to_string()
    };

    Ok(ResolvedSearchPath {
        command_arg,
        display_path,
    })
}

fn summarize_test_output(stdout: &str, passed: bool) -> Value {
    let clean_stdout = ANSI_ESCAPE_RE.replace_all(stdout, "");
    let mut summary = Map::new();
    summary.insert(
        "status".to_string(),
        Value::String(if passed { "ok" } else { "failed" }.to_string()),
    );

    if let Some(captures) = AST_GREP_TEST_RESULT_RE.captures(&clean_stdout) {
        let passed_cases = captures
            .get(2)
            .and_then(|value| value.as_str().parse::<usize>().ok())
            .unwrap_or(0);
        let failed_cases = captures
            .get(3)
            .and_then(|value| value.as_str().parse::<usize>().ok())
            .unwrap_or(0);
        summary.insert("passed_cases".to_string(), json!(passed_cases));
        summary.insert("failed_cases".to_string(), json!(failed_cases));
        summary.insert(
            "total_cases".to_string(),
            json!(passed_cases + failed_cases),
        );
    }

    Value::Object(summary)
}

#[cfg(test)]
mod tests {
    use super::{
        StructuralSearchRequest, StructuralWorkflow, build_query_result,
        execute_structural_search, format_ast_grep_failure, normalize_match,
        preflight_parseable_pattern, sanitize_pattern_for_tree_sitter,
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
    fn build_query_result_truncates_matches() {
        let result = build_query_result(
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
    fn structural_request_defaults_workflow_to_query() {
        let request = StructuralSearchRequest::from_args(&json!({
            "action": "structural",
            "pattern": "fn $NAME() {}"
        }))
        .expect("valid request");

        assert_eq!(request.workflow, StructuralWorkflow::Query);
    }

    #[test]
    fn structural_request_requires_pattern_for_query() {
        let err = StructuralSearchRequest::from_args(&json!({
            "action": "structural",
            "pattern": "   "
        }))
        .expect_err("pattern required");

        assert!(err.to_string().contains("requires a non-empty `pattern`"));
    }

    #[test]
    fn structural_request_allows_scan_without_pattern() {
        let request = StructuralSearchRequest::from_args(&json!({
            "action": "structural",
            "workflow": "scan"
        }))
        .expect("scan should not require a pattern");

        assert_eq!(request.workflow, StructuralWorkflow::Scan);
        assert_eq!(request.requested_path(), ".");
        assert_eq!(request.requested_config_path(), "sgconfig.yml");
    }

    #[test]
    fn structural_request_allows_test_without_pattern() {
        let request = StructuralSearchRequest::from_args(&json!({
            "action": "structural",
            "workflow": "test"
        }))
        .expect("test should not require a pattern");

        assert_eq!(request.workflow, StructuralWorkflow::Test);
        assert_eq!(request.requested_config_path(), "sgconfig.yml");
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
        assert!(err.to_string().contains("bundled `ast-grep` skill"));
    }

    #[test]
    fn structural_request_rejects_query_only_fields_for_scan() {
        let err = StructuralSearchRequest::from_args(&json!({
            "action": "structural",
            "workflow": "scan",
            "lang": "rust"
        }))
        .expect_err("scan rejects query-only fields");

        assert!(err.to_string().contains("workflow='scan'"));
        assert!(err.to_string().contains("does not accept `lang`"));
    }

    #[test]
    fn structural_request_rejects_scan_only_fields_for_query() {
        let err = StructuralSearchRequest::from_args(&json!({
            "action": "structural",
            "pattern": "fn $NAME() {}",
            "config_path": "sgconfig.yml"
        }))
        .expect_err("query rejects config path");

        assert!(err.to_string().contains("workflow='query'"));
        assert!(err.to_string().contains("does not accept `config_path`"));
    }

    #[test]
    fn structural_request_rejects_query_only_fields_for_test() {
        let err = StructuralSearchRequest::from_args(&json!({
            "action": "structural",
            "workflow": "test",
            "selector": "function_item"
        }))
        .expect_err("test rejects query-only fields");

        assert!(err.to_string().contains("workflow='test'"));
        assert!(err.to_string().contains("does not accept `selector`"));
    }

    #[test]
    fn structural_request_rejects_scan_only_fields_for_test() {
        let err = StructuralSearchRequest::from_args(&json!({
            "action": "structural",
            "workflow": "test",
            "globs": ["**/*.rs"]
        }))
        .expect_err("test rejects globs");

        assert!(err.to_string().contains("workflow='test'"));
        assert!(err.to_string().contains("does not accept `globs`"));
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
        assert!(text.contains("bundled `ast-grep` skill"));
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

    #[tokio::test]
    #[serial]
    async fn structural_scan_normalizes_findings_and_truncates() {
        let temp = TempDir::new().expect("workspace tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("create src");
        fs::write(temp.path().join("sgconfig.yml"), "ruleDirs: []\n").expect("write config");
        let args_path = temp.path().join("sg_args.txt");
        let script = format!(
            "#!/bin/sh\nprintf '%s\n' \"$@\" > \"{}\"\nprintf '%s\n' '{}' '{}'\n",
            args_path.display(),
            r#"{"text":"items.iter().for_each(handle);","range":{"start":{"line":5,"column":4},"end":{"line":5,"column":29}},"file":"src/lib.rs","lines":"5: items.iter().for_each(handle);","language":"Rust","ruleId":"no-iterator-for-each","severity":"warning","message":"Prefer a for loop.","note":"Avoid side-effect-only for_each.","metadata":{"docs":"https://example.com/rule","category":"style"}}"#,
            r#"{"text":"items.into_iter().for_each(handle);","range":{"start":{"line":9,"column":0},"end":{"line":9,"column":34}},"file":"src/main.rs","lines":"9: items.into_iter().for_each(handle);","language":"Rust","ruleId":"no-iterator-for-each","severity":"warning","message":"Prefer a for loop.","note":null,"metadata":{"docs":"https://example.com/rule","category":"style"}}"#
        );
        let (_script_dir, script_path) = write_fake_sg(&script);

        let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
        let result = execute_structural_search(
            temp.path(),
            json!({
                "action": "structural",
                "workflow": "scan",
                "path": "src",
                "config_path": "sgconfig.yml",
                "filter": "no-iterator-for-each",
                "globs": ["**/*.rs", "!target/**"],
                "context_lines": 2,
                "max_results": 1
            }),
        )
        .await
        .expect("scan should succeed");

        assert_eq!(result["backend"], "ast-grep");
        assert_eq!(result["workflow"], "scan");
        assert_eq!(result["path"], "src");
        assert_eq!(result["config_path"], "sgconfig.yml");
        assert_eq!(result["truncated"], true);

        let findings = result["findings"].as_array().expect("findings");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0]["file"], "src/lib.rs");
        assert_eq!(findings[0]["line_number"], 5);
        assert_eq!(findings[0]["language"], "Rust");
        assert_eq!(findings[0]["rule_id"], "no-iterator-for-each");
        assert_eq!(findings[0]["severity"], "warning");
        assert_eq!(findings[0]["message"], "Prefer a for loop.");
        assert_eq!(findings[0]["note"], "Avoid side-effect-only for_each.");
        assert_eq!(findings[0]["metadata"]["category"], "style");
        assert_eq!(result["summary"]["total_findings"], 2);
        assert_eq!(result["summary"]["returned_findings"], 1);
        assert_eq!(result["summary"]["by_rule"]["no-iterator-for-each"], 2);
        assert_eq!(result["summary"]["by_severity"]["warning"], 2);

        let args = fs::read_to_string(args_path).expect("read sg args");
        assert!(args.lines().any(|line| line == "scan"));
        assert!(args.lines().any(|line| line == "--config"));
        assert!(args.lines().any(|line| line == "sgconfig.yml"));
        assert!(args.lines().any(|line| line == "--filter"));
        assert!(args.lines().any(|line| line == "no-iterator-for-each"));
        assert!(args.lines().any(|line| line == "--globs"));
        assert!(args.lines().any(|line| line == "--context"));
        assert!(args.lines().any(|line| line == "2"));
        assert!(args.lines().any(|line| line == "src"));
    }

    #[tokio::test]
    #[serial]
    async fn structural_test_returns_stdout_stderr_and_summary() {
        let temp = TempDir::new().expect("workspace tempdir");
        fs::create_dir_all(temp.path().join("config")).expect("create config dir");
        fs::write(
            temp.path().join("config/sgconfig.yml"),
            "ruleDirs: []\n",
        )
        .expect("write config");
        let args_path = temp.path().join("sg_args.txt");
        let script = format!(
            "#!/bin/sh\nprintf '%s\n' \"$@\" > \"{}\"\nprintf '\\033[32mRunning 2 tests\\033[0m\nPASS rust/no-iterator-for-each\nFAIL rust/for-each-snapshot\ntest result: failed. 1 passed; 1 failed;\n'\nprintf 'snapshot mismatch\n' >&2\nexit 1\n",
            args_path.display(),
        );
        let (_script_dir, script_path) = write_fake_sg(&script);

        let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
        let result = execute_structural_search(
            temp.path(),
            json!({
                "action": "structural",
                "workflow": "test",
                "config_path": "config/sgconfig.yml",
                "filter": "rust/no-iterator-for-each",
                "skip_snapshot_tests": true
            }),
        )
        .await
        .expect("test workflow should return structured result");

        assert_eq!(result["backend"], "ast-grep");
        assert_eq!(result["workflow"], "test");
        assert_eq!(result["config_path"], "config/sgconfig.yml");
        assert_eq!(result["passed"], false);
        assert!(result["stdout"]
            .as_str()
            .expect("stdout")
            .contains("Running 2 tests"));
        assert!(result["stderr"]
            .as_str()
            .expect("stderr")
            .contains("snapshot mismatch"));
        assert_eq!(result["summary"]["status"], "failed");
        assert_eq!(result["summary"]["passed_cases"], 1);
        assert_eq!(result["summary"]["failed_cases"], 1);
        assert_eq!(result["summary"]["total_cases"], 2);

        let args = fs::read_to_string(args_path).expect("read sg args");
        assert!(args.lines().any(|line| line == "test"));
        assert!(args.lines().any(|line| line == "--config"));
        assert!(args.lines().any(|line| line == "config/sgconfig.yml"));
        assert!(args.lines().any(|line| line == "--filter"));
        assert!(args.lines().any(|line| line == "rust/no-iterator-for-each"));
        assert!(args.lines().any(|line| line == "--skip-snapshot-tests"));
    }
}
