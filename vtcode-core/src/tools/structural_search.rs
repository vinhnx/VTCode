use anyhow::{Context, Result, anyhow, bail};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Component, Path, PathBuf};
use tokio::fs as afs;
use tokio::process::Command;

use crate::tools::ast_grep_binary::AST_GREP_INSTALL_COMMAND;
use crate::tools::ast_grep_language::AstGrepLanguage;
use crate::tools::editing::patch::resolve_ast_grep_binary_path;
use crate::tools::error_helpers::deserialize_tool_args;
use crate::tools::tree_sitter_runtime::parse_source;
use crate::utils::path::{canonicalize_allow_missing, normalize_path, resolve_workspace_path};

const DEFAULT_MAX_RESULTS: usize = 100;
const MAX_ALLOWED_RESULTS: usize = 10_000;
const MAX_ALLOWED_GLOBS: usize = 64;
const MAX_ALLOWED_CONTEXT_LINES: usize = 20;
const MAX_AUXILIARY_OUTPUT_CHARS: usize = 64_000;
const DEFAULT_AST_GREP_CONFIG_PATH: &str = "sgconfig.yml";
const AST_GREP_FAQ_HINT: &str = "Hints: patterns must be valid parseable code for the selected language; ast-grep matches CST structure, not raw text; if the target is only a fragment, retry with a larger parseable pattern and use `selector` when the real match is a subnode inside that pattern; invalid snippets may appear to work only through tree-sitter recovery, so prefer valid `context` plus `selector` instead of relying on recovery; for C, tree-sitter-c parses fragments differently by context: `test($A)` alone becomes `macro_type_specifier`, while `test($A);` becomes `expression_statement -> call_expression`; use `context` plus `selector: call_expression` for C function-call matching; do not try to force a different node kind by combining separate `kind` and `pattern` rules; use one pattern object with `context` plus `selector` instead; operators and keywords usually are not valid meta-variable positions, so switch to parseable code plus `kind`, `regex`, `has`, or another rule object; `$VAR` matches named nodes by default, `$$VAR` includes unnamed nodes, and `$$$ARGS` matches zero or more nodes lazily; `$_NAME` prefix means non-capturing (no backreference); same-name metavariables enforce identity (`$A == $A` matches `a == a` but not `a == b`); meta variables are only detected when the whole AST node text matches meta-variable syntax, so mixed text, lowercase names, or bare `$` followed by digits will not work; repeat captured names only when the syntax must match exactly, and prefix with `_` to disable capture when equality is not required; if a name must match by prefix or suffix, capture the whole node and narrow it with `constraints.regex` instead of mixing text into the meta variable; if node role matters, make it explicit in the parseable pattern instead of guessing; `selector` can also override the default effective node when statement-level matching matters more than the inner expression; if matches are too broad or too narrow, tune `strictness` (`smart` default; `cst`, `ast`, `relaxed`, and `signature` control what matching may skip); use `debug_query` to inspect parse output when matching is surprising; structural search is syntax-aware, not scope/type/data-flow analysis; `kind` supports ESQuery-style compound selectors: `A > B` (direct child), `A B` (descendant), `A + B` (immediate sibling), `A ~ B` (general sibling), and `A, B` (either); pseudo-selectors `:has()`, `:not()`, `:is()`, `:nth-child()`, and `:nth-last-child()` narrow `kind` and `selector` matches by descendant structure, exclusion, alternatives, or sibling position; for HTML, key node kinds are `element`, `tag_name`, `attribute_name`, `attribute_value`, and `text`; use `kind: element` with `has` to match elements by tag or attribute, `kind: tag_name` to match tag names, `kind: attribute_name` to match attribute names, and `kind: text` to match text content; HTML `inside` with `stopBy: { kind: element }` scopes matches to the nearest enclosing element; HTML `<script>` and `<style>` content is parsed as embedded JavaScript/CSS respectively, so search those regions with `lang: javascript` or `lang: css` rules; for simple pattern-to-pattern rewrites, use `workflow='rewrite'` which previews replacements without applying them; use `workflow='apply'` to write rewrite results directly to disk; for FixConfig rewrites with range expansion via `expandStart`/`expandEnd`, use `workflow='rewrite'` with `fix_config` which generates a temporary YAML rule and previews the expanded replacements; for advanced rewrite operations using `transform` (replace for regex substitution with capture groups, substring for Python-style Unicode slicing, convert for identifier case changes like camelCase/snakeCase/kebabCase/pascalCase), `fix`, `rewriters`, load the bundled `ast-grep` skill which covers the full transform pipeline including regex capture groups, chained sequential transformations, conditional separators from multi-captures, and string-form shorthand syntax; use `matches` to reference a utility rule by name; define local utilities in the `utils` section of the request; composite rules `all` (conjunction), `any` (disjunction), and `not` (negation) combine sub-rules; use `matches` with `utils` for recursive pattern matching; cyclic `matches` dependencies are not allowed; for `matches` and composite rules, `lang` is required because the YAML rule generation path is used; to exclude files from the search, use the `exclude` parameter (e.g. `exclude: ['*.md', 'tests/**']`) instead of passing `--exclude` directly to ast-grep, which is not a valid CLI flag; `exclude` entries are converted to negative `--globs` patterns automatically.";
const AST_GREP_PROJECT_CONFIG_HINT: &str = "If the target language is not built into ast-grep, register it in workspace-local `sgconfig.yml` under `customLanguages` with a compiled tree-sitter dynamic library. Prefer `tree-sitter build --output <lib>` to compile it, or use `TREE_SITTER_LIBDIR` with `tree-sitter test` on older tree-sitter versions. Reusing a compatible parser library from Neovim is also valid. If the parser exists but the extension is unusual, map it with `languageGlobs`. Some embedded-language cases are built in, such as HTML `<script>` / `<style>` extraction. If the target syntax is embedded inside another host language, configure `languageInjections` with `hostLanguage`, `rule`, and `injected`; the rule should capture the embedded subregion with a meta variable like `$CONTENT`. If `$VAR` is not valid syntax for that language, use its configured `expandoChar` instead. Use `tree-sitter parse <file>` to inspect parser output when the grammar or file association is unclear. ast-grep rules are single-language, so shared JS/TS-style coverage usually means parsing both through the superset via `languageGlobs` or maintaining separate rules. Use `testConfigs` with `testDir` (required) and optional `snapshotDir` to configure ast-grep test discovery. Use `utilDirs` to declare directories for global utility rules shared across multiple rule files. Use `workflow='inspect'` to see the project's current `testConfigs`, `utilDirs`, `languageInjections`, `customLanguages`, and `languageGlobs` configuration. Utility rules must declare `id` and `language` and can only use `id`, `language`, `rule`, `constraints`, and local `utils`.";
const DEBUG_QUERY_LANG_HINT: &str = "action='structural' requires an effective `lang` when `debug_query` is set. Inference only works for unambiguous file paths or single-language positive globs; narrow `path`, add a single-language glob, or set `lang` explicitly";
const STRUCTURAL_FORBIDDEN_KEYS: &[&str] = &[
    "stdin",
    "json",
    "color",
    "heading",
    "inspect",
    "include_metadata",
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

const VALID_NO_IGNORE_VALUES: &[&str] = &["hidden", "dot", "exclude", "global", "parent", "vcs"];
const VALID_FORMAT_VALUES: &[&str] = &["github", "sarif"];
const VALID_REPORT_STYLE_VALUES: &[&str] = &["rich", "medium", "short"];
const VALID_BUILTIN_RULES: &[&str] = &["unused-suppression", "no-suppress-all"];
const MAX_THREADS: u32 = 256;
static AST_GREP_METAVARIABLE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\$\$?\$?[A-Za-z_][A-Za-z0-9_]*").expect("ast-grep metavariable regex must compile")
});
/// Valid ast-grep metavariable: `$` or `$$` followed by uppercase/startunderscore,
/// then uppercase/digits/underscores. Multi-metavariable `$$$` is also valid.
static AST_GREP_VALID_METAVAR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\$\$?(\$?[A-Z_][A-Z0-9_]*)$").expect("ast-grep valid metavar regex must compile")
});
static ANSI_ESCAPE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").expect("ansi escape regex must compile"));
static AST_GREP_TEST_RESULT_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"test result:\s*(ok|failed)\.\s*(\d+)\s+passed;\s*(\d+)\s+failed;")
        .expect("ast-grep test summary regex must compile")
});
/// Matches per-rule result lines: `PASS rule-id` or `FAIL rule-id` with
/// optional trailing dots and N/M markers (e.g. `FAIL rust/foo ...N..M`).
static AST_GREP_TEST_RULE_LINE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(PASS|FAIL)\s+(\S[\w/\-]*)(.*)$")
        .expect("ast-grep test rule line regex must compile")
});
/// Matches failure detail blocks: `[Noisy]` or `[Missing]` headers.
static AST_GREP_TEST_NOISY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\[Noisy\]\s+Expect\s+(\S[\w/\-]*)\s+to report no issue")
        .expect("ast-grep noisy detail regex must compile")
});
static AST_GREP_TEST_MISSING_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\[Missing\]\s+Expect\s+(?:rule\s+)?(\S[\w/\-]*)\s+to report issues")
        .expect("ast-grep missing detail regex must compile")
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
    Inspect,
    Rewrite,
    Count,
    Rules,
    New,
    Apply,
}

impl StructuralWorkflow {
    fn as_str(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Scan => "scan",
            Self::Test => "test",
            Self::Inspect => "inspect",
            Self::Rewrite => "rewrite",
            Self::Count => "count",
            Self::Rules => "rules",
            Self::New => "new",
            Self::Apply => "apply",
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

/// Accepted forms for the `nth_child` field: a plain number, an An+B
/// formula string, or a full object with `position`, optional `reverse`,
/// and optional `ofRule`.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum NthChildInput {
    Number(usize),
    Formula(String),
    Object(NthChildObject),
}

#[derive(Debug, Clone, Deserialize)]
struct NthChildObject {
    position: Value,
    #[serde(default)]
    reverse: Option<bool>,
    #[serde(default, rename = "ofRule")]
    of_rule: Option<Value>,
}

/// A source-range constraint with 0-based line/column positions.
/// `start` is inclusive, `end` is exclusive.
#[derive(Debug, Clone, Deserialize)]
struct RangeInput {
    start: RangePoint,
    end: RangePoint,
}

#[derive(Debug, Clone, Deserialize)]
struct RangePoint {
    line: usize,
    column: usize,
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

/// A rule object used in `expandStart` / `expandEnd` of a `FixConfig`.
/// Supports the common rule forms: `regex`, `kind`, `pattern`, plus the
/// optional `stopBy` field unique to expand rules.
#[derive(Debug, Clone, Deserialize)]
struct FixExpandRule {
    #[serde(default)]
    regex: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    pattern: Option<String>,
    /// Controls where the expansion stops. Defaults to `"end"` (expand to
    /// the end of the enclosing node). Set to `"line"` to stop at end of
    /// line, or a rule object to stop at a specific sibling.
    #[serde(default)]
    stop_by: Option<Value>,
}

impl FixExpandRule {
    fn is_empty(&self) -> bool {
        self.regex.is_none() && self.kind.is_none() && self.pattern.is_none()
    }

    fn validate(&self, label: &str) -> Result<()> {
        if self.is_empty() {
            bail!("`{label}` must specify at least one of `regex`, `kind`, or `pattern`");
        }
        Ok(())
    }

    /// Serialize this expand rule to a YAML-compatible JSON value for rule
    /// file generation.
    fn to_yaml_value(&self) -> Value {
        let mut obj = Map::new();
        if let Some(regex) = &self.regex {
            obj.insert("regex".to_string(), Value::String(regex.clone()));
        }
        if let Some(kind) = &self.kind {
            obj.insert("kind".to_string(), Value::String(kind.clone()));
        }
        if let Some(pattern) = &self.pattern {
            obj.insert("pattern".to_string(), Value::String(pattern.clone()));
        }
        if let Some(stop_by) = &self.stop_by {
            obj.insert("stopBy".to_string(), stop_by.clone());
        }
        Value::Object(obj)
    }
}

/// Advanced fix configuration that allows expanding the replacement range
/// beyond the matched AST node. This maps to ast-grep's `FixConfig` YAML
/// rule feature.
///
/// Use `FixConfig` when replacing only the matched node is not enough,
/// especially for deleting list items or key-value pairs that also need
/// a surrounding comma removed.
#[derive(Debug, Clone, Deserialize)]
struct FixConfig {
    /// The replacement template string. Meta variables from the matched
    /// pattern can be referenced here (e.g. `$VAR`, `$$$ARGS`).
    template: String,
    /// Optional rule to expand the fix range start backwards. The range
    /// start moves left until the rule is no longer met.
    #[serde(default)]
    expand_start: Option<FixExpandRule>,
    /// Optional rule to expand the fix range end forwards. The range end
    /// moves right until the rule is no longer met.
    #[serde(default)]
    expand_end: Option<FixExpandRule>,
}

impl FixConfig {
    fn validate(&self) -> Result<()> {
        // Template can be empty for "delete" operations (replace matched
        // node with nothing). Validation ensures the field is present.
        if let Some(expand_start) = &self.expand_start {
            expand_start.validate("fix_config.expand_start")?;
        }
        if let Some(expand_end) = &self.expand_end {
            expand_end.validate("fix_config.expand_end")?;
        }
        Ok(())
    }

    fn has_expansion(&self) -> bool {
        self.expand_start.is_some() || self.expand_end.is_some()
    }
}

#[derive(Debug, Clone, Deserialize)]
struct StructuralSearchRequest {
    #[serde(default)]
    workflow: StructuralWorkflow,
    #[serde(default)]
    pattern: Option<String>,
    #[serde(default)]
    kind: Option<String>,
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
    /// Glob patterns to exclude from the search. Each entry is a glob
    /// pattern (e.g. `"*.md"`, `"tests/**"`). These are converted to
    /// negative `--globs` flags (`!pattern`) for ast-grep. Can be a
    /// single string or an array of strings.
    #[serde(default, alias = "exclude")]
    exclude: Option<GlobInput>,
    #[serde(default)]
    context_lines: Option<usize>,
    #[serde(default)]
    max_results: Option<usize>,
    #[serde(default)]
    skip_snapshot_tests: Option<bool>,
    /// Update all snapshot files without interactive confirmation.
    /// Only valid for `test` workflow. Passed as `--update-all` to `sg test`.
    #[serde(default)]
    update_all: Option<bool>,
    /// Launch an interactive session to accept/reject snapshot updates.
    /// Only valid for `test` workflow. Passed as `--interactive` to `sg test`.
    #[serde(default)]
    interactive: Option<bool>,
    /// Override the test directory for sg test.
    /// Only valid for `test` workflow. Passed as `--test-dir` to `sg test`.
    #[serde(default)]
    test_dir: Option<String>,
    /// Override the snapshot directory for sg test.
    /// Only valid for `test` workflow. Passed as `--snapshot-dir` to `sg test`.
    #[serde(default)]
    snapshot_dir: Option<String>,
    /// Include `severity: off` rules in test.
    /// Only valid for `test` workflow. Passed as `--include-off` to `sg test`.
    #[serde(default)]
    include_off: Option<bool>,
    #[serde(default)]
    rewrite: Option<String>,
    /// Advanced fix configuration for the rewrite workflow. When present,
    /// the tool generates a temporary YAML rule with `fix` as a `FixConfig`
    /// object (template + expandStart/expandEnd) and runs `sg scan` instead
    /// of `sg run --rewrite`.
    #[serde(default, rename = "fix_config")]
    fix_config: Option<FixConfig>,

    /// Match node text by Rust regex. Passed as `--regex` to the ast-grep
    /// CLI. Requires `lang` to be set. Only valid for `query` and `rewrite`
    /// workflows.
    #[serde(default)]
    regex: Option<String>,

    /// Match by 1-based position among named siblings. Accepts a number,
    /// an An+B formula string, or an object with `position`, optional
    /// `reverse`, and optional `ofRule`. Only valid for `query` workflow;
    /// triggers YAML rule generation.
    #[serde(default, rename = "nth_child")]
    nth_child: Option<NthChildInput>,

    /// Match by source position (0-based line/column, start inclusive, end
    /// exclusive). Only valid for `query` workflow; triggers YAML rule
    /// generation.
    #[serde(default)]
    range: Option<RangeInput>,

    // -- Relational rule fields ------------------------------------------------
    /// Relational: match if a descendant matches this rule.
    #[serde(default)]
    has: Option<Box<Value>>,
    /// Relational: match if an ancestor matches this rule.
    #[serde(default)]
    inside: Option<Box<Value>>,
    /// Relational: match if a preceding sibling matches this rule.
    #[serde(default)]
    follows: Option<Box<Value>>,
    /// Relational: match if a following sibling matches this rule.
    #[serde(default)]
    precedes: Option<Box<Value>>,
    /// Narrow meta-variable matches by additional constraints.
    #[serde(default)]
    constraints: Option<Map<String, Value>>,

