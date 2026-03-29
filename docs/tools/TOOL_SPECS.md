# VT Code Tool Specifications

This document describes the canonical public tool surface exposed to VT Code models. Legacy aliases such as `read_file`, `grep_file`, `list_files`, and PTY helpers may still route internally, but prompts, schemas, and evaluations should target the unified tools below.

## Canonical Public Tools

- `unified_search`
  Purpose: Read-only discovery and lookup across the workspace and related runtime state.
  Actions:
  - `grep`: broad text search
  - `list`: file discovery
  - `structural`: syntax-aware code search via local ast-grep (`sg`)
  - `tools`: tool discovery
  - `errors`: archived/current error lookup
  - `agent`: agent/runtime info
  - `web`: fetch a URL
  - `skill`: load a skill by name

- `unified_file`
  Purpose: File reads and workspace-local edits.
  Actions:
  - `read`
  - `write`
  - `edit`
  - `patch`
  - `delete`
  - `move`
  - `copy`

- `unified_exec`
  Purpose: Command execution and session control.
  Actions:
  - `run`
  - `write`
  - `poll`
  - `continue`
  - `inspect`
  - `list`
  - `close`
  - `code`

- `request_user_input`
  Purpose: Collect short structured user decisions when the current mode allows it.

- `apply_patch`
  Purpose: First-class patch application for models that support the freeform patch surface.

## `unified_search`

### `grep`

- Required: `pattern`
- Optional: `path` (default `"."`), `case_sensitive`, `context_lines`, `max_results`
- Use when: you need broad text matching or a quick file-content sweep

### `list`

- Optional: `path` (default `"."`), `mode`, `max_results`
- Use when: you need file names, directories, or tree-style discovery

### `structural`

- Required: `action="structural"`
- Optional common fields: `workflow` (`"query" | "scan" | "test"`, default `"query"`), `path` (default `"."` for query/scan; public structural takes one root per call even though raw `ast-grep run` accepts multiple paths), `config_path` (default workspace `sgconfig.yml` for scan/test), `filter`, `globs`, `context_lines`, `max_results`
- `workflow="query"`:
  - Required: `pattern`
  - Optional: `lang`, `selector`, `strictness`, `debug_query`
  - Internal mapping: read-only `ast-grep run --pattern ... --json=compact --color=never`, plus optional `--lang`, `--selector`, `--strictness`, repeated `--globs`, and `--context`
  - `lang` accepts ast-grep built-in aliases. VT Code normalizes and infers a local subset it can pre-parse itself, such as Rust, Python, JavaScript, TypeScript, TSX, Go, and Java, while explicit unsupported aliases are still passed through to ast-grep unchanged.
  - `strictness` tunes ast-grep matching for read-only queries. `smart` is the default; common alternatives are `cst`, `ast`, `relaxed`, and `signature`. `template` is passed through for ast-grep compatibility.
  - `context_lines` maps to ast-grep `--context`; raw `--before` and `--after` are intentionally not exposed
  - ast-grep `run` exit code `1` is normalized to an empty `matches` array instead of surfacing as a VT Code error
  - Result shape: top-level `matches` array with `file`, `line_number`, `text`/`lines`, `language`, and compact `range` metadata, plus `backend: "ast-grep"`
- `workflow="scan"`:
  - Optional: `path`, `config_path`, `filter`, `globs`, `context_lines`, `max_results`
  - Internal mapping: read-only `ast-grep scan --config ... --json=stream --include-metadata --color=never`, plus optional `--filter`, repeated `--globs`, and `--context`
  - `context_lines` maps to ast-grep `--context`; raw `--before` and `--after` are intentionally not exposed
  - ast-grep `scan` exit code `1` when error-severity findings exist is normalized to structured `findings` instead of surfacing as a VT Code error
  - Result shape: top-level `findings` array with `file`, `line_number`, `text`/`lines`, `language`, `range`, `rule_id`, `severity`, `message`, `note`, optional `metadata`, plus `summary`, `truncated`, and `backend: "ast-grep"`
- `workflow="test"`:
  - Optional: `config_path`, `filter`, `skip_snapshot_tests`
  - Result shape: `passed`, `stdout`, `stderr`, `summary`, and `backend: "ast-grep"`
