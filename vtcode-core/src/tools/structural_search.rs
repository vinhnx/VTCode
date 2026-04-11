use anyhow::{Context, Result, anyhow, bail};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use tokio::process::Command;

use crate::tools::ast_grep_binary::AST_GREP_INSTALL_COMMAND;
use crate::tools::ast_grep_language::AstGrepLanguage;
use crate::tools::editing::patch::resolve_ast_grep_binary_path;
use crate::tools::tree_sitter_runtime::parse_source;
use crate::utils::path::{canonicalize_allow_missing, normalize_path, resolve_workspace_path};

const DEFAULT_MAX_RESULTS: usize = 100;
const MAX_ALLOWED_RESULTS: usize = 10_000;
const MAX_ALLOWED_GLOBS: usize = 64;
const MAX_ALLOWED_CONTEXT_LINES: usize = 20;
const MAX_AUXILIARY_OUTPUT_CHARS: usize = 64_000;
const DEFAULT_AST_GREP_CONFIG_PATH: &str = "sgconfig.yml";
const AST_GREP_FAQ_HINT: &str = "Hints: patterns must be valid parseable code for the selected language; ast-grep matches CST structure, not raw text; if the target is only a fragment, retry with a larger parseable pattern and use `selector` when the real match is a subnode inside that pattern; invalid snippets may appear to work only through tree-sitter recovery, so prefer valid `context` plus `selector` instead of relying on recovery; do not try to force a different node kind by combining separate `kind` and `pattern` rules; use one pattern object with `context` plus `selector` instead; operators and keywords usually are not valid meta-variable positions, so switch to parseable code plus `kind`, `regex`, `has`, or another rule object; `$VAR` matches named nodes by default, `$$VAR` includes unnamed nodes, and `$$$ARGS` matches zero or more nodes lazily; meta variables are only detected when the whole AST node text matches meta-variable syntax, so mixed text or lowercase names will not work; repeat captured names only when the syntax must match exactly, and prefix with `_` to disable capture when equality is not required; if a name must match by prefix or suffix, capture the whole node and narrow it with `constraints.regex` instead of mixing text into the meta variable; if node role matters, make it explicit in the parseable pattern instead of guessing; `selector` can also override the default effective node when statement-level matching matters more than the inner expression; if matches are too broad or too narrow, tune `strictness` (`smart` default; `cst`, `ast`, `relaxed`, and `signature` control what matching may skip); use `debug_query` to inspect parse output when matching is surprising; structural search is syntax-aware, not scope/type/data-flow analysis.";
const AST_GREP_PROJECT_CONFIG_HINT: &str = "If the target language is not built into ast-grep, register it in workspace-local `sgconfig.yml` under `customLanguages` with a compiled tree-sitter dynamic library. Prefer `tree-sitter build --output <lib>` to compile it, or use `TREE_SITTER_LIBDIR` with `tree-sitter test` on older tree-sitter versions. Reusing a compatible parser library from Neovim is also valid. If the parser exists but the extension is unusual, map it with `languageGlobs`. Some embedded-language cases are built in, such as HTML `<script>` / `<style>` extraction. If the target syntax is embedded inside another host language, configure `languageInjections` with `hostLanguage`, `rule`, and `injected`; the rule should capture the embedded subregion with a meta variable like `$CONTENT`. If `$VAR` is not valid syntax for that language, use its configured `expandoChar` instead. Use `tree-sitter parse <file>` to inspect parser output when the grammar or file association is unclear. ast-grep rules are single-language, so shared JS/TS-style coverage usually means parsing both through the superset via `languageGlobs` or maintaining separate rules.";
const DEBUG_QUERY_LANG_HINT: &str = "action='structural' requires an effective `lang` when `debug_query` is set. Inference only works for unambiguous file paths or single-language positive globs; narrow `path`, add a single-language glob, or set `lang` explicitly";
const STRUCTURAL_FORBIDDEN_KEYS: &[&str] = &[
    "rewrite",
    "interactive",
    "update_all",
    "stdin",
    "json",
    "color",
    "heading",
    "threads",
    "inspect",
    "follow",
    "no_ignore",
    "before",
    "after",
    "include_metadata",
    "test_dir",
    "snapshot_dir",
    "include_off",
    "format",
    "report_style",
    "error",
    "warning",
    "info",
    "hint",
    "off",
    "rule",
    "inline_rules",
    "config",
    "yes",
    "base_dir",
];
static AST_GREP_METAVARIABLE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\$\$?[A-Za-z_][A-Za-z0-9_]*").expect("ast-grep metavariable regex must compile")
});
static ANSI_ESCAPE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").expect("ansi escape regex must compile"));
static AST_GREP_TEST_RESULT_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"test result:\s*(ok|failed)\.\s*(\d+)\s+passed;\s*(\d+)\s+failed;")
        .expect("ast-grep test summary regex must compile")
});