    // -- Composite rule fields -------------------------------------------------
    /// Composite: reference a utility rule by name via `matches`.
    #[serde(default)]
    matches: Option<String>,
    /// Composite: all sub-rules must match (conjunction).
    #[serde(default)]
    all: Option<Vec<Value>>,
    /// Composite: any sub-rule must match (disjunction).
    #[serde(default)]
    any: Option<Vec<Value>>,
    /// Composite: the sub-rule must not match (negation).
    #[serde(default)]
    not: Option<Box<Value>>,
    /// Local utility rules defined inline for this query. Each key is a
    /// utility rule id and each value is the rule object.
    #[serde(default)]
    utils: Option<Map<String, Value>>,

    // -- Transform fields ------------------------------------------------------
    /// Transform pipeline for meta-variable substitution. Each key is a
    /// new variable name and each value defines the transform operation
    /// (replace, substring, or convert). Transformed variables can be
    /// referenced in `fix_config.template` via `$$$VAR_NAME`.
    ///
    /// Only valid for `query`, `count`, and `rewrite` workflows that use
    /// YAML rule generation. Requires `lang` to be set.
    #[serde(default)]
    transform: Option<Map<String, Value>>,

    // -- Scan-specific fields ---------------------------------------------------
    /// Post-run severity filter for `scan` workflow. When present, only
    /// findings whose severity matches one of the listed values are returned.
    /// Valid values: `error`, `warning`, `info`, `hint`. This filters the
    /// output after ast-grep runs; it does not override rule severities.
    #[serde(default)]
    severities: Option<Vec<String>>,

    /// Control which ignore files ast-grep respects. Valid values:
    /// `hidden`, `dot`, `exclude`, `global`, `parent`, `vcs`.
    /// Only valid for `scan`, `query`, and `rewrite` workflows.
    #[serde(default, alias = "no-ignore")]
    no_ignore: Option<Vec<String>>,

    /// Follow symbolic links while traversing directories.
    /// Only valid for `scan`, `query`, and `rewrite` workflows.
    #[serde(default)]
    follow: Option<bool>,

    /// Number of threads for ast-grep to use. 0 means auto.
    /// Only valid for `scan` workflow. Max 256.
    #[serde(default)]
    threads: Option<u32>,

    /// Output format for CI pipelines. Valid values: `github`, `sarif`.
    /// Only valid for `scan` workflow. When set, the raw formatted output
    /// is returned instead of the normal JSON stream.
    #[serde(default)]
    format: Option<String>,

    /// Diagnostic report style. Valid values: `rich`, `medium`, `short`.
    /// Only valid for `scan` workflow.
    #[serde(default, alias = "report-style")]
    report_style: Option<String>,

    /// Number of context lines to show before each match. Mutually
    /// exclusive with `context_lines`. Only valid for `query`, `scan`,
    /// and `rewrite` workflows.
    #[serde(default, alias = "before-lines")]
    before_lines: Option<usize>,

    /// Number of context lines to show after each match. Mutually
    /// exclusive with `context_lines`. Only valid for `query`, `scan`,
    /// and `rewrite` workflows.
    #[serde(default, alias = "after-lines")]
    after_lines: Option<usize>,

    /// Built-in ast-grep rules to activate. Valid values:
    /// `unused-suppression`, `no-suppress-all`. Each entry is activated
    /// at the severity specified in the format `"rule-id:severity"`
    /// (e.g. `"unused-suppression:error"`). If no severity is specified,
    /// defaults to the rule's built-in severity.
    /// Only valid for `scan` workflow.
    #[serde(default, alias = "builtin-rules")]
    builtin_rules: Option<Vec<String>>,

    // -- New workflow fields ---------------------------------------------------
    /// Subcommand for `workflow='new'`: `project`, `rule`, `test`, or `util`.
    #[serde(default, rename = "new_subcommand")]
    new_subcommand: Option<String>,

    /// Name of the rule, test, or utility to create.
    /// Required for `new` subcommands `rule`, `test`, and `util`.
    #[serde(default, rename = "new_name")]
    new_name: Option<String>,
}

impl StructuralSearchRequest {
    fn from_args(args: &Value) -> Result<Self> {
        reject_forbidden_args(args)?;

        let mut request: Self = deserialize_tool_args(args, "structural_search")?;
        request.normalize();
        request.validate()?;

        Ok(request)
    }

    fn normalize(&mut self) {
        if self.workflow == StructuralWorkflow::Query
            || self.workflow == StructuralWorkflow::Rewrite
            || self.workflow == StructuralWorkflow::Count
            || self.workflow == StructuralWorkflow::Apply
        {
            self.lang = self.normalized_or_inferred_lang();
        }
    }