- Constraints:
  - Read-only only; rewrite/apply flags are rejected
  - `lang`, `selector`, `strictness`, and `debug_query` are only valid for `workflow="query"`
  - `lang` is required when `debug_query` is set
  - `skip_snapshot_tests` is only valid for `workflow="test"`
  - Requires a local `sg` / `ast-grep` binary; if missing, VT Code returns an actionable error, points to the bundled `ast-grep` skill, and recommends `vtcode dependencies install search-tools` or `vtcode dependencies install ast-grep`
  - VT Code-managed installs live in `~/.vtcode/bin`
  - On Linux, prefer the canonical `ast-grep` binary name instead of `sg`
  - Raw ast-grep CLI flags such as `--stdin`, `--json`, `--color`, `--heading`, `--threads`, `--inspect`, `--follow`, `--no-ignore`, `--before`, `--after`, `--interactive`, `--update-all`, `--rewrite`, `--rule`, `--inline-rules`, `--format`, `--report-style`, `--error`, `--warning`, `--info`, `--hint`, and `--off` are not part of the public structural surface and should go through the bundled `ast-grep` skill plus `unified_exec` when needed
  - Test-only ast-grep flags such as `--test-dir`, `--snapshot-dir`, `--include-off`, interactive snapshot review, and snapshot update flows are also CLI-only and not part of the public structural surface
  - `ast-grep new`, `ast-grep lsp`, `ast-grep completions`, and top-level help/command-discovery flows are CLI-only and should go through the bundled `ast-grep` skill plus `unified_exec`
  - Syntax-aware only; do not treat this surface as scope, type, or data-flow analysis
  - Pattern syntax follows ast-grep rules: `$VAR` captures one named node, `$$$ARGS` captures zero or more nodes, `$$VAR` includes unnamed nodes, and `$_` suppresses capture
  - `workflow="query"` patterns must be valid parseable code; for fragments, unnamed-token cases, or role-sensitive matching, prefer the bundled `ast-grep` skill workflow
  - Path and glob language inference use VT Code’s local subset of ast-grep-compatible extensions, not the full ast-grep built-in catalog; set `lang` explicitly when inference is ambiguous or outside that subset
  - Custom languages are supported only through local ast-grep configuration, typically workspace `sgconfig.yml` `customLanguages` plus a compiled tree-sitter dynamic library
  - Built-in embedded-language behavior is limited to what ast-grep already knows, such as HTML `<script>` / `<style>` extraction; other nested-language cases depend on local `languageInjections`
  - Non-standard extensions and embedded languages should be handled through local ast-grep config such as `languageGlobs` and `languageInjections`, not by guessing a different file language in the tool call
  - Public project support stops at read-only `sg scan` and `sg test`
  - Use the bundled `ast-grep` skill for `sg new`, rewrite/apply flows, interactive flags, `transform`, `replace`, `substring`, `convert`, `toCase`, `separatedBy`, `rewrite`, `joinBy`, `rewriters`, custom parser compilation, or non-trivial `sgconfig.yml` authoring/debugging
- Use when: you need syntax-aware search, read-only project rule scans, or read-only ast-grep rule tests
- Avoid when: plain text grep is simpler, the search target is not syntax-sensitive, or the task depends on semantic/static-analysis facts

### `tools`

- Required: `keyword`
- Optional: `detail_level`

### `errors`

- Optional: `scope`

### `agent`

- Optional: `mode`

### `web`

- Required: `url`
- Optional: `prompt`, `max_bytes`, `timeout_secs`

### `skill`

- Required: `name`

## Guidance