#[cfg(test)]
mod tests;

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
        self.validate_limits()?;

        match self.workflow {
            StructuralWorkflow::Query => self.validate_query(),
            StructuralWorkflow::Scan => self.validate_scan(),
            StructuralWorkflow::Test => self.validate_test(),
        }
    }

    fn validate_limits(&self) -> Result<()> {
        let glob_count = self.normalized_globs().len();
        if glob_count > MAX_ALLOWED_GLOBS {
            bail!(
                "action='structural' accepts at most {} non-empty `globs` entries",
                MAX_ALLOWED_GLOBS
            );
        }

        if let Some(context_lines) = self.context_lines
            && context_lines > MAX_ALLOWED_CONTEXT_LINES
        {
            bail!(
                "action='structural' accepts at most {} `context_lines`",
                MAX_ALLOWED_CONTEXT_LINES
            );
        }

        Ok(())
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

    fn normalized_globs(&self) -> Vec<String> {
        self.globs
            .clone()
            .map(GlobInput::into_vec)
            .unwrap_or_default()
            .into_iter()
            .map(|glob| glob.trim().to_string())
            .filter(|glob| !glob.is_empty())
            .collect()
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
        StructuralWorkflow::Query => {
            execute_structural_query(workspace_root, &request, &ast_grep).await
        }
        StructuralWorkflow::Scan => {
            execute_structural_scan(workspace_root, &request, &ast_grep).await
        }
        StructuralWorkflow::Test => {
            execute_structural_test(workspace_root, &request, &ast_grep).await
        }
    }
}

async fn execute_structural_query(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
) -> Result<Value> {
    let search_path = resolve_search_path(workspace_root, request.requested_path())?;
    let globs = request.normalized_globs();
    if let Some(hint) = preflight_parseable_pattern(request)? {
        return Ok(build_fragment_result(
            request,
            &search_path.display_path,
            hint,
        ));
    }
    let command_path = search_path.command_arg.clone();

    if let Some(debug_query) = &request.debug_query {
        let mut command = ast_grep_command(ast_grep, workspace_root, "run");
        command
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

        let output =
            run_ast_grep_command(&mut command, "failed to run ast-grep debug query").await?;

        if !output.status.success() {
            bail!(
                "{}",
                format_ast_grep_failure(
                    "ast-grep debug query failed",
                    stderr_or_stdout(&output.stderr, &output.stdout)
                )
            );
        }

        return Ok(build_debug_query_result(
            request,
            &search_path.display_path,
            debug_query,
            &output.stdout,
        ));
    }

    let mut command = ast_grep_command(ast_grep, workspace_root, "run");
    command
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
    apply_context_and_globs(&mut command, request.context_lines, &globs);
    command.arg(&command_path);

    let output =
        run_ast_grep_command(&mut command, "failed to run ast-grep structural search").await?;

    let no_matches = output.status.code() == Some(1);
    if !output.status.success() && !no_matches {
        bail!(
            "{}",
            format_ast_grep_failure(
                "ast-grep structural search failed",
                stderr_or_stdout(&output.stderr, &output.stdout)
            )
        );
    }

    let matches = if no_matches && String::from_utf8_lossy(&output.stdout).trim().is_empty() {
        Vec::new()
    } else {
        parse_compact_matches(&output.stdout)?
    };
    Ok(build_query_result(
        request,
        &search_path.display_path,
        matches,
    ))
}

async fn execute_structural_scan(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
) -> Result<Value> {
    let search_path = resolve_search_path(workspace_root, request.requested_path())?;
    let config_path = resolve_config_path(workspace_root, request.requested_config_path()).await?;
    let globs = request.normalized_globs();

    let mut command = ast_grep_command(ast_grep, workspace_root, "scan");
    command
        .arg("--config")
        .arg(&config_path.command_arg)
        .arg("--json=stream")
        .arg("--include-metadata")
        .arg("--color=never");

    if let Some(filter) = request.filter() {
        command.arg("--filter").arg(filter);
    }
    apply_context_and_globs(&mut command, request.context_lines, &globs);
    command.arg(&search_path.command_arg);

    let output =
        run_ast_grep_command(&mut command, "failed to run ast-grep structural scan").await?;

    let findings_with_error_exit = output.status.code() == Some(1);
    if !output.status.success() && !findings_with_error_exit {
        bail!(
            "{}",
            format_ast_grep_failure(
                "ast-grep structural scan failed",
                stderr_or_stdout(&output.stderr, &output.stdout)
            )
        );
    }

    let findings =
        if findings_with_error_exit && String::from_utf8_lossy(&output.stdout).trim().is_empty() {
            Vec::new()
        } else {
            parse_stream_findings(&output.stdout)?
        };
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

    let mut command = ast_grep_command(ast_grep, workspace_root, "test");
    command.arg("--config").arg(&config_path.command_arg);

    if let Some(filter) = request.filter() {
        command.arg("--filter").arg(filter);
    }
    if request.skip_snapshot_tests == Some(true) {
        command.arg("--skip-snapshot-tests");
    }

    let output =
        run_ast_grep_command(&mut command, "failed to run ast-grep structural test").await?;

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

    let (sanitized_pattern, contains_metavariables) =
        sanitize_pattern_for_tree_sitter(request.pattern().expect("query pattern validated"));
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
        if has_argument_key(object, key) {
            bail!(
                "action='structural' is read-only; remove `{}`. For `sg scan`, `sg test`, `sg new`, `sgconfig.yml`, or rewrite-oriented ast-grep tasks, load the bundled `ast-grep` skill first and use `unified_exec` only when the public structural surface cannot express the needed CLI flow.",
                key
            );
        }
    }

    Ok(())
}

