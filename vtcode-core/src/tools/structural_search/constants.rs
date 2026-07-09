#![allow(unused_imports)]

#[allow(unused_imports)]
use super::*;

pub(super) const DEFAULT_MAX_RESULTS: usize = 100;
pub(super) const MAX_ALLOWED_RESULTS: usize = 10_000;
pub(super) const MAX_ALLOWED_GLOBS: usize = 64;
pub(super) const MAX_ALLOWED_CONTEXT_LINES: usize = 20;
pub(super) const MAX_AUXILIARY_OUTPUT_CHARS: usize = 64_000;
pub(super) const DEFAULT_AST_GREP_CONFIG_PATH: &str = "sgconfig.yml";
pub(super) const AST_GREP_PATTERN_HINT: &str = "Hints: patterns must be valid parseable code for the selected language; ast-grep matches CST structure, not raw text; if the target is only a fragment, retry with a larger parseable `context` and use `selector` when the real match is a subnode inside that pattern; do not try to force a different node kind by combining separate `kind` and `pattern` rules; use one pattern object with `context` plus `selector` instead; operators and keywords usually are not valid meta-variable positions, so switch to parseable code plus `kind`, `regex`, `has`, or another rule object; `$VAR` matches named nodes by default, `$$VAR` includes unnamed nodes, and `$$$ARGS` matches zero or more nodes lazily; `$_NAME` prefix means non-capturing (no backreference); same-name metavariables enforce identity (`$A == $A` matches `a == a` but not `a == b`); meta variables are only detected when the whole AST node text matches meta-variable syntax, so mixed text, lowercase names, or bare `$` followed by digits will not work; if a name must match by prefix or suffix, capture the whole node and narrow it with `constraints.regex` instead of mixing text into the meta variable; `selector` can also override the default effective node when statement-level matching matters more than the inner expression; if matches are too broad or too narrow, tune `strictness` (`smart` default; `cst`, `ast`, `relaxed`, and `signature` control what matching may skip); use `debug_query` to inspect parse output when matching is surprising; structural search is syntax-aware, not scope/type/data-flow analysis.";
pub(super) const AST_GREP_REWRITE_HINT: &str = "For simple pattern-to-pattern rewrites, use `workflow='rewrite'` which previews replacements without applying them; use `workflow='apply'` to write rewrite results directly to disk; for FixConfig rewrites with range expansion via `expandStart`/`expandEnd`, use `workflow='rewrite'` with `fix_config` which generates a temporary YAML rule and previews the expanded replacements.";
pub(super) const AST_GREP_GENERIC_TAIL: &str = "Retry `code_search` with a refined semantic structural query before switching tools. For `sg scan`, `sg test`, `sg new`, `sgconfig.yml`, or advanced rewrite-oriented ast-grep tasks with `transform` or `rewriters`, load the bundled `ast-grep` skill first and use `exec_command.cmd` only when the public structural surface and skill guidance still cannot express the needed CLI flow.";
pub(super) const AST_GREP_PROJECT_CONFIG_HINT: &str = "If the target language is not built into ast-grep, register it in workspace-local `sgconfig.yml` under `customLanguages` with a compiled tree-sitter dynamic library. Prefer `tree-sitter build --output <lib>` to compile it, or use `TREE_SITTER_LIBDIR` with `tree-sitter test` on older tree-sitter versions. Reusing a compatible parser library from Neovim is also valid. If the parser exists but the extension is unusual, map it with `languageGlobs`. Some embedded-language cases are built in, such as HTML `<script>` / `<style>` extraction. If the target syntax is embedded inside another host language, configure `languageInjections` with `hostLanguage`, `rule`, and `injected`; the rule should capture the embedded subregion with a meta variable like `$CONTENT`. If `$VAR` is not valid syntax for that language, use its configured `expandoChar` instead. Use `tree-sitter parse <file>` to inspect parser output when the grammar or file association is unclear. ast-grep rules are single-language, so shared JS/TS-style coverage usually means parsing both through the superset via `languageGlobs` or maintaining separate rules. Use `testConfigs` with `testDir` (required) and optional `snapshotDir` to configure ast-grep test discovery. Use `utilDirs` to declare directories for global utility rules shared across multiple rule files. Use `workflow='inspect'` to see the project's current `testConfigs`, `utilDirs`, `languageInjections`, `customLanguages`, and `languageGlobs` configuration. Utility rules must declare `id` and `language` and can only use `id`, `language`, `rule`, `constraints`, and local `utils`.";
pub(super) const DEBUG_QUERY_LANG_HINT: &str = "action='structural' requires an effective `lang` when `debug_query` is set. Inference only works for unambiguous file paths or single-language positive globs; narrow `path`, add a single-language glob, or set `lang` explicitly";
pub(super) const STRUCTURAL_FORBIDDEN_KEYS: &[&str] = &[
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

pub(super) const VALID_NO_IGNORE_VALUES: &[&str] =
    &["hidden", "dot", "exclude", "global", "parent", "vcs"];
pub(super) const VALID_FORMAT_VALUES: &[&str] = &["github", "sarif", "files_with_matches", "count"];
pub(super) const VALID_REPORT_STYLE_VALUES: &[&str] = &["rich", "medium", "short"];
pub(super) const VALID_BUILTIN_RULES: &[&str] = &["unused-suppression", "no-suppress-all"];
pub(super) const MAX_THREADS: u32 = 256;
/// ast-grep exit code for "no matches found" (not a real error).
pub(super) const AST_GREP_NO_MATCHES_EXIT: i32 = 1;
pub(super) static AST_GREP_METAVARIABLE_RE: Lazy<Regex> = Lazy::new(|| {
    // SAFETY: regex is a static string literal, compilation is guaranteed to succeed
    Regex::new(r"\$\$?\$?[A-Za-z_][A-Za-z0-9_]*").expect("ast-grep metavariable regex must compile")
});
/// Valid ast-grep metavariable: `$` or `$$` followed by uppercase/startunderscore,
/// then uppercase/digits/underscores. Multi-metavariable `$$$` is also valid.
pub(super) // SAFETY: regex is a static string literal, compilation is guaranteed to succeed
static AST_GREP_VALID_METAVAR_RE: Lazy<Regex> = Lazy::new(|| {
    // SAFETY: regex is a static string literal, compilation is guaranteed to succeed
    Regex::new(r"^\$\$?(\$?[A-Z_][A-Z0-9_]*)$").expect("ast-grep valid metavar regex must compile")
});
// SAFETY: regex is a static string literal, compilation is guaranteed to succeed
pub(super) static ANSI_ESCAPE_RE: Lazy<Regex> =
    // SAFETY: regex is a static string literal, compilation is guaranteed to succeed
    Lazy::new(|| {
        Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").expect("ansi escape regex must compile")
    });
// SAFETY: regex is a static string literal, compilation is guaranteed to succeed
pub(super) static AST_GREP_TEST_RESULT_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"test result:\s*(ok|failed)\.\s*(\d+)\s+passed;\s*(\d+)\s+failed;")
        .expect("ast-grep test summary regex must compile")
});
/// Matches per-rule result lines: `PASS rule-id` or `FAIL rule-id` with
/// optional trailing dots and N/M markers (e.g. `FAIL rust/foo ...N..M`).
pub(super) static AST_GREP_TEST_RULE_LINE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(PASS|FAIL)\s+(\S[\w/\-]*)(.*)$")
        .expect("ast-grep test rule line regex must compile")
});
/// Matches failure detail blocks: `[Noisy]` or `[Missing]` headers.
pub(super) static AST_GREP_TEST_NOISY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\[Noisy\]\s+Expect\s+(\S[\w/\-]*)\s+to report no issue")
        .expect("ast-grep noisy detail regex must compile")
});
pub(super) static AST_GREP_TEST_MISSING_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\[Missing\]\s+Expect\s+(?:rule\s+)?(\S[\w/\-]*)\s+to report issues")
        .expect("ast-grep missing detail regex must compile")
});