- Prefer `unified_search` over shell `grep`/`find` for normal workspace discovery.
- Prefer `grep` for broad text search.
- Prefer `structural` for syntax-sensitive search, read-only project scans, and read-only ast-grep rule tests.
- Prefer public `structural` `strictness` on `workflow="query"` when the task is just tuning read-only matching between `cst`, `smart`, `ast`, `relaxed`, and `signature`.
- Prefer `workflow="scan"` for public `sg scan` equivalents and `workflow="test"` for public `sg test` equivalents.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task becomes rule authoring, `sg new`, rewrite/apply work, interactive ast-grep work, or `sgconfig.yml` authoring/debugging.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task is a quick-start or install flow, including `ast-grep --help`, shell quoting for metavariables, or optional-chaining style first rewrites.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task asks for ast-grep catalog examples, existing rewrite examples, or help adapting catalog rules to this repository.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task needs project scaffolding via `ast-grep new` or `ast-grep new rule`, or when it needs guidance around `rules/`, `rule-tests/`, `utils/`, and `sgconfig.yml`.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task needs `sgconfig.yml` top-level config semantics such as `ruleDirs`, `testConfigs`, `testDir`, `snapshotDir`, `utilDirs`, `languageGlobs` precedence, target-triple `libraryPath`, `languageSymbol`, or experimental `languageInjections`.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task needs `scan --rule`, `scan --inline-rules`, scan severity overrides (`--error`, `--warning`, `--info`, `--hint`, `--off`), scan output modes such as `--format` / `--report-style`, relational/composite rule objects, positive-rule requirements, limited `kind` ESQuery syntax, `matches` utility rules, `nthChild` formulas or `reverse` / `ofRule`, `range`, relational `field`, exact `stopBy` semantics, local/global utility rules, or rule-order debugging.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task needs `test --test-dir`, `--snapshot-dir`, `--include-off`, snapshot update flows, interactive snapshot review, or detailed `ast-grep test` CLI behavior beyond public `workflow="test"`.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task needs rule-config YAML keys such as `url`, `metadata`, `constraints`, `severity`, `message`, `note`, `labels`, `files`, `ignores`, `caseInsensitive` glob objects, `severity: off`, `--include-metadata`, or YAML multi-document rule files.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task depends on config semantics like single-meta `constraints`, `constraints` after `rule`, `note` without meta interpolation, label-variable scoping, `files` / `ignores` precedence, relative glob roots, the `./` path gotcha, or the difference between YAML `ignores` and CLI `--no-ignore`.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task needs `--rewrite`, YAML string `fix`, `FixConfig`, `template`, `expandStart`, `expandEnd`, meta variables anywhere in replacement text, comma/list-item cleanup, `--interactive`, `--update-all`, or indentation-sensitive rewrite behavior.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task needs transformation-object details such as `replace`, `substring`, `convert`, `toCase`, `separatedBy`, `CaseChange`, string-form transforms, or experimental `transform.rewrite` ordering semantics.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task needs rewriter-specific semantics such as required rewriter fields, rewriter-local captures / utils / transforms, nested rewriter calls, or the barrel-import rewrite pattern.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task needs raw ast-grep CLI behavior such as `--stdin`, `--json`, `--heading`, `--threads`, `--inspect`, `--follow`, `--no-ignore`, `--before`, `--after`, scan `-r`, `lsp`, shell completions, GitHub Action setup, direct `--color never` control, or run-command exit-code details.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task is about top-level command discovery such as `ast-grep --help`, subcommand selection, `new project|rule|test|util`, `lsp`, `completions`, or `help`.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task is really about pattern syntax design, meta-variable capture rules, `$$$ARGS`, `$_`, `$$VAR`, or object-style patterns.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task is troubleshooting incomplete fragments, interpreting `debug_query`, comparing Playground vs CLI results, or using pattern-object `context` plus `selector`.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task needs `kind` plus `pattern` troubleshooting, `rule order` guidance, prefix matching via `constraints.regex`, or multi language rule strategy via `languageGlobs` versus separate rules.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task asks how ast-grep works at a high level, including Tree-Sitter parsing, pattern vs YAML vs API inputs, search/rewrite/lint/analyze modes, or multi-core processing.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task is about pattern core concepts such as textual vs structural matching, AST vs CST, named vs unnamed nodes, `kind` vs `field`, or significant vs trivial nodes.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task is about pattern parsing heuristics such as invalid / incomplete / ambiguous snippets, effective-node selection via `selector`, meta-variable detection, lazy multi-meta behavior, or `expandoChar`.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task is about ast-grep’s match algorithm itself, choosing between strictness levels, or using CLI / YAML strictness outside the public read-only query surface.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task needs Find & Patch style rewrites such as `rewriters`, `transform.rewrite`, `joinBy`, recursive sub-node patching, or barrel-import splitting.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task outgrows rule syntax and needs ast-grep’s Node NAPI / Python / Rust API, `parse`, `Lang`, `pattern`, `kind`, `SgRoot`, `SgNode`, `NapiConfig`, `ast_grep_core`, computed replacements, conditional AST edits, or node-order/count logic.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task mentions deprecated language-specific JS objects like `js.parse(...)`; the current guidance should move that work to the unified NAPI functions instead.
- Prefer `load_skill` with the bundled `ast-grep` skill when the target snippet is not valid standalone code and needs pattern-object `context` plus `selector`.
- Prefer `load_skill` with the bundled `ast-grep` skill when matching depends on `$$VAR`, `field`, modifiers/operators, or other CST-level distinctions.
- Prefer `load_skill` with the bundled `ast-grep` skill when the requested language is not built into ast-grep and needs workspace `sgconfig.yml` `customLanguages` setup or `expandoChar`.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task needs custom parser compilation via `tree-sitter build`, the `TREE_SITTER_LIBDIR` fallback, reused Neovim parser libraries, or parser inspection with `tree-sitter parse`.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task is mainly about the built-in language catalog, alias selection, extension defaults, or deciding between built-in extension mapping and `languageGlobs`.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task needs `languageGlobs`, `languageInjections`, `hostLanguage`, `injected`, dynamic injected candidates through `$LANG`, `$CONTENT`-style embedded-region capture, styled-components CSS, GraphQL template literals, local/global utility rules, `transform`, `rewrite`, `joinBy`, or `rewriters`.
- Prefer another analysis tool when the task depends on scope, type, control-flow, data-flow, taint, or constant-propagation facts; ast-grep’s structural surface does not provide that analysis.
- `action="intelligence"` remains executor-compatible for legacy callers, but it is deprecated and not part of the public schema.
