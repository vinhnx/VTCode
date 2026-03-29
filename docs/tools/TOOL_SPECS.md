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
- Optional common fields: `workflow` (`"query" | "scan" | "test"`, default `"query"`), `path` (default `"."` for query/scan), `config_path` (default workspace `sgconfig.yml` for scan/test), `filter`, `globs`, `context_lines`, `max_results`
- `workflow="query"`:
  - Required: `pattern`
  - Optional: `lang`, `selector`, `strictness`, `debug_query`
  - Result shape: top-level `matches` array with `file`, `line_number`, `text`/`lines`, `language`, and compact `range` metadata, plus `backend: "ast-grep"`
- `workflow="scan"`:
  - Optional: `path`, `config_path`, `filter`, `globs`, `context_lines`, `max_results`
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
  - Syntax-aware only; do not treat this surface as scope, type, or data-flow analysis
  - Pattern syntax follows ast-grep rules: `$VAR` captures one named node, `$$$ARGS` captures zero or more nodes, `$$VAR` includes unnamed nodes, and `$_` suppresses capture
  - `workflow="query"` patterns must be valid parseable code; for fragments, unnamed-token cases, or role-sensitive matching, prefer the bundled `ast-grep` skill workflow
  - Custom languages are supported only through local ast-grep configuration, typically workspace `sgconfig.yml` `customLanguages` plus a compiled tree-sitter dynamic library
  - Non-standard extensions and embedded languages should be handled through local ast-grep config such as `languageGlobs` and `languageInjections`, not by guessing a different file language in the tool call
  - Public project support stops at read-only `sg scan` and `sg test`
  - Use the bundled `ast-grep` skill for `sg new`, rewrite/apply flows, interactive flags, `transform`, `rewriters`, or non-trivial `sgconfig.yml` authoring/debugging
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
- Prefer `workflow="scan"` for public `sg scan` equivalents and `workflow="test"` for public `sg test` equivalents.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task becomes rule authoring, `sg new`, rewrite/apply work, interactive ast-grep work, or `sgconfig.yml` authoring/debugging.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task is a quick-start or install flow, including `ast-grep --help`, shell quoting for metavariables, or optional-chaining style first rewrites.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task asks for ast-grep catalog examples, existing rewrite examples, or help adapting catalog rules to this repository.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task needs project scaffolding via `ast-grep new` or `ast-grep new rule`, or when it needs guidance around `rules/`, `rule-tests/`, `utils/`, and `sgconfig.yml`.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task needs `scan --rule`, `scan --inline-rules`, relational/composite rule objects, `matches` utility rules, or rule-order debugging.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task needs `--rewrite`, YAML `fix`, `template`, `expandStart`, `expandEnd`, `--interactive`, `--update-all`, or indentation-sensitive rewrite behavior.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task needs raw ast-grep CLI behavior such as `--stdin`, `--json`, `scan -r`, `lsp`, shell completions, GitHub Action setup, or direct `--color never` control.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task is really about pattern syntax design, meta-variable capture rules, `$$$ARGS`, `$_`, `$$VAR`, or object-style patterns.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task outgrows rule syntax and needs ast-grep’s JavaScript/Python/Rust API, `ast_grep_core`, computed replacements, conditional AST edits, or node-order/count logic.
- Prefer `load_skill` with the bundled `ast-grep` skill when the target snippet is not valid standalone code and needs pattern-object `context` plus `selector`.
- Prefer `load_skill` with the bundled `ast-grep` skill when matching depends on `$$VAR`, `field`, modifiers/operators, or other CST-level distinctions.
- Prefer `load_skill` with the bundled `ast-grep` skill when the requested language is not built into ast-grep and needs workspace `sgconfig.yml` `customLanguages` setup or `expandoChar`.
- Prefer `load_skill` with the bundled `ast-grep` skill when the task needs `languageGlobs`, `languageInjections`, local/global utility rules, `transform`, or `rewriters`.
- `action="intelligence"` remains executor-compatible for legacy callers, but it is deprecated and not part of the public schema.