    fn validate(&self) -> Result<()> {
        self.validate_limits()?;

        match self.workflow {
            StructuralWorkflow::Query => self.validate_query(),
            StructuralWorkflow::Scan => self.validate_scan(),
            StructuralWorkflow::Test => self.validate_test(),
            StructuralWorkflow::Inspect => self.validate_inspect(),
            StructuralWorkflow::Rewrite => self.validate_rewrite(),
            StructuralWorkflow::Count => self.validate_query(),
            StructuralWorkflow::Rules => self.validate_scan(),
            StructuralWorkflow::New => self.validate_new(),
            StructuralWorkflow::Apply => self.validate_apply(),
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

        // Validate no_ignore values.
        if let Some(no_ignore) = &self.no_ignore {
            for value in no_ignore {
                let normalized = value.trim().to_ascii_lowercase();
                if !VALID_NO_IGNORE_VALUES.contains(&normalized.as_str()) {
                    bail!(
                        "invalid `no_ignore` value `{value}`; expected one of: {}",
                        VALID_NO_IGNORE_VALUES.join(", ")
                    );
                }
            }
        }

        // Validate format value.
        if let Some(fmt) = self.effective_format()
            && !VALID_FORMAT_VALUES.contains(&fmt)
        {
            bail!(
                "invalid `format` value `{}`; expected one of: {}",
                fmt,
                VALID_FORMAT_VALUES.join(", ")
            );
        }

        // Validate report_style value.
        if let Some(style) = self.effective_report_style()
            && !VALID_REPORT_STYLE_VALUES.contains(&style)
        {
            bail!(
                "invalid `report_style` value `{}`; expected one of: {}",
                style,
                VALID_REPORT_STYLE_VALUES.join(", ")
            );
        }

        // Validate builtin_rules values.
        if let Some(rules) = self.effective_builtin_rules() {
            for rule in rules {
                let rule_name = rule.split(':').next().unwrap_or(rule);
                if !VALID_BUILTIN_RULES.contains(&rule_name) {
                    bail!(
                        "invalid builtin rule `{rule_name}`; expected one of: {}",
                        VALID_BUILTIN_RULES.join(", ")
                    );
                }
            }
        }

        // Validate mutual exclusivity of context_lines vs before_lines/after_lines.
        if self.context_lines.is_some()
            && (self.before_lines.is_some() || self.after_lines.is_some())
        {
            bail!(
                "`context_lines` is mutually exclusive with `before_lines` and `after_lines`; use one or the other"
            );
        }

        Ok(())
    }

    fn validate_query(&self) -> Result<()> {
        let has_relational = self.has.is_some()
            || self.inside.is_some()
            || self.follows.is_some()
            || self.precedes.is_some();

        let has_composite = self.matches.is_some() || self.all.is_some() || self.any.is_some();

        if self.pattern().is_none()
            && self.kind().is_none()
            && self.regex_pattern().is_none()
            && self.nth_child.is_none()
            && self.range.is_none()
            && !has_relational
            && !has_composite
            && self.constraints.is_none()
        {
            bail!(
                "action='structural' workflow='query' requires a non-empty `pattern`, `kind`, \
                 `regex`, `nth_child`, `range`, `has`, `inside`, `follows`, `precedes`, \
                 `matches`, `all`, or `any`"
            );
        }

        self.reject_present("config_path", self.config_path.as_deref())?;
        self.reject_present("filter", self.filter.as_deref())?;
        self.reject_flag("skip_snapshot_tests", self.skip_snapshot_tests)?;
        self.reject_flag("update_all", self.update_all)?;
        self.reject_flag("interactive", self.interactive)?;
        self.reject_present("test_dir", self.test_dir.as_deref())?;
        self.reject_present("snapshot_dir", self.snapshot_dir.as_deref())?;
        self.reject_flag("include_off", self.include_off)?;

        if self.debug_query.is_some() && self.lang.as_deref().is_none_or(str::is_empty) {
            bail!(DEBUG_QUERY_LANG_HINT);
        }

        if self.regex_pattern().is_some() && self.lang.as_deref().is_none_or(str::is_empty) {
            bail!("action='structural' with `regex` requires `lang` to be set");
        }

        if has_relational && self.lang.as_deref().is_none_or(str::is_empty) {
            bail!(
                "action='structural' with relational rules (`has`/`inside`/`follows`/`precedes`) requires `lang` to be set"
            );
        }

        if has_composite && self.lang.as_deref().is_none_or(str::is_empty) {
            bail!(
                "action='structural' with composite rules (`matches`/`all`/`any`) requires `lang` to be set"
            );
        }

        if self.transform.is_some() && self.lang.as_deref().is_none_or(str::is_empty) {
            bail!(
                "action='structural' with `transform` requires `lang` to be set because transform \
                 definitions are emitted into YAML rules that target a specific language"
            );
        }

        self.validate_nth_child_position()?;

        Ok(())
    }

    fn validate_nth_child_position(&self) -> Result<()> {
        if let Some(ref nth) = self.nth_child {
            match nth {
                NthChildInput::Number(n) => {
                    if *n == 0 {
                        bail!("`nth_child` position is 1-based; 0 is not valid");
                    }
                }
                NthChildInput::Object(obj) => {
                    if let Some(pos) = obj.position.as_u64()
                        && pos == 0
                    {
                        bail!("`nth_child` position is 1-based; 0 is not valid");
                    }
                }
                NthChildInput::Formula(_) => {
                    // An+B formulas are validated by ast-grep itself.
                }
            }
        }
        Ok(())
    }

    fn validate_scan(&self) -> Result<()> {
        self.reject_present("pattern", self.pattern.as_deref())?;
        self.reject_present("kind", self.kind.as_deref())?;
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
        self.reject_present("regex", self.regex.as_deref())?;
        self.reject_flag("skip_snapshot_tests", self.skip_snapshot_tests)?;
        self.reject_flag("update_all", self.update_all)?;
        self.reject_flag("interactive", self.interactive)?;
        self.reject_present("test_dir", self.test_dir.as_deref())?;
        self.reject_present("snapshot_dir", self.snapshot_dir.as_deref())?;
        self.reject_flag("include_off", self.include_off)?;
        self.reject_nth_child()?;
        self.reject_range()?;
        self.reject_composite_rules()?;
        self.reject_transform()?;
        Ok(())
    }

    fn validate_test(&self) -> Result<()> {
        self.reject_present("pattern", self.pattern.as_deref())?;
        self.reject_present("kind", self.kind.as_deref())?;
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
        self.reject_present("regex", self.regex.as_deref())?;
        self.reject_nth_child()?;
        self.reject_range()?;
        self.reject_composite_rules()?;
        self.reject_transform()?;
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

    fn validate_inspect(&self) -> Result<()> {
        self.reject_present("pattern", self.pattern.as_deref())?;
        self.reject_present("kind", self.kind.as_deref())?;
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
        self.reject_present("filter", self.filter.as_deref())?;
        self.reject_present("regex", self.regex.as_deref())?;
        self.reject_flag("skip_snapshot_tests", self.skip_snapshot_tests)?;
        self.reject_flag("update_all", self.update_all)?;
        self.reject_flag("interactive", self.interactive)?;
        self.reject_present("test_dir", self.test_dir.as_deref())?;
        self.reject_present("snapshot_dir", self.snapshot_dir.as_deref())?;
        self.reject_flag("include_off", self.include_off)?;
        self.reject_nth_child()?;
        self.reject_range()?;
        self.reject_composite_rules()?;
        self.reject_transform()?;
        if self.globs.is_some() {
            bail!(
                "action='structural' workflow='inspect' does not accept `globs`; use `config_path` and `path`."
            );
        }
        if self.context_lines.is_some() {
            bail!(
                "action='structural' workflow='inspect' does not accept `context_lines`; use `config_path` and `path`."
            );
        }
        if self.max_results.is_some() {
            bail!(
                "action='structural' workflow='inspect' does not accept `max_results`; use `config_path` and `path`."
            );
        }
        Ok(())
    }

    fn validate_rewrite(&self) -> Result<()> {
        if self.pattern().is_none() && self.regex_pattern().is_none() {
            bail!(
                "action='structural' workflow='rewrite' requires a non-empty `pattern` or `regex`"
            );
        }

        let has_string_rewrite = self.rewrite_text().is_some();
        let has_fix_config = self.fix_config.is_some();

        if !has_string_rewrite && !has_fix_config {
            bail!(
                "action='structural' workflow='rewrite' requires a non-empty `rewrite` string \
                 or a `fix_config` object with `template` and optional `expand_start`/`expand_end`"
            );
        }

        if has_fix_config {
            self.fix_config
                .as_ref()
                .expect("fix_config validated present")
                .validate()?;
        }

        self.reject_present("config_path", self.config_path.as_deref())?;
        self.reject_present("filter", self.filter.as_deref())?;
        self.reject_flag("skip_snapshot_tests", self.skip_snapshot_tests)?;
        self.reject_flag("update_all", self.update_all)?;
        self.reject_flag("interactive", self.interactive)?;
        self.reject_nth_child()?;
        self.reject_range()?;
        self.reject_relational_rules()?;
        self.reject_composite_rules()?;

        if self.debug_query.is_some() && self.lang.as_deref().is_none_or(str::is_empty) {
            bail!(DEBUG_QUERY_LANG_HINT);
        }

        if self.regex_pattern().is_some() && self.lang.as_deref().is_none_or(str::is_empty) {
            bail!("action='structural' with `regex` requires `lang` to be set");
        }

        Ok(())
    }

    fn validate_new(&self) -> Result<()> {
        let subcommand = self
            .new_subcommand
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let subcommand = subcommand.ok_or_else(|| {
            anyhow!(
                "action='structural' workflow='new' requires `new_subcommand` \
                 (one of: project, rule, test, util)"
            )
        })?;

        if !matches!(subcommand, "project" | "rule" | "test" | "util") {
            bail!(
                "action='structural' workflow='new' `new_subcommand` must be one of \
                 project, rule, test, util; got `{subcommand}`"
            );
        }

        // rule, test, and util require a name.
        if subcommand != "project" {
            let name = self
                .new_name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            if name.is_none() {
                bail!(
                    "action='structural' workflow='new' subcommand `{subcommand}` \
                     requires `new_name`"
                );
            }
        }

        // rule and util require a language.
        if (subcommand == "rule" || subcommand == "util")
            && self.lang.as_deref().is_none_or(str::is_empty)
        {
            bail!(
                "action='structural' workflow='new' subcommand `{subcommand}` \
                 requires `lang`"
            );
        }

        // Reject fields that don't apply to the new workflow.
        self.reject_present("pattern", self.pattern.as_deref())?;
        self.reject_present("kind", self.kind.as_deref())?;
        self.reject_present("selector", self.selector.as_deref())?;
        self.reject_present(
            "strictness",
            self.strictness.as_ref().map(StructuralStrictness::as_str),
        )?;
        self.reject_present(
            "debug_query",
            self.debug_query.as_ref().map(DebugQueryFormat::as_str),
        )?;
        self.reject_present("filter", self.filter.as_deref())?;
        self.reject_present("regex", self.regex.as_deref())?;
        self.reject_present("rewrite", self.rewrite.as_deref())?;
        self.reject_flag("skip_snapshot_tests", self.skip_snapshot_tests)?;
        self.reject_flag("update_all", self.update_all)?;
        self.reject_flag("interactive", self.interactive)?;
        self.reject_nth_child()?;
        self.reject_range()?;
        self.reject_relational_rules()?;
        self.reject_composite_rules()?;
        self.reject_transform()?;

        Ok(())
    }

    fn validate_apply(&self) -> Result<()> {
        if self.pattern().is_none() && self.regex_pattern().is_none() {
            bail!("action='structural' workflow='apply' requires a non-empty `pattern` or `regex`");
        }

        let has_string_rewrite = self.rewrite_text().is_some();
        let has_fix_config = self.fix_config.is_some();

        if !has_string_rewrite && !has_fix_config {
            bail!(
                "action='structural' workflow='apply' requires a non-empty `rewrite` string \
                 or a `fix_config` object with `template` and optional `expand_start`/`expand_end`"
            );
        }

        if has_fix_config {
            self.fix_config
                .as_ref()
                .expect("fix_config validated present")
                .validate()?;
        }

        self.reject_present("config_path", self.config_path.as_deref())?;
        self.reject_present("filter", self.filter.as_deref())?;
        self.reject_flag("skip_snapshot_tests", self.skip_snapshot_tests)?;
        self.reject_flag("update_all", self.update_all)?;
        self.reject_flag("interactive", self.interactive)?;
        self.reject_nth_child()?;
        self.reject_range()?;
        self.reject_relational_rules()?;
        self.reject_composite_rules()?;

        if self.debug_query.is_some() && self.lang.as_deref().is_none_or(str::is_empty) {
            bail!(DEBUG_QUERY_LANG_HINT);
        }

        if self.regex_pattern().is_some() && self.lang.as_deref().is_none_or(str::is_empty) {
            bail!("action='structural' with `regex` requires `lang` to be set");
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

    fn reject_nth_child(&self) -> Result<()> {
        if self.nth_child.is_some() {
            bail!(
                "action='structural' workflow='{}' does not accept `nth_child`.",
                self.workflow.as_str()
            );
        }
        Ok(())
    }

    fn reject_range(&self) -> Result<()> {
        if self.range.is_some() {
            bail!(
                "action='structural' workflow='{}' does not accept `range`.",
                self.workflow.as_str()
            );
        }
        Ok(())
    }

    fn reject_relational_rules(&self) -> Result<()> {
        if self.has.is_some()
            || self.inside.is_some()
            || self.follows.is_some()
            || self.precedes.is_some()
            || self.constraints.is_some()
        {
            bail!(
                "action='structural' workflow='{}' does not accept relational rules (`has`, `inside`, `follows`, `precedes`) or `constraints`.",
                self.workflow.as_str()
            );
        }
        Ok(())
    }

    fn reject_composite_rules(&self) -> Result<()> {
        if self.matches.is_some()
            || self.all.is_some()
            || self.any.is_some()
            || self.not.is_some()
            || self.utils.is_some()
        {
            bail!(
                "action='structural' workflow='{}' does not accept composite rules \
                 (`matches`, `all`, `any`, `not`) or `utils`.",
                self.workflow.as_str()
            );
        }
        Ok(())
    }

    fn reject_transform(&self) -> Result<()> {
        if self.transform.is_some() {
            bail!(
                "action='structural' workflow='{}' does not accept `transform`; \
                 `transform` is only valid for `query`, `count`, and `rewrite` workflows \
                 that use YAML rule generation.",
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

    fn kind(&self) -> Option<&str> {
        self.kind
            .as_deref()
            .map(str::trim)
            .filter(|kind| !kind.is_empty())
    }

    fn regex_pattern(&self) -> Option<&str> {
        self.regex
            .as_deref()
            .map(str::trim)
            .filter(|r| !r.is_empty())
    }

    fn filter(&self) -> Option<&str> {
        self.filter
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    fn rewrite_text(&self) -> Option<&str> {
        self.rewrite
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    /// Returns the effective rewrite template: the simple `rewrite` string,
    /// or the `fix_config.template` when a FixConfig is present.
    fn effective_rewrite_template(&self) -> Option<&str> {
        if let Some(rewrite) = self.rewrite_text() {
            return Some(rewrite);
        }
        self.fix_config
            .as_ref()
            .map(|fc| fc.template.trim())
            .filter(|t| !t.is_empty())
    }

    fn normalized_globs(&self) -> Vec<String> {
        let mut result: Vec<String> = self
            .globs
            .clone()
            .map(GlobInput::into_vec)
            .unwrap_or_default()
            .into_iter()
            .map(|glob| glob.trim().to_string())
            .filter(|glob| !glob.is_empty())
            .collect();

        // Merge exclude patterns as negative globs (prefixed with `!`).
        if let Some(excludes) = &self.exclude {
            for pattern in excludes.clone().into_vec() {
                let trimmed = pattern.trim().to_string();
                if trimmed.is_empty() {
                    continue;
                }
                // Strip a leading `!` if the caller already provided one,
                // then re-add it to guarantee the negative-glob prefix.
                let negative = if let Some(stripped) = trimmed.strip_prefix('!') {
                    format!("!{stripped}")
                } else {
                    format!("!{trimmed}")
                };
                result.push(negative);
            }
        }

        result
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

    fn effective_severities(&self) -> Option<Vec<&str>> {
        self.severities.as_ref().map(|v| {
            v.iter()
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .map(|s| match s.as_str() {
                    "error" => "error",
                    "warning" | "warn" => "warning",
                    "info" => "info",
                    "hint" => "hint",
                    _ => "unknown",
                })
                .collect()
        })
    }

    fn effective_no_ignore(&self) -> Option<&[String]> {
        self.no_ignore
            .as_ref()
            .filter(|v| !v.is_empty())
            .map(|v| v.as_slice())
    }

    fn effective_follow(&self) -> bool {
        self.follow == Some(true)
    }

    fn effective_threads(&self) -> Option<u32> {
        self.threads.map(|t| t.min(MAX_THREADS))
    }

    fn effective_format(&self) -> Option<&str> {
        self.format
            .as_deref()
            .map(str::trim)
            .filter(|f| !f.is_empty())
    }

    fn effective_report_style(&self) -> Option<&str> {
        self.report_style
            .as_deref()
            .map(str::trim)
            .filter(|r| !r.is_empty())
    }

    fn effective_before_lines(&self) -> Option<usize> {
        self.before_lines
            .filter(|&n| n <= MAX_ALLOWED_CONTEXT_LINES)
    }

    fn effective_after_lines(&self) -> Option<usize> {
        self.after_lines.filter(|&n| n <= MAX_ALLOWED_CONTEXT_LINES)
    }

    fn effective_builtin_rules(&self) -> Option<&[String]> {
        self.builtin_rules
            .as_ref()
            .filter(|v| !v.is_empty())
            .map(|v| v.as_slice())
    }
}

#[derive(Debug, Clone, Deserialize)]
struct AstGrepMetaVar {
    text: String,
    range: AstGrepRange,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct AstGrepMetaVariables {
    #[serde(default)]
    single: BTreeMap<String, AstGrepMetaVar>,
    #[serde(default)]
    multi: BTreeMap<String, Vec<AstGrepMetaVar>>,
    #[serde(default)]
    transformed: BTreeMap<String, String>,
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
    #[serde(default, rename = "metaVariables")]
    meta_variables: Option<AstGrepMetaVariables>,
}

#[derive(Debug, Clone, Deserialize)]
struct AstGrepRewriteMatch {
    file: String,
    text: String,
    #[serde(default)]
    lines: Option<String>,
    #[serde(default)]
    language: Option<String>,
    range: AstGrepRange,
    #[serde(default, rename = "metaVariables")]
    meta_variables: Option<AstGrepMetaVariables>,
    #[serde(default)]
    replacement: Option<String>,
    #[serde(default, rename = "replacementOffsets")]
    replacement_offsets: Option<AstGrepByteOffset>,
}

#[derive(Debug, Clone, Deserialize)]
struct AstGrepLabel {
    text: String,
    range: AstGrepRange,
    #[serde(default)]
    source: Option<String>,
}

/// Severity level for ast-grep scan findings.
///
/// ast-grep defines five severity levels:
/// - `error`: reports an error; causes `ast-grep scan` to exit non-zero
/// - `warning`: reports a warning
/// - `info`: reports an informational message
/// - `hint`: reports a hint (the default severity for ast-grep rules)
/// - `off`: disables the rule entirely
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AstGrepSeverity {
    Error,
    Warning,
    Info,
    Hint,
    Off,
}

impl AstGrepSeverity {
    fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
            Self::Hint => "hint",
            Self::Off => "off",
        }
    }

    fn from_str_normalized(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "error" => Some(Self::Error),
            "warning" | "warn" => Some(Self::Warning),
            "info" => Some(Self::Info),
            "hint" => Some(Self::Hint),
            "off" | "none" => Some(Self::Off),
            _ => None,
        }
    }
}

impl fmt::Display for AstGrepSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AstGrepSeverity {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        AstGrepSeverity::from_str_normalized(&s).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "unknown severity `{s}`; expected error, warning, info, hint, or off"
            ))
        })
    }
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
    severity: Option<AstGrepSeverity>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    metadata: Option<Value>,
    #[serde(default)]
    labels: Vec<AstGrepLabel>,
}

#[derive(Debug, Clone, Deserialize)]
struct AstGrepByteOffset {
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct AstGrepRange {
    start: AstGrepPoint,
    end: AstGrepPoint,
    #[serde(default, rename = "byteOffset")]
    byte_offset: Option<AstGrepByteOffset>,
}

#[derive(Debug, Clone, Deserialize)]
struct AstGrepPoint {
    line: usize,
    column: usize,
}

pub async fn execute_structural_search(workspace_root: &Path, args: Value) -> Result<Value> {
    let request = StructuralSearchRequest::from_args(&args)?;
    // Pure-Rust workflows that don't need the ast-grep binary.
    if request.workflow == StructuralWorkflow::Inspect {
        return execute_structural_inspect(workspace_root, &request).await;
    }
    if request.workflow == StructuralWorkflow::Rules {
        return execute_structural_rules(workspace_root, &request).await;
    }
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
        StructuralWorkflow::Inspect => {
            anyhow::bail!("Inspect workflow should be handled before this point")
        }
        StructuralWorkflow::Rewrite => {
            execute_structural_rewrite(workspace_root, &request, &ast_grep).await
        }
        StructuralWorkflow::Count => {
            execute_structural_count(workspace_root, &request, &ast_grep).await
        }
        StructuralWorkflow::Rules => {
            anyhow::bail!("Rules workflow should be handled before this point")
        }
        StructuralWorkflow::New => {
            execute_structural_new(workspace_root, &request, &ast_grep).await
        }
        StructuralWorkflow::Apply => {
            execute_structural_apply(workspace_root, &request, &ast_grep).await
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
    if request.pattern().is_some()
        && let Some(hint) = preflight_parseable_pattern(request)?
    {
        return Ok(build_fragment_result(
            request,
            &search_path.display_path,
            hint,
        ));
    }
    let command_path = search_path.command_arg.clone();

    if let Some(debug_query) = &request.debug_query {
        let pattern = request
            .pattern()
            .ok_or_else(|| anyhow!("pattern is required for debug query"))?;
        let lang = request
            .lang
            .as_deref()
            .filter(|l| !l.trim().is_empty())
            .ok_or_else(|| anyhow!("lang is required for debug query"))?;
        let mut command = ast_grep_command(ast_grep, workspace_root, "run");
        command
            .arg(format!("--pattern={}", pattern))
            .arg("--lang")
            .arg(lang)
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

    // When relational rules, composite rules, transforms, or constraints are
    // present, use YAML rule generation because these operators cannot be
    // expressed via CLI flags.
    if request.has.is_some()
        || request.inside.is_some()
        || request.follows.is_some()
        || request.precedes.is_some()
        || request.constraints.is_some()
        || request.matches.is_some()
        || request.all.is_some()
        || request.any.is_some()
        || request.not.is_some()
        || request.utils.is_some()
        || request.transform.is_some()
    {
        return execute_atomic_rule_query(workspace_root, request, ast_grep, &search_path).await;
    }

    let mut command = ast_grep_command(ast_grep, workspace_root, "run");
    if let Some(pattern) = request.pattern() {
        command.arg(format!("--pattern={pattern}"));
    }
    command.arg("--json=compact").arg("--color=never");

    if let Some(lang) = request
        .lang
        .as_deref()
        .filter(|lang| !lang.trim().is_empty())
    {
        command.arg("--lang").arg(lang);
    }
    if let Some(kind) = request.kind() {
        command.arg("--kind").arg(kind);
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
    apply_context_and_globs(
        &mut command,
        request.context_lines,
        request.effective_before_lines(),
        request.effective_after_lines(),
        &globs,
    );
    if request.effective_follow() {
        command.arg("--follow");
    }
    if let Some(no_ignore) = request.effective_no_ignore() {
        for value in no_ignore {
            command.arg("--no-ignore").arg(value.trim());
        }
    }
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
    let config_path =
        resolve_config_path(workspace_root, request.requested_config_path(), true).await?;
    let globs = request.normalized_globs();

    // When --format is set (github/sarif), we skip --json and --include-metadata
    // because the output format changes and we return raw output instead.
    let use_ci_format = request.effective_format().is_some();

    let mut command = ast_grep_command(ast_grep, workspace_root, "scan");
    command
        .arg("--config")
        .arg(&config_path.command_arg)
        .arg("--color=never");

    if use_ci_format {
        command.arg(format!(
            "--format={}",
            request
                .effective_format()
                .expect("use_ci_format is true only when effective_format is Some")
        ));
    } else {
        command.arg("--json=stream").arg("--include-metadata");
    }

    if let Some(filter) = request.filter() {
        command.arg("--filter").arg(filter);
    }

    // --no-ignore flags.
    if let Some(no_ignore) = request.effective_no_ignore() {
        for value in no_ignore {
            command.arg("--no-ignore").arg(value.trim());
        }
    }

    // --follow flag.
    if request.effective_follow() {
        command.arg("--follow");
    }

    // --threads flag.
    if let Some(threads) = request.effective_threads() {
        command.arg("--threads").arg(threads.to_string());
    }

    // --report-style flag.
    if let Some(style) = request.effective_report_style() {
        command.arg(format!("--report-style={style}"));
    }

    // Built-in rules as severity override flags (e.g. --error=unused-suppression).
    if let Some(builtin_rules) = request.effective_builtin_rules() {
        for rule_entry in builtin_rules {
            let (rule_name, severity) = match rule_entry.split_once(':') {
                Some((name, sev)) => (name.trim(), sev.trim()),
                None => (rule_entry.trim(), ""),
            };
            if severity.is_empty() {
                // Activate at built-in default severity.
                command.arg(format!("--hint={rule_name}"));
            } else {
                command.arg(format!("--{severity}={rule_name}"));
            }
        }
    }

    apply_context_and_globs(
        &mut command,
        request.context_lines,
        request.effective_before_lines(),
        request.effective_after_lines(),
        &globs,
    );
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

    // When --format is set, return the raw formatted output instead of
    // parsing as JSON stream (github/sarif formats are not JSON stream).
    if use_ci_format {
        let raw = String::from_utf8_lossy(&output.stdout);
        return Ok(json!({
            "backend": "ast-grep",
            "workflow": "scan",
            "config_path": config_path.display_path,
            "path": search_path.display_path,
            "format": request.effective_format(),
            "output": truncate_auxiliary_output(&raw),
            "exit_code": output.status.code(),
        }));
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
    let config_path =
        resolve_config_path(workspace_root, request.requested_config_path(), true).await?;

    let mut command = ast_grep_command(ast_grep, workspace_root, "test");
    command.arg("--config").arg(&config_path.command_arg);

    if let Some(filter) = request.filter() {
        command.arg("--filter").arg(filter);
    }
    if request.skip_snapshot_tests == Some(true) {
        command.arg("--skip-snapshot-tests");
    }
    if request.update_all == Some(true) {
        command.arg("--update-all");
    }
    if request.interactive == Some(true) {
        command.arg("--interactive");
    }
    if let Some(test_dir) = request.test_dir.as_deref().filter(|s| !s.trim().is_empty()) {
        command.arg("--test-dir").arg(test_dir);
    }
    if let Some(snapshot_dir) = request
        .snapshot_dir
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        command.arg("--snapshot-dir").arg(snapshot_dir);
    }
    if request.include_off == Some(true) {
        command.arg("--include-off");
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

async fn execute_structural_rewrite(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
) -> Result<Value> {
    let search_path = resolve_search_path(workspace_root, request.requested_path())?;
    let globs = request.normalized_globs();

    if let Some(hint) = preflight_parseable_pattern(request)? {
        return Ok(build_rewrite_fragment_result(
            request,
            &search_path.display_path,
            hint,
        ));
    }

    // When FixConfig with expansion or transform is present, use the YAML
    // rule path because `sg run --rewrite` only supports simple string fixes.
    let needs_yaml_rewrite = request
        .fix_config
        .as_ref()
        .is_some_and(|fc| fc.has_expansion())
        || request.transform.is_some();

    if needs_yaml_rewrite {
        return execute_fixconfig_rewrite(workspace_root, request, ast_grep, &search_path).await;
    }

    // Simple string rewrite via `sg run --rewrite`.
    let command_path = search_path.command_arg.clone();
    let pattern = request
        .pattern()
        .ok_or_else(|| anyhow!("pattern is required for rewrite"))?;
    let template = request
        .effective_rewrite_template()
        .ok_or_else(|| anyhow!("rewrite template is required"))?;
    let mut command = ast_grep_command(ast_grep, workspace_root, "run");
    command
        .arg(format!("--pattern={}", pattern))
        .arg(format!("--rewrite={}", template))
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
    apply_context_and_globs(
        &mut command,
        request.context_lines,
        request.effective_before_lines(),
        request.effective_after_lines(),
        &globs,
    );
    if request.effective_follow() {
        command.arg("--follow");
    }
    if let Some(no_ignore) = request.effective_no_ignore() {
        for value in no_ignore {
            command.arg("--no-ignore").arg(value.trim());
        }
    }
    command.arg(&command_path);

    let output =
        run_ast_grep_command(&mut command, "failed to run ast-grep structural rewrite").await?;

    let no_matches = output.status.code() == Some(1);
    if !output.status.success() && !no_matches {
        bail!(
            "{}",
            format_ast_grep_failure(
                "ast-grep structural rewrite failed",
                stderr_or_stdout(&output.stderr, &output.stdout)
            )
        );
    }

    let rewrites = if no_matches && String::from_utf8_lossy(&output.stdout).trim().is_empty() {
        Vec::new()
    } else {
        parse_rewrite_matches(&output.stdout)?
    };
    Ok(build_rewrite_result(
        request,
        &search_path.display_path,
        rewrites,
    ))
}

async fn execute_structural_count(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
) -> Result<Value> {
    let search_path = resolve_search_path(workspace_root, request.requested_path())?;
    let globs = request.normalized_globs();

    // When nthChild, range, relational rules, composite rules, transforms, or
    // constraints are present, use YAML rule generation and count scan findings.
    if request.nth_child.is_some()
        || request.range.is_some()
        || request.has.is_some()
        || request.inside.is_some()
        || request.follows.is_some()
        || request.precedes.is_some()
        || request.constraints.is_some()
        || request.matches.is_some()
        || request.all.is_some()
        || request.any.is_some()
        || request.not.is_some()
        || request.utils.is_some()
        || request.transform.is_some()
    {
        return execute_atomic_rule_count(workspace_root, request, ast_grep, &search_path).await;
    }

    let command_path = search_path.command_arg.clone();
    let mut command = ast_grep_command(ast_grep, workspace_root, "run");
    if let Some(pattern) = request.pattern() {
        command.arg(format!("--pattern={pattern}"));
    }
    command.arg("--json=compact").arg("--color=never");

    if let Some(lang) = request
        .lang
        .as_deref()
        .filter(|lang| !lang.trim().is_empty())
    {
        command.arg("--lang").arg(lang);
    }
    if let Some(kind) = request.kind() {
        command.arg("--kind").arg(kind);
    }
    if let Some(regex) = request.regex_pattern() {
        command.arg("--regex").arg(regex);
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
    apply_context_and_globs(
        &mut command,
        request.context_lines,
        request.effective_before_lines(),
        request.effective_after_lines(),
        &globs,
    );
    if request.effective_follow() {
        command.arg("--follow");
    }
    if let Some(no_ignore) = request.effective_no_ignore() {
        for value in no_ignore {
            command.arg("--no-ignore").arg(value.trim());
        }
    }
    command.arg(&command_path);

    let output =
        run_ast_grep_command(&mut command, "failed to run ast-grep structural count").await?;

    let no_matches = output.status.code() == Some(1);
    if !output.status.success() && !no_matches {
        bail!(
            "{}",
            format_ast_grep_failure(
                "ast-grep structural count failed",
                stderr_or_stdout(&output.stderr, &output.stdout)
            )
        );
    }

    let count = if no_matches && String::from_utf8_lossy(&output.stdout).trim().is_empty() {
        0
    } else {
        parse_compact_matches(&output.stdout)?.len()
    };

    let max_results = request.effective_max_results();
    let truncated = count > max_results;

    let mut result = json!({
        "backend": "ast-grep",
        "workflow": "count",
        "path": search_path.display_path,
        "count": count,
        "truncated": truncated,
    });
    if let Some(pattern) = request.pattern() {
        result["pattern"] = json!(pattern);
    }
    if let Some(kind) = request.kind() {
        result["kind"] = json!(kind);
    }
    Ok(result)
}

/// Build a YAML rule string for an atomic count query.
fn build_atomic_rule_yaml(request: &StructuralSearchRequest, lang: &str) -> String {
    use std::fmt::Write as _;
    let mut yaml = String::new();
    let _ = writeln!(yaml, "id: atomic-count");
    let _ = writeln!(yaml, "language: {lang}");
    let _ = writeln!(yaml, "severity: info");

    // Emit local utility rules if present.
    if let Some(utils) = &request.utils
        && !utils.is_empty()
    {
        yaml.push_str("utils:\n");
        for (util_name, util_rule) in utils {
            let _ = writeln!(yaml, "  {}:", util_name);
            value_to_yaml(&mut yaml, util_rule, 4);
        }
    }

    let _ = writeln!(yaml, "rule:");

    if let Some(pattern) = request.pattern() {
        let _ = writeln!(yaml, "  pattern: {}", yaml_escape_scalar(pattern));
    }
    if let Some(kind) = request.kind() {
        let _ = writeln!(yaml, "  kind: {}", yaml_escape_scalar(kind));
    }
    if let Some(regex) = request.regex_pattern() {
        let _ = writeln!(yaml, "  regex: {}", yaml_escape_scalar(regex));
    }
    if let Some(selector) = request.selector.as_deref().filter(|s| !s.trim().is_empty()) {
        let _ = writeln!(yaml, "  selector: {}", yaml_escape_scalar(selector));
    }
    if let Some(strictness) = &request.strictness {
        let _ = writeln!(yaml, "  strictness: {}", strictness.as_str());
    }

    if let Some(nth) = &request.nth_child {
        match nth {
            NthChildInput::Number(n) => {
                let _ = writeln!(yaml, "  nthChild: {n}");
            }
            NthChildInput::Formula(f) => {
                let _ = writeln!(yaml, "  nthChild: {}", yaml_escape_scalar(f));
            }
            NthChildInput::Object(obj) => {
                let _ = writeln!(yaml, "  nthChild:");
                match &obj.position {
                    Value::Number(n) => {
                        let _ = writeln!(yaml, "    position: {n}");
                    }
                    Value::String(s) => {
                        let _ = writeln!(yaml, "    position: {}", yaml_escape_scalar(s));
                    }
                    _ => {
                        let _ = writeln!(yaml, "    position: {}", obj.position);
                    }
                }
                if let Some(reverse) = obj.reverse {
                    let _ = writeln!(yaml, "    reverse: {reverse}");
                }
                if let Some(of_rule) = &obj.of_rule {
                    let _ = writeln!(yaml, "    ofRule:");
                    if let Some(of_obj) = of_rule.as_object() {
                        for (k, v) in of_obj {
                            match v {
                                Value::String(s) => {
                                    let _ = writeln!(yaml, "      {k}: {}", yaml_escape_scalar(s));
                                }
                                Value::Number(n) => {
                                    let _ = writeln!(yaml, "      {k}: {n}");
                                }
                                Value::Bool(b) => {
                                    let _ = writeln!(yaml, "      {k}: {b}");
                                }
                                _ => {
                                    let _ = writeln!(yaml, "      {k}: {v}");
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(r) = &request.range {
        let _ = writeln!(yaml, "  range:");
        let _ = writeln!(yaml, "    start:");
        let _ = writeln!(yaml, "      line: {}", r.start.line);
        let _ = writeln!(yaml, "      column: {}", r.start.column);
        let _ = writeln!(yaml, "    end:");
        let _ = writeln!(yaml, "      line: {}", r.end.line);
        let _ = writeln!(yaml, "      column: {}", r.end.column);
    }

    // Relational rules.
    emit_value_yaml_field(&mut yaml, "  ", "has", request.has.as_deref());
    emit_value_yaml_field(&mut yaml, "  ", "inside", request.inside.as_deref());
    emit_value_yaml_field(&mut yaml, "  ", "follows", request.follows.as_deref());
    emit_value_yaml_field(&mut yaml, "  ", "precedes", request.precedes.as_deref());

    // Constraints.
    if let Some(constraints) = &request.constraints
        && !constraints.is_empty()
    {
        yaml.push_str("  constraints:\n");
        for (var_name, constraint_value) in constraints {
            yaml.push_str(&format!("    {}:\n", var_name));
            value_to_yaml(&mut yaml, constraint_value, 6);
        }
    }

    // Composite rules.
    if let Some(matches_name) = &request.matches {
        let _ = writeln!(yaml, "  matches: {}", yaml_escape_scalar(matches_name));
    }
    if let Some(all_rules) = &request.all
        && !all_rules.is_empty()
    {
        yaml.push_str("  all:\n");
        for rule in all_rules {
            yaml.push_str("    - ");
            match rule {
                Value::String(s) => {
                    let _ = writeln!(yaml, "pattern: {}", yaml_escape_scalar(s));
                }
                _ => {
                    yaml.push('\n');
                    value_to_yaml(&mut yaml, rule, 6);
                }
            }
        }
    }
    if let Some(any_rules) = &request.any
        && !any_rules.is_empty()
    {
        yaml.push_str("  any:\n");
        for rule in any_rules {
            yaml.push_str("    - ");
            match rule {
                Value::String(s) => {
                    let _ = writeln!(yaml, "pattern: {}", yaml_escape_scalar(s));
                }
                _ => {
                    yaml.push('\n');
                    value_to_yaml(&mut yaml, rule, 6);
                }
            }
        }
    }
    if let Some(not_rule) = &request.not {
        yaml.push_str("  not:\n");
        match not_rule.as_ref() {
            Value::String(s) => {
                let _ = writeln!(yaml, "    pattern: {}", yaml_escape_scalar(s));
            }
            _ => {
                value_to_yaml(&mut yaml, not_rule, 4);
            }
        }
    }

    // Emit transform pipeline if present.
    if let Some(transform) = &request.transform
        && !transform.is_empty()
    {
        yaml.push_str("transform:\n");
        for (var_name, transform_def) in transform {
            let _ = writeln!(yaml, "  {}:", var_name);
            value_to_yaml(&mut yaml, transform_def, 4);
        }
    }

    yaml
}

/// Emit a relational rule field from a JSON value into YAML.
///
/// When the value is a bare string, it is emitted as `pattern: <value>` under
/// the field name (matching ast-grep's shorthand semantics where a string
/// relational rule means `{pattern: "..."}`).
fn emit_value_yaml_field(yaml: &mut String, pad: &str, name: &str, value: Option<&Value>) {
    if let Some(val) = value {
        yaml.push_str(&format!("{pad}{name}:\n"));
        match val {
            Value::String(s) => {
                let child_pad = " ".repeat(pad.len() + 2);
                yaml.push_str(&format!("{child_pad}pattern: {}\n", yaml_escape_scalar(s)));
            }
            _ => {
                value_to_yaml(yaml, val, pad.len() + 2);
            }
        }
    }
}

/// Recursively serialize a JSON value to YAML at the given indentation.
fn value_to_yaml(yaml: &mut String, value: &Value, indent: usize) {
    let pad = " ".repeat(indent);
    match value {
        Value::String(s) => {
            yaml.push_str(&format!("{pad}{}\n", yaml_escape_scalar(s)));
        }
        Value::Number(n) => {
            yaml.push_str(&format!("{pad}{n}\n"));
        }
        Value::Bool(b) => {
            yaml.push_str(&format!("{pad}{b}\n"));
        }
        Value::Null => {
            yaml.push_str(&format!("{pad}null\n"));
        }
        Value::Array(arr) => {
            for item in arr {
                yaml.push_str(&format!("{pad}- "));
                match item {
                    Value::String(s) => yaml.push_str(&format!("{}\n", yaml_escape_scalar(s))),
                    Value::Number(n) => yaml.push_str(&format!("{n}\n")),
                    Value::Bool(b) => yaml.push_str(&format!("{b}\n")),
                    _ => {
                        yaml.push('\n');
                        value_to_yaml(yaml, item, indent + 2);
                    }
                }
            }
        }
        Value::Object(obj) => {
            for (key, val) in obj {
                match val {
                    Value::Object(_) | Value::Array(_) => {
                        yaml.push_str(&format!("{pad}{key}:\n"));
                        value_to_yaml(yaml, val, indent + 2);
                    }
                    Value::String(s) => {
                        yaml.push_str(&format!("{pad}{key}: {}\n", yaml_escape_scalar(s)));
                    }
                    Value::Number(n) => {
                        yaml.push_str(&format!("{pad}{key}: {n}\n"));
                    }
                    Value::Bool(b) => {
                        yaml.push_str(&format!("{pad}{key}: {b}\n"));
                    }
                    Value::Null => {
                        yaml.push_str(&format!("{pad}{key}: null\n"));
                    }
                }
            }
        }
    }
}

/// Execute count via YAML rule generation (for nthChild/range/has/inside/constraints).
async fn execute_atomic_rule_count(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
    search_path: &ResolvedSearchPath,
) -> Result<Value> {
    let lang = request
        .lang
        .as_deref()
        .filter(|l| !l.trim().is_empty())
        .unwrap_or("javascript");

    let rule_yaml = build_atomic_rule_yaml(request, lang);

    let temp_dir = tempfile::tempdir().with_context(|| {
        "failed to create temporary directory for atomic rule count".to_string()
    })?;
    let rules_dir = temp_dir.path().join("rules");
    afs::create_dir_all(&rules_dir).await.with_context(|| {
        format!(
            "failed to create rules directory at {}",
            rules_dir.display()
        )
    })?;

    let rule_path = rules_dir.join("atomic-count.yml");
    afs::write(&rule_path, &rule_yaml)
        .await
        .with_context(|| format!("failed to write atomic rule to {}", rule_path.display()))?;

    let sgconfig_path = temp_dir.path().join("sgconfig.yml");
    let sgconfig_content = format!("ruleDirs:\n  - {}\n", rules_dir.display());
    afs::write(&sgconfig_path, &sgconfig_content)
        .await
        .with_context(|| {
            format!(
                "failed to write sgconfig.yml to {}",
                sgconfig_path.display()
            )
        })?;

    let mut command = ast_grep_command(ast_grep, workspace_root, "scan");
    command
        .arg("--config")
        .arg(&sgconfig_path)
        .arg("--json=stream")
        .arg("--include-metadata")
        .arg("--color=never");

    let globs = request.normalized_globs();
    apply_context_and_globs(
        &mut command,
        request.context_lines,
        request.effective_before_lines(),
        request.effective_after_lines(),
        &globs,
    );
    command.arg(&search_path.command_arg);

    let output =
        run_ast_grep_command(&mut command, "failed to run ast-grep atomic rule count").await?;

    let findings_with_error_exit = output.status.code() == Some(1);
    if !output.status.success() && !findings_with_error_exit {
        bail!(
            "{}",
            format_ast_grep_failure(
                "ast-grep atomic rule count failed",
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

    let count = findings.len();
    let max_results = request.effective_max_results();
    let truncated = count > max_results;

    let mut result = json!({
        "backend": "ast-grep",
        "workflow": "count",
        "path": search_path.display_path,
        "count": count,
        "truncated": truncated,
    });
    if let Some(pattern) = request.pattern() {
        result["pattern"] = json!(pattern);
    }
    if let Some(kind) = request.kind() {
        result["kind"] = json!(kind);
    }
    Ok(result)
}

/// Execute a query via YAML rule generation when relational rules
/// or constraints are present.
async fn execute_atomic_rule_query(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
    search_path: &ResolvedSearchPath,
) -> Result<Value> {
    let lang = request
        .lang
        .as_deref()
        .filter(|l| !l.trim().is_empty())
        .unwrap_or("javascript");

    let rule_yaml = build_atomic_rule_yaml(request, lang);

    let temp_dir = tempfile::tempdir().with_context(|| {
        "failed to create temporary directory for atomic rule query".to_string()
    })?;
    let rules_dir = temp_dir.path().join("rules");
    afs::create_dir_all(&rules_dir).await.with_context(|| {
        format!(
            "failed to create rules directory at {}",
            rules_dir.display()
        )
    })?;

    let rule_path = rules_dir.join("atomic-query.yml");
    afs::write(&rule_path, &rule_yaml)
        .await
        .with_context(|| format!("failed to write atomic rule to {}", rule_path.display()))?;

    let sgconfig_path = temp_dir.path().join("sgconfig.yml");
    let sgconfig_content = format!("ruleDirs:\n  - {}\n", rules_dir.display());
    afs::write(&sgconfig_path, &sgconfig_content)
        .await
        .with_context(|| {
            format!(
                "failed to write sgconfig.yml to {}",
                sgconfig_path.display()
            )
        })?;

    let mut command = ast_grep_command(ast_grep, workspace_root, "scan");
    command
        .arg("--config")
        .arg(&sgconfig_path)
        .arg("--json=stream")
        .arg("--include-metadata")
        .arg("--color=never");

    let globs = request.normalized_globs();
    apply_context_and_globs(
        &mut command,
        request.context_lines,
        request.effective_before_lines(),
        request.effective_after_lines(),
        &globs,
    );
    command.arg(&search_path.command_arg);

    let output =
        run_ast_grep_command(&mut command, "failed to run ast-grep atomic rule query").await?;

    let findings_with_error_exit = output.status.code() == Some(1);
    if !output.status.success() && !findings_with_error_exit {
        bail!(
            "{}",
            format_ast_grep_failure(
                "ast-grep atomic rule query failed",
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

    let max_results = request.effective_max_results();
    let truncated = findings.len() > max_results;
    let normalized_matches = findings
        .into_iter()
        .take(max_results)
        .map(|finding| {
            let mut match_object = Map::new();
            match_object.insert("file".to_string(), Value::String(finding.file));
            match_object.insert("line_number".to_string(), json!(finding.range.start.line));
            match_object.insert("text".to_string(), Value::String(finding.text.clone()));
            match_object.insert(
                "lines".to_string(),
                Value::String(finding.lines.unwrap_or(finding.text)),
            );
            if let Some(language) = finding.language {
                match_object.insert("language".to_string(), Value::String(language));
            }
            match_object.insert("range".to_string(), build_range_value(&finding.range));
            if let Some(message) = finding.message {
                match_object.insert("message".to_string(), Value::String(message));
            }
            if let Some(metadata) = &finding.metadata {
                match_object.insert("metadata".to_string(), metadata.clone());
            }
            Value::Object(match_object)
        })
        .collect::<Vec<_>>();

    let mut result = json!({
        "backend": "ast-grep",
        "path": search_path.display_path,
        "matches": normalized_matches,
        "truncated": truncated,
    });
    if let Some(pattern) = request.pattern() {
        result["pattern"] = json!(pattern);
    }
    if let Some(kind) = request.kind() {
        result["kind"] = json!(kind);
    }
    Ok(result)
}

/// Execute a FixConfig rewrite and return raw scan findings as rewrite-like
/// matches. The `replacement` field is set to the template string and
/// `replacement_offsets` is `None` because scan findings do not include
/// byte-offset replacement data.
async fn execute_fixconfig_rewrite_to_matches(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
    search_path: &ResolvedSearchPath,
) -> Result<Vec<AstGrepRewriteMatch>> {
    let fix_config = request
        .fix_config
        .as_ref()
        .ok_or_else(|| anyhow!("fix_config is required for fixconfig rewrite"))?;
    let pattern = request
        .pattern()
        .ok_or_else(|| anyhow!("pattern is required for fixconfig rewrite"))?;
    let lang = request
        .lang
        .as_deref()
        .filter(|l| !l.trim().is_empty())
        .unwrap_or("javascript");
    let template = request
        .effective_rewrite_template()
        .unwrap_or_default()
        .to_string();

    let rule_yaml = build_fixconfig_rule_yaml(
        pattern,
        lang,
        fix_config,
        request.selector.as_deref(),
        request.transform.as_ref(),
    );

    let temp_dir = tempfile::tempdir().with_context(|| {
        "failed to create temporary directory for FixConfig rewrite matches".to_string()
    })?;
    let rules_dir = temp_dir.path().join("rules");
    afs::create_dir_all(&rules_dir).await.with_context(|| {
        format!(
            "failed to create rules directory at {}",
            rules_dir.display()
        )
    })?;

    let rule_path = rules_dir.join("fixconfig-rewrite.yml");
    afs::write(&rule_path, &rule_yaml)
        .await
        .with_context(|| format!("failed to write FixConfig rule to {}", rule_path.display()))?;

    let sgconfig_path = temp_dir.path().join("sgconfig.yml");
    let sgconfig_content = format!("ruleDirs:\n  - {}\n", rules_dir.display());
    afs::write(&sgconfig_path, &sgconfig_content)
        .await
        .with_context(|| {
            format!(
                "failed to write sgconfig.yml to {}",
                sgconfig_path.display()
            )
        })?;

    let mut command = ast_grep_command(ast_grep, workspace_root, "scan");
    command
        .arg("--config")
        .arg(&sgconfig_path)
        .arg("--json=stream")
        .arg("--include-metadata")
        .arg("--color=never");

    let globs = request.normalized_globs();
    apply_context_and_globs(
        &mut command,
        request.context_lines,
        request.effective_before_lines(),
        request.effective_after_lines(),
        &globs,
    );
    command.arg(&search_path.command_arg);

    let output = run_ast_grep_command(
        &mut command,
        "failed to run ast-grep FixConfig rewrite scan for apply",
    )
    .await?;

    let findings_with_error_exit = output.status.code() == Some(1);
    if !output.status.success() && !findings_with_error_exit {
        bail!(
            "{}",
            format_ast_grep_failure(
                "ast-grep FixConfig rewrite scan failed",
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

    // Convert scan findings to rewrite-like matches. The replacement is the
    // template string; byte offsets are not available from scan findings.
    Ok(findings
        .into_iter()
        .map(|f| AstGrepRewriteMatch {
            file: f.file,
            text: f.text,
            lines: f.lines,
            language: f.language,
            range: f.range,
            meta_variables: None,
            replacement: Some(template.clone()),
            replacement_offsets: None,
        })
        .collect())
}

/// Execute a FixConfig rewrite by generating a temporary YAML rule with
/// `fix` as a `FixConfig` object (template + expandStart/expandEnd) and
/// running `sg scan` against it.
///
/// This is necessary because `sg run --rewrite` only supports simple
/// string fixes. FixConfig with expandStart/expandEnd requires the YAML
/// rule file path.
async fn execute_fixconfig_rewrite(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
    search_path: &ResolvedSearchPath,
) -> Result<Value> {
    let fix_config = request
        .fix_config
        .as_ref()
        .ok_or_else(|| anyhow!("fix_config is required for fixconfig rewrite"))?;
    let pattern = request
        .pattern()
        .ok_or_else(|| anyhow!("pattern is required for fixconfig rewrite"))?;
    let lang = request
        .lang
        .as_deref()
        .filter(|l| !l.trim().is_empty())
        .unwrap_or("javascript");

    // Build the YAML rule content.
    let rule_yaml = build_fixconfig_rule_yaml(
        pattern,
        lang,
        fix_config,
        request.selector.as_deref(),
        request.transform.as_ref(),
    );

    // Create a temporary directory with the rule file and sgconfig.yml.
    let temp_dir = tempfile::tempdir().with_context(|| {
        "failed to create temporary directory for FixConfig rewrite rule".to_string()
    })?;
    let rules_dir = temp_dir.path().join("rules");
    afs::create_dir_all(&rules_dir).await.with_context(|| {
        format!(
            "failed to create rules directory at {}",
            rules_dir.display()
        )
    })?;

    let rule_path = rules_dir.join("fixconfig-rewrite.yml");
    afs::write(&rule_path, &rule_yaml)
        .await
        .with_context(|| format!("failed to write FixConfig rule to {}", rule_path.display()))?;

    let sgconfig_path = temp_dir.path().join("sgconfig.yml");
    let sgconfig_content = format!("ruleDirs:\n  - {}\n", rules_dir.display());
    afs::write(&sgconfig_path, &sgconfig_content)
        .await
        .with_context(|| {
            format!(
                "failed to write sgconfig.yml to {}",
                sgconfig_path.display()
            )
        })?;

    // Run `sg scan` with the temporary config.
    let mut command = ast_grep_command(ast_grep, workspace_root, "scan");
    command
        .arg("--config")
        .arg(&sgconfig_path)
        .arg("--json=stream")
        .arg("--include-metadata")
        .arg("--color=never");

    let globs = request.normalized_globs();
    apply_context_and_globs(
        &mut command,
        request.context_lines,
        request.effective_before_lines(),
        request.effective_after_lines(),
        &globs,
    );
    command.arg(&search_path.command_arg);

    let output = run_ast_grep_command(
        &mut command,
        "failed to run ast-grep FixConfig rewrite scan",
    )
    .await?;

    let findings_with_error_exit = output.status.code() == Some(1);
    if !output.status.success() && !findings_with_error_exit {
        bail!(
            "{}",
            format_ast_grep_failure(
                "ast-grep FixConfig rewrite scan failed",
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

    // Convert scan findings to rewrite-style output.
    Ok(build_fixconfig_rewrite_result(
        request,
        &search_path.display_path,
        findings,
    ))
}

/// Build a YAML rule string for a FixConfig rewrite.
///
/// The rule has:
/// - `id`: a descriptive identifier
/// - `language`: the target language
/// - `severity: info` (rewrite, not a lint warning)
/// - `rule.pattern` or `rule.pattern` + `rule.selector`
/// - `fix`: a FixConfig object with `template` and optional
///   `expandStart`/`expandEnd`
/// - `transform`: optional transform pipeline for meta-variable substitution
fn build_fixconfig_rule_yaml(
    pattern: &str,
    lang: &str,
    fix_config: &FixConfig,
    selector: Option<&str>,
    transform: Option<&Map<String, Value>>,
) -> String {
    let mut yaml = String::new();
    yaml.push_str("id: fixconfig-rewrite\n");
    yaml.push_str(&format!("language: {lang}\n"));
    yaml.push_str("severity: info\n");
    yaml.push_str("rule:\n");

    if let Some(selector) = selector.filter(|s| !s.trim().is_empty()) {
        yaml.push_str(&format!("  pattern: {}\n", yaml_escape_scalar(pattern)));
        yaml.push_str(&format!("  selector: {}\n", yaml_escape_scalar(selector)));
    } else {
        yaml.push_str(&format!("  pattern: {}\n", yaml_escape_scalar(pattern)));
    }

    // Emit transform pipeline before fix so that transformed variables
    // can be referenced in the fix template.
    if let Some(transform) = transform
        && !transform.is_empty()
    {
        yaml.push_str("transform:\n");
        for (var_name, transform_def) in transform {
            use std::fmt::Write as _;
            let _ = writeln!(yaml, "  {}:", var_name);
            value_to_yaml(&mut yaml, transform_def, 4);
        }
    }

    yaml.push_str("fix:\n");
    yaml.push_str(&format!(
        "  template: {}\n",
        yaml_escape_scalar(&fix_config.template)
    ));

    if let Some(expand_start) = &fix_config.expand_start {
        yaml.push_str("  expandStart:\n");
        append_expand_rule_yaml(&mut yaml, expand_start);
    }

    if let Some(expand_end) = &fix_config.expand_end {
        yaml.push_str("  expandEnd:\n");
        append_expand_rule_yaml(&mut yaml, expand_end);
    }

    yaml
}

/// Append expand rule fields to the YAML string, indented at the correct level.
fn append_expand_rule_yaml(yaml: &mut String, rule: &FixExpandRule) {
    if let Some(regex) = &rule.regex {
        yaml.push_str(&format!("    regex: {}\n", yaml_escape_scalar(regex)));
    }
    if let Some(kind) = &rule.kind {
        yaml.push_str(&format!("    kind: {}\n", yaml_escape_scalar(kind)));
    }
    if let Some(pattern) = &rule.pattern {
        yaml.push_str(&format!("    pattern: {}\n", yaml_escape_scalar(pattern)));
    }
    if let Some(stop_by) = &rule.stop_by {
        match stop_by {
            Value::String(s) => {
                yaml.push_str(&format!("    stopBy: {}\n", yaml_escape_scalar(s)));
            }
            Value::Object(_) => {
                // For object stopBy, render as inline JSON-ish YAML.
                // This handles cases like `stopBy: { kind: "," }` or
                // `stopBy: { regex: "," }`.
                yaml.push_str("    stopBy:\n");
                if let Some(obj) = stop_by.as_object() {
                    for (key, val) in obj {
                        match val {
                            Value::String(s) => {
                                yaml.push_str(&format!(
                                    "      {}: {}\n",
                                    key,
                                    yaml_escape_scalar(s)
                                ));
                            }
                            Value::Number(n) => {
                                yaml.push_str(&format!("      {}: {}\n", key, n));
                            }
                            Value::Bool(b) => {
                                yaml.push_str(&format!("      {}: {}\n", key, b));
                            }
                            _ => {
                                yaml.push_str(&format!("      {}: {}\n", key, val));
                            }
                        }
                    }
                }
            }
            _ => {
                yaml.push_str(&format!("    stopBy: {}\n", stop_by));
            }
        }
    }
}

/// Escape a string value for YAML output. Wraps in single quotes if the
/// value contains special YAML characters, and escapes internal single
/// quotes by doubling them.
fn yaml_escape_scalar(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }

    let needs_quoting = value.contains(':')
        || value.contains('#')
        || value.contains('{')
        || value.contains('}')
        || value.contains('[')
        || value.contains(']')
        || value.contains(',')
        || value.contains('&')
        || value.contains('*')
        || value.contains('?')
        || value.contains('|')
        || value.contains('-')
        || value.contains('>')
        || value.contains('!')
        || value.contains('%')
        || value.contains('@')
        || value.contains('`')
        || value.contains('"')
        || value.contains('\'')
        || value.starts_with(' ')
        || value.ends_with(' ')
        || value == "true"
        || value == "false"
        || value == "null"
        || value == "yes"
        || value == "no"
        || value.parse::<f64>().is_ok();

    if needs_quoting {
        let escaped = value.replace('\'', "''");
        format!("'{escaped}'")
    } else {
        value.to_string()
    }
}

/// Build rewrite-style result from scan findings. Converts scan findings
/// into the same shape as rewrite results so callers get a consistent
/// response format regardless of the internal path taken.
fn build_fixconfig_rewrite_result(
    request: &StructuralSearchRequest,
    display_path: &str,
    findings: Vec<AstGrepScanFinding>,
) -> Value {
    let max_results = request.effective_max_results();
    let truncated = findings.len() > max_results;
    let template = request
        .effective_rewrite_template()
        .unwrap_or_default()
        .to_string();

    let normalized_rewrites: Vec<Value> = findings
        .into_iter()
        .take(max_results)
        .map(|finding| {
            let mut rewrite_object = Map::new();
            rewrite_object.insert("file".to_string(), Value::String(finding.file));
            rewrite_object.insert("line_number".to_string(), json!(finding.range.start.line));
            rewrite_object.insert("text".to_string(), Value::String(finding.text.clone()));
            rewrite_object.insert(
                "lines".to_string(),
                Value::String(finding.lines.unwrap_or(finding.text)),
            );
            if let Some(language) = finding.language {
                rewrite_object.insert("language".to_string(), Value::String(language));
            }
            rewrite_object.insert("range".to_string(), build_range_value(&finding.range));
            // The replacement is the FixConfig template. The actual expanded
            // replacement would require applying the rule in-process, which
            // ast-grep handles externally. We include the template as the
            // intended replacement and note the expansion config.
            rewrite_object.insert("replacement".to_string(), Value::String(template.clone()));
            if let Some(message) = finding.message {
                rewrite_object.insert("message".to_string(), Value::String(message));
            }
            Value::Object(rewrite_object)
        })
        .collect();

    let mut result = json!({
        "backend": "ast-grep",
        "workflow": "rewrite",
        "path": display_path,
        "rewrites": normalized_rewrites,
        "truncated": truncated,
        "fix_config": {
            "template": request.fix_config.as_ref().map(|fc| fc.template.clone()),
            "expand_start": request.fix_config.as_ref().and_then(|fc| fc.expand_start.as_ref().map(|es| es.to_yaml_value())),
            "expand_end": request.fix_config.as_ref().and_then(|fc| fc.expand_end.as_ref().map(|ee| ee.to_yaml_value())),
        },
    });
    if let Some(pattern) = request.pattern() {
        result["pattern"] = json!(pattern);
    }
    if let Some(template) = request.effective_rewrite_template() {
        result["rewrite"] = json!(template);
    }
    result
}

fn build_rewrite_fragment_result(
    request: &StructuralSearchRequest,
    display_path: &str,
    hint: String,
) -> Value {
    let next_action = "Retry `unified_search` with `action='structural'` using a larger parseable pattern and `selector` when the real target is a subnode inside that pattern. Do not rerun the same fragment unchanged.";

    let mut result = json!({
        "backend": "ast-grep",
        "workflow": "rewrite",
        "path": display_path,
        "rewrites": [],
        "truncated": false,
        "is_recoverable": true,
        "next_action": next_action,
        "hint": hint,
    });
    if let Some(pattern) = request.pattern() {
        result["pattern"] = json!(pattern);
    }
    if let Some(rewrite_text) = request.rewrite_text() {
        result["rewrite"] = json!(rewrite_text);
    }
    result
}

fn parse_rewrite_matches(stdout: &[u8]) -> Result<Vec<AstGrepRewriteMatch>> {
    serde_json::from_slice(stdout).context("failed to parse ast-grep rewrite JSON output")
}

fn build_rewrite_result(
    request: &StructuralSearchRequest,
    display_path: &str,
    rewrites: Vec<AstGrepRewriteMatch>,
) -> Value {
    let max_results = request.effective_max_results();
    let truncated = rewrites.len() > max_results;
    let normalized_rewrites = rewrites
        .into_iter()
        .take(max_results)
        .map(normalize_rewrite_match)
        .collect::<Vec<_>>();

    let mut result = json!({
        "backend": "ast-grep",
        "workflow": "rewrite",
        "path": display_path,
        "rewrites": normalized_rewrites,
        "truncated": truncated,
    });
    if let Some(pattern) = request.pattern() {
        result["pattern"] = json!(pattern);
    }
    if let Some(rewrite_text) = request.rewrite_text() {
        result["rewrite"] = json!(rewrite_text);
    }
    result
}

fn normalize_rewrite_match(entry: AstGrepRewriteMatch) -> Value {
    let mut rewrite_object = Map::new();
    rewrite_object.insert("file".to_string(), Value::String(entry.file));
    rewrite_object.insert("line_number".to_string(), json!(entry.range.start.line));
    rewrite_object.insert("text".to_string(), Value::String(entry.text.clone()));
    rewrite_object.insert(
        "lines".to_string(),
        Value::String(entry.lines.unwrap_or(entry.text)),
    );
    if let Some(language) = entry.language {
        rewrite_object.insert("language".to_string(), Value::String(language));
    }
    rewrite_object.insert("range".to_string(), build_range_value(&entry.range));
    if let Some(replacement) = entry.replacement {
        rewrite_object.insert("replacement".to_string(), Value::String(replacement));
    }
    if let Some(offsets) = entry.replacement_offsets {
        rewrite_object.insert(
            "replacementOffsets".to_string(),
            json!({ "start": offsets.start, "end": offsets.end }),
        );
    }
    if let Some(meta_vars) = entry.meta_variables {
        rewrite_object.insert(
            "metaVariables".to_string(),
            build_meta_variables_value(&meta_vars),
        );
    }
    Value::Object(rewrite_object)
}

async fn execute_structural_inspect(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
) -> Result<Value> {
    let requested_config = request.requested_config_path();
    let config_path = resolve_config_path(workspace_root, requested_config, false).await?;

    let resolved_full = if Path::new(&config_path.command_arg).is_absolute() {
        PathBuf::from(&config_path.command_arg)
    } else {
        workspace_root.join(&config_path.command_arg)
    };
    let config_exists = resolved_full.is_file();

    let rule_dir_hints = if config_exists {
        extract_rule_dirs(&resolved_full).await
    } else {
        Vec::new()
    };

    let language_injections = if config_exists {
        extract_language_injections(&resolved_full).await
    } else {
        Vec::new()
    };

    let custom_languages = if config_exists {
        extract_custom_languages(&resolved_full).await
    } else {
        Value::Object(Map::new())
    };

    let language_globs = if config_exists {
        extract_language_globs(&resolved_full).await
    } else {
        Value::Object(Map::new())
    };

    let test_configs = if config_exists {
        extract_test_configs(&resolved_full).await
    } else {
        Vec::new()
    };

    let util_dirs = if config_exists {
        extract_util_dirs(&resolved_full).await
    } else {
        Vec::new()
    };

    let discovered = if !config_exists {
        let is_default = requested_config == DEFAULT_AST_GREP_CONFIG_PATH;
        if is_default {
            match discover_project_config(workspace_root).await {
                Some(found) => {
                    let display = found
                        .strip_prefix(workspace_root)
                        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
                        .unwrap_or_else(|_| found.to_string_lossy().replace('\\', "/"));
                    vec![display]
                }
                None => Vec::new(),
            }
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let search_path = resolve_search_path(workspace_root, request.requested_path())?;

    Ok(json!({
        "backend": "ast-grep",
        "workflow": "inspect",
        "project_dir": search_path.display_path,
        "config_path": config_path.display_path,
        "config_exists": config_exists,
        "rule_dir_hints": rule_dir_hints,
        "test_configs": test_configs,
        "util_dirs": util_dirs,
        "language_injections": language_injections,
        "custom_languages": custom_languages,
        "language_globs": language_globs,
        "discovered_configs": discovered,
    }))
}

async fn execute_structural_rules(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
) -> Result<Value> {
    let requested_config = request.requested_config_path();
    let config_path = resolve_config_path(workspace_root, requested_config, false).await?;

    let resolved_full = if Path::new(&config_path.command_arg).is_absolute() {
        PathBuf::from(&config_path.command_arg)
    } else {
        workspace_root.join(&config_path.command_arg)
    };
    let config_exists = resolved_full.is_file();

    if !config_exists {
        return Ok(json!({
            "backend": "ast-grep",
            "workflow": "rules",
            "config_path": config_path.display_path,
            "config_exists": false,
            "rules": [],
        }));
    }

    let rule_dirs = extract_rule_dirs(&resolved_full).await;
    let config_parent = resolved_full.parent().unwrap_or(workspace_root);

    let mut rules = Vec::new();
    for dir in &rule_dirs {
        let dir_path = config_parent.join(dir);
        if !dir_path.is_dir() {
            continue;
        }
        collect_rules_from_dir(&dir_path, &mut rules).await;
    }

    Ok(json!({
        "backend": "ast-grep",
        "workflow": "rules",
        "config_path": config_path.display_path,
        "config_exists": true,
        "rule_dirs": rule_dirs,
        "rules": rules,
    }))
}

async fn execute_structural_new(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
) -> Result<Value> {
    let subcommand = request
        .new_subcommand
        .as_deref()
        .expect("new_subcommand validated present");

    let mut command = ast_grep_command(ast_grep, workspace_root, "new");
    command.arg(subcommand).arg("--yes");

    if let Some(name) = request.new_name.as_deref().filter(|s| !s.trim().is_empty()) {
        command.arg(name);
    }

    if let Some(lang) = request.lang.as_deref().filter(|s| !s.trim().is_empty()) {
        command.arg("--lang").arg(lang);
    }

    if let Some(config) = request
        .config_path
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        command.arg("--config").arg(config);
    }

    let output = run_ast_grep_command(&mut command, "failed to run ast-grep new").await?;

    if !output.status.success() {
        bail!(
            "{}",
            format_ast_grep_failure(
                "ast-grep new failed",
                stderr_or_stdout(&output.stderr, &output.stdout)
            )
        );
    }

    Ok(json!({
        "backend": "ast-grep",
        "workflow": "new",
        "subcommand": subcommand,
        "name": request.new_name,
        "output": String::from_utf8_lossy(&output.stdout).trim(),
    }))
}

async fn execute_structural_apply(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
) -> Result<Value> {
    let search_path = resolve_search_path(workspace_root, request.requested_path())?;
    let globs = request.normalized_globs();

    if let Some(hint) = preflight_parseable_pattern(request)? {
        return Ok(json!({
            "backend": "ast-grep",
            "workflow": "apply",
            "path": search_path.display_path,
            "files_modified": [],
            "total_replacements": 0,
            "is_recoverable": true,
            "hint": hint,
        }));
    }

    let needs_yaml_rewrite = request
        .fix_config
        .as_ref()
        .is_some_and(|fc| fc.has_expansion())
        || request.transform.is_some();

    let rewrites: Vec<AstGrepRewriteMatch> = if needs_yaml_rewrite {
        execute_fixconfig_rewrite_to_matches(workspace_root, request, ast_grep, &search_path)
            .await?
    } else {
        let pattern = request
            .pattern()
            .ok_or_else(|| anyhow!("pattern is required for apply"))?;
        let template = request
            .effective_rewrite_template()
            .ok_or_else(|| anyhow!("rewrite template is required for apply"))?;
        let command_path = search_path.command_arg.clone();
        let mut command = ast_grep_command(ast_grep, workspace_root, "run");
        command
            .arg(format!("--pattern={}", pattern))
            .arg(format!("--rewrite={}", template))
            .arg("--json=compact")
            .arg("--color=never");

        if let Some(lang) = request.lang.as_deref().filter(|s| !s.trim().is_empty()) {
            command.arg("--lang").arg(lang);
        }
        if let Some(selector) = request.selector.as_deref().filter(|s| !s.trim().is_empty()) {
            command.arg("--selector").arg(selector);
        }
        if let Some(strictness) = &request.strictness {
            command.arg("--strictness").arg(strictness.as_str());
        }
        apply_context_and_globs(
            &mut command,
            request.context_lines,
            request.effective_before_lines(),
            request.effective_after_lines(),
            &globs,
        );
        command.arg(&command_path);

        let output =
            run_ast_grep_command(&mut command, "failed to run ast-grep structural apply").await?;

        let no_matches = output.status.code() == Some(1);
        if !output.status.success() && !no_matches {
            bail!(
                "{}",
                format_ast_grep_failure(
                    "ast-grep structural apply failed",
                    stderr_or_stdout(&output.stderr, &output.stdout)
                )
            );
        }

        if no_matches && String::from_utf8_lossy(&output.stdout).trim().is_empty() {
            Vec::new()
        } else {
            parse_rewrite_matches(&output.stdout)?
        }
    };

    if rewrites.is_empty() {
        return Ok(json!({
            "backend": "ast-grep",
            "workflow": "apply",
            "path": search_path.display_path,
            "files_modified": [],
            "total_replacements": 0,
        }));
    }

    // Group rewrites by file.
    let mut by_file: BTreeMap<String, Vec<&AstGrepRewriteMatch>> = BTreeMap::new();
    for rw in &rewrites {
        by_file.entry(rw.file.clone()).or_default().push(rw);
    }

    let mut files_modified = Vec::new();
    let mut total_replacements = 0usize;

    for (file_path, file_rewrites) in &by_file {
        let abs_path = workspace_root.join(file_path);
        let content = afs::read_to_string(&abs_path)
            .await
            .with_context(|| format!("failed to read {file_path} for apply"))?;
        let mut bytes = content.into_bytes();

        // Sort by byte offset descending so we apply from end to start.
        let mut sorted: Vec<_> = file_rewrites.iter().collect();
        sorted.sort_by(|a, b| {
            let a_start = a.replacement_offsets.as_ref().map(|o| o.start).unwrap_or(0);
            let b_start = b.replacement_offsets.as_ref().map(|o| o.start).unwrap_or(0);
            b_start.cmp(&a_start)
        });

        let mut applied = 0usize;
        for rw in &sorted {
            let Some(replacement) = &rw.replacement else {
                continue;
            };
            let Some(offsets) = &rw.replacement_offsets else {
                continue;
            };
            if offsets.start > offsets.end || offsets.end > bytes.len() {
                continue;
            }
            let replacement_bytes = replacement.as_bytes();
            bytes.splice(
                offsets.start..offsets.end,
                replacement_bytes.iter().cloned(),
            );
            applied += 1;
        }

        if applied > 0 {
            afs::write(&abs_path, &bytes)
                .await
                .with_context(|| format!("failed to write {file_path}"))?;
            total_replacements += applied;
            files_modified.push(json!({
                "file": file_path,
                "replacements": applied,
            }));
        }
    }

    Ok(json!({
        "backend": "ast-grep",
        "workflow": "apply",
        "path": search_path.display_path,
        "files_modified": files_modified,
        "total_replacements": total_replacements,
    }))
}

/// Recursively collect rule summaries from YAML files in a directory.
async fn collect_rules_from_dir(dir: &Path, rules: &mut Vec<Value>) {
    let mut entries = match afs::read_dir(dir).await {
        Ok(e) => e,
        Err(_) => return,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.is_dir() {
            Box::pin(collect_rules_from_dir(&path, rules)).await;
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !matches!(ext, "yml" | "yaml") {
            continue;
        }
        let Ok(content) = afs::read_to_string(&path).await else {
            continue;
        };
        if let Some(summary) = extract_rule_summary(&content, &path) {
            rules.push(summary);
        }
    }
}

/// Extract a rule summary from a YAML file's content.
fn extract_rule_summary(content: &str, path: &Path) -> Option<Value> {
    let mut id = None;
    let mut language = None;
    let mut severity = None;
    let mut message = None;
    let mut has_fix = false;
    let mut utils_list = Vec::new();
    let mut in_utils = false;
    let mut utils_indent = 0usize;

    for line in content.lines() {
        let trimmed = line.trim();
        // Track whether we are inside the top-level `utils:` section.
        // We stay in the utils section until we hit a new top-level key
        // (a line at the same indentation as `utils:` that contains a colon).
        if in_utils {
            let indent = line.len() - line.trim_start().len();
            if indent <= utils_indent && !trimmed.is_empty() && !trimmed.starts_with('#') {
                // Hit a new top-level key; exit the utils section.
                in_utils = false;
            } else if indent == utils_indent + 2
                && let Some(name) = trimmed.strip_suffix(':')
            {
                let name = name.trim();
                if !name.is_empty() && !name.contains(' ') && !name.starts_with('#') {
                    utils_list.push(name.to_string());
                }
            }
        }
        if trimmed == "utils:" && !in_utils {
            in_utils = true;
            utils_indent = line.len() - line.trim_start().len();
        }

        if trimmed.starts_with("id:") && id.is_none() {
            id = Some(trimmed.strip_prefix("id:")?.trim().to_string());
        } else if trimmed.starts_with("language:") && language.is_none() {
            language = Some(trimmed.strip_prefix("language:")?.trim().to_string());
        } else if trimmed.starts_with("severity:") && severity.is_none() {
            severity = Some(trimmed.strip_prefix("severity:")?.trim().to_string());
        } else if trimmed.starts_with("message:") && message.is_none() {
            message = Some(
                trimmed
                    .strip_prefix("message:")?
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string(),
            );
        } else if trimmed.starts_with("fix:") && !has_fix {
            has_fix = true;
        }
    }

    let id = id?;
    let file_name = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut summary = json!({
        "id": id,
        "file": file_name,
    });
    if let Some(lang) = language {
        summary["language"] = json!(lang);
    }
    if let Some(sev) = severity {
        summary["severity"] = json!(sev);
    }
    if let Some(msg) = message {
        summary["message"] = json!(msg);
    }
    summary["has_fix"] = json!(has_fix);
    if !utils_list.is_empty() {
        summary["utils"] = json!(utils_list);
    }

    Some(summary)
}

fn preflight_parseable_pattern(request: &StructuralSearchRequest) -> Result<Option<String>> {
    let Some(language) = request
        .lang
        .as_deref()
        .and_then(AstGrepLanguage::from_user_value)
    else {
        return Ok(None);
    };

    if !language.has_local_parser() {
        return Ok(None);
    }

    let pattern = request.pattern().expect("query pattern validated");
    validate_metavariable_syntax(pattern)?;

    let (sanitized_pattern, contains_metavariables) = sanitize_pattern_for_tree_sitter(pattern);
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
            let matched = captures.get(0).map(|m| m.as_str()).unwrap_or("");
            // `$$$NAME` (multi-metavariable) and `$$NAME` (unnamed node)
            // both need multiple placeholders; `$NAME` (named node) gets one.
            if matched.starts_with("$$") {
                "placeholders"
            } else {
                "placeholder"
            }
        });

    (sanitized.into_owned(), contains_metavariables)
}

/// Validate metavariable syntax in an ast-grep pattern.
fn validate_metavariable_syntax(pattern: &str) -> Result<()> {
    static AST_GREP_DOLLAR_TOKEN_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"\$\$?\$?[A-Za-z0-9_]+").expect("ast-grep dollar token regex must compile")
    });

    for mat in AST_GREP_DOLLAR_TOKEN_RE.find_iter(pattern) {
        let token = mat.as_str();
        if !AST_GREP_VALID_METAVAR_RE.is_match(token) {
            if let Some(rest) = token.strip_prefix("$$$") {
                if rest.is_empty()
                    || !rest
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_uppercase() || c == '_')
                {
                    bail!(
                        "invalid metavariable `{token}`: multi-metavariable `$$$` must be followed by an uppercase name (e.g. `$$$ARGS`); got `{rest}`"
                    );
                }
            } else if token == "$" || token == "$$" {
                bail!(
                    "bare `{token}` is not a valid metavariable; use `$NAME` (named node), `$$NAME` (unnamed node), or `$$$NAME` (zero or more nodes)"
                );
            } else {
                let prefix = if token.starts_with("$$") { "$$" } else { "$" };
                let name = &token[prefix.len()..];
                if name.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
                    bail!(
                        "invalid metavariable `{token}`: names must start with an uppercase letter                          or underscore; use `{prefix}{upper}` instead",
                        upper = name.to_ascii_uppercase()
                    );
                }
                bail!(
                    "invalid metavariable `{token}`: names must match `[A-Z_][A-Z0-9_]*` after the `$` or `$$` prefix"
                );
            }
        }
    }
    Ok(())
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

fn apply_context_and_globs(
    command: &mut Command,
    context_lines: Option<usize>,
    before_lines: Option<usize>,
    after_lines: Option<usize>,
    globs: &[String],
) {
    if let Some(before) = before_lines {
        command.arg("--before").arg(before.to_string());
    }
    if let Some(after) = after_lines {
        command.arg("--after").arg(after.to_string());
    }
    // Symmetric context only when before/after are not set (validated upstream).
    if before_lines.is_none()
        && after_lines.is_none()
        && let Some(context_lines) = context_lines
    {
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
    let mut result = json!({
        "backend": "ast-grep",
        "path": display_path,
        "lang": request.lang,
        "debug_query": debug_query.as_str(),
        "debug_query_output": truncate_auxiliary_output(String::from_utf8_lossy(stdout).trim()),
        "matches": [],
        "truncated": false,
    });
    if let Some(pattern) = request.pattern() {
        result["pattern"] = json!(pattern);
    }
    if let Some(kind) = request.kind() {
        result["kind"] = json!(kind);
    }
    result
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

    let mut result = json!({
        "backend": "ast-grep",
        "path": display_path,
        "matches": normalized_matches,
        "truncated": truncated,
    });
    if let Some(pattern) = request.pattern() {
        result["pattern"] = json!(pattern);
    }
    if let Some(kind) = request.kind() {
        result["kind"] = json!(kind);
    }
    result
}

fn build_scan_result(
    request: &StructuralSearchRequest,
    display_path: &str,
    config_display_path: &str,
    findings: Vec<AstGrepScanFinding>,
) -> Value {
    // Apply post-run severity filter when severities are specified.
    let filtered_findings = if let Some(severities) = request.effective_severities() {
        findings
            .into_iter()
            .filter(|f| {
                f.severity
                    .as_ref()
                    .map(|s| severities.contains(&s.as_str()))
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>()
    } else {
        findings
    };

    let max_results = request.effective_max_results();
    let truncated = filtered_findings.len() > max_results;
    let normalized_findings = filtered_findings
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
        "summary": build_scan_summary(&filtered_findings, normalized_findings.len(), truncated),
        "truncated": truncated,
    })
}

fn build_fragment_result(
    request: &StructuralSearchRequest,
    display_path: &str,
    hint: String,
) -> Value {
    let next_action = "Retry `unified_search` with `action='structural'` using a larger parseable pattern and `selector` when the real target is a subnode inside that pattern. Do not rerun the same fragment unchanged.";

    let mut result = json!({
        "backend": "ast-grep",
        "path": display_path,
        "matches": [],
        "truncated": false,
        "is_recoverable": true,
        "next_action": next_action,
        "hint": hint,
    });
    if let Some(pattern) = request.pattern() {
        result["pattern"] = json!(pattern);
    }
    if let Some(kind) = request.kind() {
        result["kind"] = json!(kind);
    }
    result
}

fn build_range_value(range: &AstGrepRange) -> Value {
    let mut range_object = json!({
        "start": {
            "line": range.start.line,
            "column": range.start.column,
        },
        "end": {
            "line": range.end.line,
            "column": range.end.column,
        },
    });
    if let Some(byte_offset) = &range.byte_offset {
        range_object["byteOffset"] = json!({
            "start": byte_offset.start,
            "end": byte_offset.end,
        });
    }
    range_object
}

fn build_meta_var_value(var: &AstGrepMetaVar) -> Value {
    json!({
        "text": var.text,
        "range": build_range_value(&var.range),
    })
}

fn build_meta_variables_value(meta_vars: &AstGrepMetaVariables) -> Value {
    let mut single = Map::new();
    for (key, var) in &meta_vars.single {
        single.insert(key.clone(), build_meta_var_value(var));
    }

    let mut multi = Map::new();
    for (key, vars) in &meta_vars.multi {
        let vars_json: Vec<Value> = vars.iter().map(build_meta_var_value).collect();
        multi.insert(key.clone(), Value::Array(vars_json));
    }

    let transformed = if meta_vars.transformed.is_empty() {
        Value::Object(Map::new())
    } else {
        json!(meta_vars.transformed)
    };

    json!({
        "single": Value::Object(single),
        "multi": Value::Object(multi),
        "transformed": transformed,
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
    match_object.insert("range".to_string(), build_range_value(&entry.range));
    if let Some(meta_vars) = entry.meta_variables {
        match_object.insert(
            "metaVariables".to_string(),
            build_meta_variables_value(&meta_vars),
        );
    }
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
    finding_object.insert("range".to_string(), build_range_value(&entry.range));
    finding_object.insert("rule_id".to_string(), json!(entry.rule_id));
    finding_object.insert(
        "severity".to_string(),
        json!(entry.severity.map(|s| s.to_string())),
    );
    finding_object.insert("message".to_string(), json!(entry.message));
    finding_object.insert("note".to_string(), json!(entry.note));
    if let Some(metadata) = &entry.metadata {
        if let Some(url) = metadata.get("url").or_else(|| metadata.get("docs")) {
            finding_object.insert("url".to_string(), url.clone());
        }
        finding_object.insert("metadata".to_string(), metadata.clone());
    }
    if !entry.labels.is_empty() {
        let labels_json: Vec<Value> = entry
            .labels
            .iter()
            .map(|label| {
                let mut label_obj = Map::new();
                label_obj.insert("text".to_string(), Value::String(label.text.clone()));
                label_obj.insert("range".to_string(), build_range_value(&label.range));
                if let Some(source) = &label.source {
                    label_obj.insert("source".to_string(), Value::String(source.clone()));
                }
                Value::Object(label_obj)
            })
            .collect();
        finding_object.insert("labels".to_string(), Value::Array(labels_json));
    }
    Value::Object(finding_object)
}

fn build_scan_summary(findings: &[AstGrepScanFinding], returned: usize, truncated: bool) -> Value {
    let mut by_severity = BTreeMap::new();
    let mut by_rule = BTreeMap::new();
    let mut has_error_findings = false;

    for finding in findings {
        let severity = finding
            .severity
            .as_ref()
            .map(|s| s.as_str())
            .unwrap_or("unknown")
            .to_string();
        if severity == "error" {
            has_error_findings = true;
        }
        *by_severity.entry(severity).or_insert(0usize) += 1;

        let rule = finding.rule_id.as_deref().unwrap_or("unknown").to_string();
        *by_rule.entry(rule).or_insert(0usize) += 1;
    }

    json!({
        "total_findings": findings.len(),
        "returned_findings": returned,
        "truncated": truncated,
        "has_error_findings": has_error_findings,
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
        "summary": summarize_test_output(&stdout, &stderr, passed),
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

#[cold]
fn format_ast_grep_failure(prefix: &str, detail: String) -> String {
    let needs_project_config_hint = looks_like_language_support_issue(&detail);
    let mut message = format!("{prefix}: {detail}. {AST_GREP_FAQ_HINT}");
    if needs_project_config_hint {
        message.push(' ');
        message.push_str(AST_GREP_PROJECT_CONFIG_HINT);
    }
    message.push_str(
        " Retry `unified_search` with a refined structural pattern before switching tools. For simple rewrites, use `workflow='rewrite'` on the public structural surface. For FixConfig rewrites with range expansion, use `workflow='rewrite'` with `fix_config` on the public surface. For `sg scan`, `sg test`, `sg new`, `sgconfig.yml`, or advanced rewrite-oriented ast-grep tasks with `transform` or `rewriters`, load the bundled `ast-grep` skill first and use `unified_exec` only when the public structural surface and skill guidance still cannot express the needed CLI flow.",
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

/// Returns true when a Go pattern looks like a bare function call that
/// tree-sitter-go would parse as a type conversion (e.g. `fmt.Println($A)`
/// or `json.Unmarshal($$$)`). These patterns need `context` + `selector`
/// to disambiguate.
fn looks_like_go_call_pattern(pattern: &str) -> bool {
    // Match patterns like `pkg.Func($$$)`, `Func($$$)`, or
    // `expr.Method($$$)` where the pattern starts with an identifier
    // chain followed by parenthesized arguments. Metavariable prefixes
    // (`$`, `$$`, `$$$`) are stripped before checking identifier validity
    // so patterns like `$A.$B($$$)` are recognized as call patterns.
    let trimmed = pattern.trim();
    let Some(paren) = trimmed.find('(') else {
        return false;
    };
    if paren == 0 || !trimmed.ends_with(')') {
        return false;
    }
    let callee = &trimmed[..paren];
    // Callee must look like an identifier chain: `Func`, `pkg.Func`,
    // `pkg.Sub.Method`, etc. Strip metavariable prefixes so `$A.$B`
    // is treated like `A.B`.
    callee.split('.').all(|part| {
        let stripped = part.trim_start_matches('$');
        !stripped.is_empty()
            && stripped
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

fn looks_like_html_attribute_pattern(pattern: &str) -> bool {
    // Match patterns like `class=$VAL`, `id=$ID`, `href=$URL` where the
    // pattern looks like an HTML attribute assignment without surrounding
    // element context.
    let trimmed = pattern.trim();
    if trimmed.contains('<') || trimmed.contains('>') {
        return false;
    }
    let Some(eq) = trimmed.find('=') else {
        return false;
    };
    let attr_name = &trimmed[..eq];
    // Attribute name must be a valid HTML attribute name (letters, digits,
    // hyphens, underscores, colons for namespaced attrs).
    !attr_name.is_empty()
        && attr_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':')
}

fn looks_like_html_tag_pattern(pattern: &str) -> bool {
    // Match patterns like `<$TAG>`, `<div>`, `<$TAG $$$ATTRS>` that look
    // like HTML opening tags without a closing tag or body.
    let trimmed = pattern.trim();
    trimmed.starts_with('<')
        && trimmed.contains('>')
        && !trimmed.contains("</")
        && !trimmed.ends_with("/>")
}

/// Returns true when a Java pattern looks like a bare type-qualified
/// identifier or field declaration fragment that tree-sitter-java would
/// fail to parse as standalone code. Common examples:
/// - `$MOD String $F` (modifier + type + name without surrounding class)
/// - `@Annotation` (bare annotation without surrounding declaration)
/// - `$TYPE $VAR;` fragments that need class-body context
fn looks_like_java_declaration_fragment(pattern: &str) -> bool {
    let trimmed = pattern.trim();
    // Bare annotation: `@Foo` or `@Foo($$$)`
    if trimmed.starts_with('@') {
        return true;
    }
    // Patterns with semicolons that look like field/variable declarations
    // without class context: `String $F;`, `private $TYPE $NAME;`
    if trimmed.ends_with(';') {
        // Contains a type-like identifier followed by a metavariable
        let inner = trimmed.trim_end_matches(';').trim();
        let parts: Vec<&str> = inner.split_whitespace().collect();
        if parts.len() >= 2 {
            // Last part should look like a metavariable or identifier
            let last = parts.last().expect("parts.len() >= 2 guarantees non-empty");
            if last.starts_with('$') || last.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return true;
            }
        }
    }
    false
}

/// Returns true when a Ruby pattern looks like a bare block or pipe fragment
/// that tree-sitter-ruby would fail to parse as standalone code. Common
/// examples:
/// - `{ |$V| $V.$METHOD }` (block with pipe parameters)
/// - `do |$V| $V.$METHOD end` (do-block with pipe parameters)
/// - `&:$METHOD` (bare symbol-to-proc)
fn looks_like_css_selector_fragment(pattern: &str) -> bool {
    let trimmed = pattern.trim();
    if trimmed.starts_with('.') && trimmed.len() > 1 && !trimmed.contains('{') {
        return true;
    }
    if trimmed.starts_with('#') && trimmed.len() > 1 && !trimmed.contains('{') {
        return true;
    }
    false
}

fn looks_like_python_decorator_fragment(pattern: &str) -> bool {
    let trimmed = pattern.trim();
    if trimmed.starts_with('@') && trimmed.len() > 1 && !trimmed.contains('\n') {
        let rest = &trimmed[1..];
        return rest
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    }
    false
}

fn looks_like_ruby_block_fragment(pattern: &str) -> bool {
    let trimmed = pattern.trim();

    // Bare symbol-to-proc: `&:method_name`
    if trimmed.starts_with('&') && trimmed.len() > 1 {
        let after = &trimmed[1..];
        if after.starts_with(':') && after.len() > 1 {
            return true;
        }
    }

    // Bare pipe block: `{ |$V| ... }` or `do |$V| ... end`
    if trimmed.starts_with('{') && trimmed.contains('|') {
        return true;
    }
    if trimmed.starts_with("do") && trimmed.contains('|') {
        return true;
    }

    // Bare block body starting with pipe: `| $V | $V.$METHOD`
    if trimmed.starts_with('|') {
        return true;
    }

    false
}

/// Detect patterns that look like Rust method calls without a receiver,
/// e.g. `unwrap_or($T::default())`, `map_err($E)`, `and_then($C)`.
/// These fail tree-sitter preflight because `.method()` calls require a
/// receiver in Rust syntax. The correct ast-grep form is `$X.method($A)`.
///
/// This only fires when the full pattern contains metavariables, because
/// plain `foo()` parses fine as a function call and never reaches this
/// code path.
fn looks_like_rust_method_call_fragment(pattern: &str) -> bool {
    let trimmed = pattern.trim();
    // Must not start with `$` — that would already be a receiver.
    if trimmed.starts_with('$') {
        return false;
    }
    // Must end with `)` to look like a call.
    if !trimmed.ends_with(')') {
        return false;
    }
    // Must contain a metavariable — otherwise it's a plain expression
    // that tree-sitter can parse and this code path is never reached.
    if !trimmed.contains('$') {
        return false;
    }
    let Some(paren) = trimmed.find('(') else {
        return false;
    };
    if paren == 0 {
        return false;
    }
    let callee = &trimmed[..paren];
    // Callee must be a simple identifier — no dots (receiver.method or
    // path::method) and no colons (associated function Type::method).
    !callee.is_empty()
        && !callee.contains('.')
        && !callee.contains("::")
        && callee
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn fragment_pattern_hint(request: &StructuralSearchRequest, language: AstGrepLanguage) -> String {
    let Some(trimmed) = request.pattern() else {
        return format!(
            "Pattern is required for {} syntax queries.",
            language.display_name()
        );
    };
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
    } else if language == AstGrepLanguage::Rust && looks_like_rust_method_call_fragment(trimmed) {
        message.push_str(
            " In Rust, method calls like `unwrap_or($T)` need a receiver. \
             Use `$X.unwrap_or($T::default())` to match method calls on any receiver, \
             where `$X` captures the receiver expression. \
             For associated functions like `Type::method($A)`, use the full qualified path in the pattern.",
        );
    } else if language == AstGrepLanguage::Go && looks_like_go_call_pattern(trimmed) {
        message.push_str(
            " In Go, tree-sitter parses bare call-like fragments (e.g. `fmt.Println($A)`) as type conversions, not call expressions. \
             Wrap the call in surrounding parseable code like `func t() { fmt.Println($A) }` and use `selector: call_expression` to match only function calls. \
             Note: contextual patterns with `context` + `selector` require the CLI skill path via `unified_exec`.",
        );
    } else if language == AstGrepLanguage::Html && looks_like_html_attribute_pattern(trimmed) {
        message.push_str(
            " In HTML, bare attribute expressions like `class=$VAL` are not standalone parseable code. \
             Use `kind: attribute_name` to match attribute names, `kind: attribute_value` for values, \
             or `kind: element` with `has` to match elements containing specific attributes. \
             For example, to match elements with a specific attribute, use `kind: element` with \
             `has: { kind: attribute_name, regex: \"^class$\" }`.",
        );
    } else if language == AstGrepLanguage::Html && looks_like_html_tag_pattern(trimmed) {
        message.push_str(
            " In HTML, tree-sitter parses tag structures as `element` nodes with `tag_name` and `attribute` children. \
             Bare tag fragments like `<$TAG>` are not standalone code. Use `kind: element` with \
             `has: { field: tag_name, pattern: $TAG }` to match elements by tag name, \
             or `kind: tag_name` to match tag name nodes directly.",
        );
    } else if language == AstGrepLanguage::Java && looks_like_java_declaration_fragment(trimmed) {
        message.push_str(
            " In Java, bare type declarations, annotations, and field fragments are not standalone parseable code. \
             For field or variable declarations with modifiers/annotations, use `kind: field_declaration` with \
             `has: { field: type, regex: \"^TypeName$\" }` to match by type regardless of modifiers. \
             For annotations, use `kind: marker_annotation` or `kind: annotation` with `inside` to scope \
             to the declaration you care about. Wrap bare fragments in a full class body like \
             `class _ { $TYPE $VAR; }` and use `selector` to target the inner node.",
        );
    } else if language == AstGrepLanguage::Ruby && looks_like_ruby_block_fragment(trimmed) {
        message.push_str(
            " In Ruby, bare block fragments like `{ |$V| $V.$METHOD }` or `do |$V| $V.$METHOD end` are not \
             standalone parseable code. Wrap the block in a method call like `$LIST.select { |$V| $V.$METHOD }` \
             and use `selector: call` to match the outer call. For symbol-to-proc patterns, match the enclosing \
             method call directly with `$LIST.$ITER(&:$METHOD)`. Key Ruby tree-sitter node kinds: `call` for \
             method calls, `method_call` for keyword-style calls, `block` for `{{ }}` blocks, `do_block` for \
             `do...end` blocks, `symbol` for `:name` literals, `assignment` for variable assignments.",
        );
    } else if language == AstGrepLanguage::Css && looks_like_css_selector_fragment(trimmed) {
        message.push_str(
            " In CSS, bare selectors like `.class` or `#id` are not standalone parseable code. \
             Use `kind: rule_set` with `has` to match rule sets containing specific selectors, or \
             `kind: selector` to match selector nodes.",
        );
    } else if language == AstGrepLanguage::Python && looks_like_python_decorator_fragment(trimmed) {
        message.push_str(
            " In Python, bare decorators like `@property` are not standalone parseable code. \
             Wrap with the decorated definition and use `selector: decorated_definition` to match.",
        );
    } else if language == AstGrepLanguage::Bash && !trimmed.contains(';') {
        message.push_str(
            " In Bash, bare command fragments need script context. Use `kind: command` with `has` to match specific commands.",
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
    require_exists: bool,
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
    let workspace_root = tokio::fs::canonicalize(workspace_root)
        .await
        .with_context(|| {
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

    if require_exists && !resolved.is_file() {
        let is_default = requested_path == DEFAULT_AST_GREP_CONFIG_PATH;
        let discovered = if is_default {
            discover_project_config(&workspace_root).await
        } else {
            None
        };
        bail!(
            "{}",
            format_missing_config_error(
                requested_path,
                is_default,
                &resolved,
                discovered.as_deref()
            )
        );
    }

    build_resolved_workspace_path(&workspace_root, resolved)
}

/// Walk up from `start` looking for `sgconfig.yml` in ancestor directories.
async fn discover_project_config(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start.to_path_buf());
    while let Some(dir) = current {
        let candidate = dir.join(DEFAULT_AST_GREP_CONFIG_PATH);
        if candidate.is_file() {
            return Some(candidate);
        }
        current = dir.parent().map(Path::to_path_buf);
    }
    None
}

fn format_missing_config_error(
    requested: &str,
    is_default: bool,
    resolved: &Path,
    discovered: Option<&Path>,
) -> String {
    let mut message = if is_default {
        format!(
            "ast-grep project config `{}` not found at {}. \
             `workflow=\"scan\"` and `workflow=\"test\"` require a project config file. \
             Create `{}` with at least:\n\n  ruleDirs:\n    - rules\n\n\
             Then place rule YAML files in the `rules/` directory. \
             Or scaffold a full project with `ast-grep new project`. \
             For config authoring, load the bundled `ast-grep` skill.",
            requested,
            resolved.display(),
            requested,
        )
    } else {
        format!(
            "ast-grep project config not found at `{}` (resolved to {}). \
             Verify the `config_path` is correct and the file exists. \
             For config authoring, load the bundled `ast-grep` skill.",
            requested,
            resolved.display(),
        )
    };

    if let Some(found) = discovered {
        message.push_str(&format!(
            "\n\nNote: found `{}` at {}. \
             Set `config_path` to that path to use it, or create a local `{}` in the workspace root.",
            DEFAULT_AST_GREP_CONFIG_PATH,
            found.display(),
            DEFAULT_AST_GREP_CONFIG_PATH,
        ));
    }

    message
}

/// Best-effort extraction of `ruleDirs` entries from a sgconfig.yml file.
/// Returns relative directory paths found under the `ruleDirs:` key.
async fn extract_rule_dirs(config_path: &Path) -> Vec<String> {
    let Ok(content) = afs::read_to_string(config_path).await else {
        return Vec::new();
    };

    let mut dirs = Vec::new();
    let mut in_rule_dirs = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("ruleDirs:") {
            in_rule_dirs = true;
            // Handle inline array: ruleDirs: [rules, custom-rules]
            if let Some(bracket_content) = trimmed.strip_prefix("ruleDirs:").map(str::trim)
                && bracket_content.starts_with('[')
            {
                let inner = bracket_content.trim_matches(|c| c == '[' || c == ']');
                for item in inner.split(',') {
                    let item = item.trim().trim_matches('"').trim_matches('\'');
                    if !item.is_empty() {
                        dirs.push(item.to_string());
                    }
                }
                in_rule_dirs = false;
            }
            continue;
        }

        if in_rule_dirs {
            if trimmed.starts_with('-') {
                let item = trimmed
                    .strip_prefix('-')
                    .unwrap_or(trimmed)
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'');
                if !item.is_empty() {
                    dirs.push(item.to_string());
                }
            } else if !trimmed.is_empty() && !trimmed.starts_with('#') {
                // Hit a new key, stop collecting
                in_rule_dirs = false;
            }
        }
    }

    dirs
}

/// Best-effort extraction of `testConfigs` entries from a sgconfig.yml file.
/// Returns a list of objects with `testDir` (required) and `snapshotDir` (optional).
async fn extract_test_configs(config_path: &Path) -> Vec<Value> {
    let Ok(content) = afs::read_to_string(config_path).await else {
        return Vec::new();
    };

    let mut configs = Vec::new();
    let mut in_test_configs = false;
    let mut current_item: Option<Map<String, Value>> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("testConfigs:") {
            in_test_configs = true;
            if trimmed.contains('[') {
                break; // Inline array of objects is too complex for line-by-line parsing
            }
            continue;
        }

        if !in_test_configs {
            continue;
        }

        // List item start: "- "
        if trimmed.starts_with("- ") {
            // Flush previous item
            if let Some(item) = current_item.take() {
                configs.push(Value::Object(item));
            }
            current_item = Some(Map::new());
            // Check for inline key-value: "- testDir: tests"
            let after_dash = trimmed.strip_prefix("- ").unwrap_or(trimmed).trim();
            if let Some((key, value)) = parse_yaml_simple_kv(after_dash)
                && let Some(ref mut item) = current_item
            {
                item.insert(key, value);
            }
            continue;
        }

        // A new top-level key (not a list item) ends the section
        if !line.starts_with(' ') && !line.starts_with('\t') && !trimmed.is_empty() {
            if let Some(item) = current_item.take() {
                configs.push(Value::Object(item));
            }
            in_test_configs = false;
            continue;
        }

        // Key-value inside a list item (indented deeper than "- ")
        if let Some(ref mut item) = current_item
            && let Some((key, value)) = parse_yaml_simple_kv(trimmed)
        {
            item.insert(key, value);
        }
    }

    // Flush last item
    if let Some(item) = current_item {
        configs.push(Value::Object(item));
    }

    configs
}

/// Best-effort extraction of `utilDirs` entries from a sgconfig.yml file.
/// Returns relative directory paths found under the `utilDirs:` key.
async fn extract_util_dirs(config_path: &Path) -> Vec<String> {
    let Ok(content) = afs::read_to_string(config_path).await else {
        return Vec::new();
    };

    let mut dirs = Vec::new();
    let mut in_util_dirs = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("utilDirs:") {
            in_util_dirs = true;
            // Handle inline array: utilDirs: [utils, shared]
            if let Some(bracket_content) = trimmed.strip_prefix("utilDirs:").map(str::trim)
                && bracket_content.starts_with('[')
            {
                let inner = bracket_content.trim_matches(|c| c == '[' || c == ']');
                for item in inner.split(',') {
                    let item = item.trim().trim_matches('"').trim_matches('\'');
                    if !item.is_empty() {
                        dirs.push(item.to_string());
                    }
                }
                in_util_dirs = false;
            }
            continue;
        }

        if in_util_dirs {
            if trimmed.starts_with('-') {
                let item = trimmed
                    .strip_prefix('-')
                    .unwrap_or(trimmed)
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'');
                if !item.is_empty() {
                    dirs.push(item.to_string());
                }
            } else if !trimmed.is_empty() && !trimmed.starts_with('#') {
                // Hit a new key, stop collecting
                in_util_dirs = false;
            }
        }
    }

    dirs
}

/// Best-effort extraction of `languageInjections` entries from a sgconfig.yml file.
/// Returns a list of objects with `host_language`, `rule_pattern` (or `rule_kind`),
/// and `injected` language name.
async fn extract_language_injections(config_path: &Path) -> Vec<Value> {
    let Ok(content) = std::fs::read_to_string(config_path) else {
        return Vec::new();
    };

    let mut injections = Vec::new();
    let mut in_injections = false;
    let mut current_item: Option<Map<String, Value>> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        // Detect start of languageInjections section
        if trimmed.starts_with("languageInjections:") {
            in_injections = true;
            // Handle inline array (uncommon but possible)
            if trimmed.contains('[') {
                break; // Inline array of objects is too complex for line-by-line parsing
            }
            continue;
        }

        if !in_injections {
            continue;
        }

        // List item start: "- " (may be at 0-indent in YAML)
        if trimmed.starts_with("- ") {
            // Flush previous item
            if let Some(item) = current_item.take() {
                injections.push(Value::Object(item));
            }
            current_item = Some(Map::new());
            // Check for inline key-value: "- hostLanguage: js"
            let after_dash = trimmed.strip_prefix("- ").unwrap_or(trimmed).trim();
            if let Some((key, value)) = parse_yaml_simple_kv(after_dash)
                && let Some(ref mut item) = current_item
            {
                item.insert(key, value);
            }
            continue;
        }

        // A new top-level key (not a list item) ends the section
        if !line.starts_with(' ') && !line.starts_with('\t') && !trimmed.is_empty() {
            if let Some(item) = current_item.take() {
                injections.push(Value::Object(item));
            }
            in_injections = false;
            continue;
        }

        // Key-value inside a list item (indented deeper than "- ")
        if let Some(ref mut item) = current_item
            && let Some((key, value)) = parse_yaml_simple_kv(trimmed)
        {
            item.insert(key, value);
        }
    }

    // Flush last item
    if let Some(item) = current_item {
        injections.push(Value::Object(item));
    }

    injections
}

/// Best-effort extraction of `customLanguages` entries from a sgconfig.yml file.
/// Returns a JSON object mapping language names to their config (library_path, extensions).
async fn extract_custom_languages(config_path: &Path) -> Value {
    let Ok(content) = afs::read_to_string(config_path).await else {
        return Value::Object(Map::new());
    };

    let mut languages = Map::new();
    let mut in_custom_languages = false;
    let mut current_lang: Option<String> = None;
    let mut current_lang_config: Option<Map<String, Value>> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("customLanguages:") {
            in_custom_languages = true;
            continue;
        }

        if !in_custom_languages {
            continue;
        }

        // A new top-level key ends the section
        if !line.starts_with(' ')
            && !line.starts_with('\t')
            && !trimmed.is_empty()
            && !trimmed.starts_with('#')
        {
            if let (Some(lang), Some(config)) = (current_lang.take(), current_lang_config.take()) {
                languages.insert(lang, Value::Object(config));
            }
            in_custom_languages = false;
            continue;
        }

        // Language entry at 2-space indent: "graphql:"
        let indent = line.len() - line.trim_start().len();
        if indent == 2 && trimmed.ends_with(':') && !trimmed.contains(' ') {
            // Flush previous language
            if let (Some(lang), Some(config)) = (current_lang.take(), current_lang_config.take()) {
                languages.insert(lang, Value::Object(config));
            }
            current_lang = Some(trimmed.trim_end_matches(':').to_string());
            current_lang_config = Some(Map::new());
            continue;
        }

        // Key-value inside a language entry
        if let Some(ref mut config) = current_lang_config
            && let Some((key, value)) = parse_yaml_simple_kv(trimmed)
        {
            config.insert(key, value);
        }
    }

    // Flush last language
    if let (Some(lang), Some(config)) = (current_lang, current_lang_config) {
        languages.insert(lang, Value::Object(config));
    }

    Value::Object(languages)
}

/// Best-effort extraction of `languageGlobs` entries from a sgconfig.yml file.
/// Returns a JSON object mapping language names to their glob pattern arrays.
async fn extract_language_globs(config_path: &Path) -> Value {
    let Ok(content) = afs::read_to_string(config_path).await else {
        return Value::Object(Map::new());
    };

    let mut globs = Map::new();
    let mut in_language_globs = false;
    let mut current_lang: Option<String> = None;
    let mut current_patterns: Vec<Value> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("languageGlobs:") {
            in_language_globs = true;
            continue;
        }

        if !in_language_globs {
            continue;
        }

        // A new top-level key ends the section
        if !line.starts_with(' ')
            && !line.starts_with('\t')
            && !trimmed.is_empty()
            && !trimmed.starts_with('#')
        {
            if let Some(lang) = current_lang.take() {
                globs.insert(lang, Value::Array(std::mem::take(&mut current_patterns)));
            }
            in_language_globs = false;
            continue;
        }

        let indent = line.len() - line.trim_start().len();

        // Language entry at 2-space indent: "tsx:"
        if indent == 2 && trimmed.ends_with(':') && !trimmed.starts_with('-') {
            // Flush previous language
            if let Some(lang) = current_lang.take() {
                globs.insert(lang, Value::Array(std::mem::take(&mut current_patterns)));
            }
            current_lang = Some(trimmed.trim_end_matches(':').to_string());
            continue;
        }

        // Glob pattern entry: "- \"*.tsx\"" at 4-space indent
        if indent >= 4 && trimmed.starts_with("- ") {
            let pattern = trimmed
                .strip_prefix("- ")
                .unwrap_or(trimmed)
                .trim()
                .trim_matches('"')
                .trim_matches('\'');
            if !pattern.is_empty() {
                current_patterns.push(Value::String(pattern.to_string()));
            }
        }
    }

    // Flush last language
    if let Some(lang) = current_lang {
        globs.insert(lang, Value::Array(current_patterns));
    }

    Value::Object(globs)
}

/// Parse a simple YAML key-value pair like `hostLanguage: js` or `libraryPath: graphql.so`.
/// Returns (key, Value) where key is the trimmed left side and Value is the trimmed right side.
fn parse_yaml_simple_kv(input: &str) -> Option<(String, Value)> {
    let colon_pos = input.find(':')?;
    let key = input[..colon_pos].trim().to_string();
    if key.is_empty() || key.contains(' ') {
        return None;
    }
    let raw_value = input[colon_pos + 1..].trim();

    // Skip keys with empty values -- these are typically parent keys
    // with nested content (e.g. `rule:` followed by indented sub-keys).
    if raw_value.is_empty() {
        return None;
    }

    // Handle inline array: extensions: [graphql]
    if raw_value.starts_with('[') && raw_value.ends_with(']') {
        let inner = &raw_value[1..raw_value.len() - 1];
        let items: Vec<Value> = inner
            .split(',')
            .map(|s| Value::String(s.trim().trim_matches('"').trim_matches('\'').to_string()))
            .filter(|v| !v.as_str().is_some_and(str::is_empty))
            .collect();
        return Some((key, Value::Array(items)));
    }

    let value = raw_value.trim_matches('"').trim_matches('\'');
    Some((key, Value::String(value.to_string())))
}

fn summarize_test_output(stdout: &str, stderr: &str, passed: bool) -> Value {
    let clean_stdout = ANSI_ESCAPE_RE.replace_all(stdout, "");
    let clean_stderr = ANSI_ESCAPE_RE.replace_all(stderr, "");
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

    let rules = parse_test_rule_results(&clean_stdout);
    if !rules.is_empty() {
        summary.insert(
            "rules".to_string(),
            Value::Array(
                rules
                    .iter()
                    .map(|r| {
                        json!({
                            "rule_id": r.rule_id,
                            "passed": r.passed,
                            "markers": r.markers,
                        })
                    })
                    .collect(),
            ),
        );
    }

    let failure_details = parse_test_failure_details(&clean_stdout, &clean_stderr);
    if !failure_details.is_empty() {
        summary.insert("failure_details".to_string(), Value::Array(failure_details));
    }

    Value::Object(summary)
}

/// Per-rule result parsed from `sg test` stdout.
struct TestRuleResult {
    rule_id: String,
    passed: bool,
    /// N = Noisy (false positive), M = Missing (false negative).
    markers: Vec<String>,
}

/// Parse per-rule PASS/FAIL lines from `sg test` stdout.
///
/// The `sg test` output format is:
/// ```text
/// PASS rule-id .....
/// FAIL rule-id ...N..M
/// ```
/// where dots represent individual test cases and N/M markers indicate
/// Noisy or Missing failures within that rule.
fn parse_test_rule_results(stdout: &str) -> Vec<TestRuleResult> {
    let mut results = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(caps) = AST_GREP_TEST_RULE_LINE_RE.captures(trimmed) {
            let status = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let rule_id = caps
                .get(2)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            let trailing = caps.get(3).map(|m| m.as_str()).unwrap_or("");
            let markers: Vec<String> = trailing
                .chars()
                .filter_map(|c| match c {
                    'N' => Some("noisy".to_string()),
                    'M' => Some("missing".to_string()),
                    _ => None,
                })
                .collect();
            results.push(TestRuleResult {
                rule_id,
                passed: status == "PASS",
                markers,
            });
        }
    }
    results
}

/// Parse failure detail blocks from `sg test` output.
///
/// The `sg test` output on failure contains blocks like:
/// ```text
/// ----------- Failure Details -----------
/// [Noisy] Expect rule-id to report no issue, but some issues found in:
///
///   <code snippet>
///
/// [Missing] Expect rule rule-id to report issues, but none found in:
///
///   <code snippet>
/// ```
fn parse_test_failure_details(stdout: &str, stderr: &str) -> Vec<Value> {
    let combined = format!("{stdout}\n{stderr}");
    let mut details = Vec::new();
    let mut lines = combined.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim();

        if let Some(caps) = AST_GREP_TEST_NOISY_RE.captures(trimmed) {
            let rule_id = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let code_snippet = extract_failure_snippet(&mut lines);
            details.push(json!({
                "type": "noisy",
                "rule_id": rule_id,
                "code_snippet": code_snippet,
            }));
        } else if let Some(caps) = AST_GREP_TEST_MISSING_RE.captures(trimmed) {
            let rule_id = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let code_snippet = extract_failure_snippet(&mut lines);
            details.push(json!({
                "type": "missing",
                "rule_id": rule_id,
                "code_snippet": code_snippet,
            }));
        }
    }
    details
}

/// Extract the indented code snippet following a failure detail header.
/// Reads lines until a blank line or another `[`-prefixed header.
fn extract_failure_snippet<'a>(lines: &mut std::iter::Peekable<std::str::Lines<'a>>) -> String {
    let mut snippet_lines = Vec::new();
    // Skip blank lines between the header and the code snippet.
    while let Some(peeked) = lines.peek() {
        if peeked.trim().is_empty() {
            lines.next();
        } else {
            break;
        }
    }
    // Collect indented code lines.
    while let Some(peeked) = lines.peek() {
        let trimmed = peeked.trim();
        if trimmed.is_empty() || trimmed.starts_with('[') {
            break;
        }
        snippet_lines.push(
            lines
                .next()
                .expect("peek() guaranteed next() is Some")
                .to_string(),
        );
    }
    snippet_lines.join("\n")
}