fn has_argument_key(object: &Map<String, Value>, key: &str) -> bool {
    object.get(key).is_some()
        || key
            .contains('_')
            .then(|| key.replace('_', "-"))
            .as_ref()
            .is_some_and(|hyphenated| object.get(hyphenated).is_some())
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

fn ast_grep_command(ast_grep: &Path, workspace_root: &Path, subcommand: &str) -> Command {
    let mut command = Command::new(ast_grep);
    command.current_dir(workspace_root).arg(subcommand);
    command
}

async fn run_ast_grep_command(
    command: &mut Command,
    context: &str,
) -> Result<std::process::Output> {
    command.output().await.with_context(|| context.to_string())
}

fn apply_context_and_globs(command: &mut Command, context_lines: Option<usize>, globs: &[String]) {
    if let Some(context_lines) = context_lines {
        command.arg("--context").arg(context_lines.to_string());
    }
    for glob in globs {
        command.arg("--globs").arg(glob);
    }
}

fn build_debug_query_result(
    request: &StructuralSearchRequest,
    display_path: &str,
    debug_query: &DebugQueryFormat,
    stdout: &[u8],
) -> Value {
    json!({
        "backend": "ast-grep",
        "pattern": request.pattern().expect("query pattern validated"),
        "path": display_path,
        "lang": request.lang,
        "debug_query": debug_query.as_str(),
        "debug_query_output": truncate_auxiliary_output(String::from_utf8_lossy(stdout).trim()),
        "matches": [],
        "truncated": false,
    })
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
    let next_action = "Retry `unified_search` with `action='structural'` using a larger parseable pattern and `selector` when the real target is a subnode inside that pattern. Do not rerun the same fragment unchanged.";

    json!({
        "backend": "ast-grep",
        "pattern": request.pattern().expect("query pattern validated"),
        "path": display_path,
        "matches": [],
        "truncated": false,
        "is_recoverable": true,
        "next_action": next_action,
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
        let severity = finding.severity.as_deref().unwrap_or("unknown").to_string();
        *by_severity.entry(severity).or_insert(0usize) += 1;

        let rule = finding.rule_id.as_deref().unwrap_or("unknown").to_string();
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
    let stdout = truncate_auxiliary_output(&String::from_utf8_lossy(stdout));
    let stderr = truncate_auxiliary_output(&String::from_utf8_lossy(stderr));

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

fn truncate_auxiliary_output(text: &str) -> String {
    let char_count = text.chars().count();
    if char_count <= MAX_AUXILIARY_OUTPUT_CHARS {
        return text.to_string();
    }

    let truncated = text
        .chars()
        .take(MAX_AUXILIARY_OUTPUT_CHARS)
        .collect::<String>();
    format!(
        "{truncated}\n...[truncated {} chars]",
        char_count - MAX_AUXILIARY_OUTPUT_CHARS
    )
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

fn build_resolved_workspace_path(
    workspace_root: &Path,
    resolved: PathBuf,
) -> Result<ResolvedSearchPath> {
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

fn resolve_search_path(workspace_root: &Path, requested_path: &str) -> Result<ResolvedSearchPath> {
    let requested = PathBuf::from(requested_path);
    let resolved = resolve_workspace_path(workspace_root, requested.as_path())
        .or_else(|original_error| {
            let Some(remapped) =
                remap_legacy_crates_search_path(workspace_root, requested.as_path())
            else {
                return Err(original_error);
            };
            resolve_workspace_path(workspace_root, remapped.as_path()).with_context(|| {
                format!("Failed to resolve structural search path: {requested_path}")
            })
        })
        .with_context(|| format!("Failed to resolve structural search path: {requested_path}"))?;

    build_resolved_workspace_path(workspace_root, resolved)
}

fn remap_legacy_crates_search_path(
    workspace_root: &Path,
    requested_path: &Path,
) -> Option<PathBuf> {
    let relative = if requested_path.is_absolute() {
        requested_path.strip_prefix(workspace_root).ok()?
    } else {
        requested_path
    };

    let mut components = relative.components();
    match components.next()? {
        Component::Normal(component) if component == "crates" => {}
        _ => return None,
    }

    let remapped: PathBuf = components.collect();
    if remapped.as_os_str().is_empty() {
        return None;
    }

    workspace_root.join(&remapped).exists().then_some(remapped)
}

async fn resolve_config_path(
    workspace_root: &Path,
    requested_path: &str,
) -> Result<ResolvedSearchPath> {
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

    build_resolved_workspace_path(&workspace_root, resolved)
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
