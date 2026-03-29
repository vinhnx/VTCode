# Ast-Grep Project Workflows

Use the public structural tool first for read-only project checks:

- `workflow="scan"` maps to `sg scan <path> --config <config_path>`
- `workflow="test"` maps to `sg test --config <config_path>`
- Default config path is workspace `sgconfig.yml`
- `filter` narrows rules in `scan` and test cases in `test`
- `globs`, `context_lines`, and `max_results` apply to `scan`
- `skip_snapshot_tests` applies only to `test`

## Command Overview

- `ast-grep run` handles ad-hoc search, `--debug-query`, and one-off rewrite flows.
- `ast-grep scan` handles project scans and isolated rule runs.
- `ast-grep new` bootstraps projects and generates rules.
- `ast-grep test` runs rule tests.
- `ast-grep lsp` starts the language server for editor integration.

## Project Scaffolding

- `ast-grep scan` needs project scaffolding before it can run repository rules.
- The minimum scaffold is:
  - workspace `sgconfig.yml`
  - at least one rule directory, usually `rules/`
- `ast-grep new` bootstraps the scaffold interactively and can also create:
  - `rule-tests/` for rule tests
  - `utils/` for reusable utility rules
- Typical scaffold layout:
  - `sgconfig.yml`
  - `rules/`
  - `rule-tests/`
  - `utils/`
- `ast-grep new rule` creates a rule file directly and can optionally create its paired test file.
- If the repo already has `sgconfig.yml` and `rules/`, extend the existing scaffold instead of creating a second one.

## Rule Essentials

- A minimal rule file starts with `id`, `language`, and root `rule`.
- The root `rule` matches one target node per result. Meta variables come from that matched node and its substructure.
- Rule objects are conjunctive: a node must satisfy every field that appears.
- Rule objects are effectively unordered. If `has` or `inside` behavior becomes order-sensitive, move the logic into explicit `all` clauses so evaluation order is obvious.
- Rule object categories:
  - Atomic: `pattern`, `kind`, `regex`
  - Relational: `inside`, `has`, `follows`, `precedes`
  - Composite: `all`, `any`, `not`, `matches`
- `language` changes how patterns are parsed, so author patterns in the actual target language instead of assuming syntax is portable.

## Rewrite Essentials

- Use `ast-grep run --pattern ... --rewrite ...` for ad-hoc rewrites.
- Add YAML `fix` to rules when the rewrite should be versioned with the rule.
- You can keep related rewrite rules in one file with YAML document separators `---`.
- Meta variables used in `pattern` can be reused in `fix`.
- Unmatched meta variables become empty strings in `fix`.
- `fix` is indentation-sensitive. Multiline templates preserve their authored indentation relative to the matched source position.
- If `$VARName` would be parsed as a larger meta variable, use a transform instead of concatenating uppercase suffixes directly.
- Use advanced `FixConfig` when replacing the target node is not enough:
  - `template` is the replacement text
  - `expandStart` expands the rewritten range backward while its rule keeps matching
  - `expandEnd` expands the rewritten range forward while its rule keeps matching
- `expandStart` / `expandEnd` are the right tool for deleting adjacent commas or other surrounding syntax that is not part of the target node itself.
- Keep `transform` and `rewriters` in the same skill-driven rewrite workflow.

## CLI Modes

- `--interactive` reviews rewrite results one-by-one. Interactive controls are `y` accept, `n` skip, `e` open in editor, and `q` quit.
- `--json` emits raw ast-grep JSON output. Use it when a shell pipeline needs native ast-grep data instead of VT Code’s normalized result objects.
- `--debug-query` is useful for isolated query iteration. In VT Code, prefer the public structural `debug_query` field before dropping to raw CLI.
- `--stdin` lets ast-grep parse piped code, but:
  - it conflicts with `--interactive`
  - `run --stdin` requires `--lang`
  - `scan --stdin` requires exactly one rule via `--rule` / `-r`
  - stdin mode needs both `--stdin` and a non-TTY execution context
- `--color never` is the direct CLI switch when raw ast-grep output must be plain and non-ANSI.

## Tooling Around ast-grep

- `ast-grep completions` generates shell completion scripts.
- GitHub Action support runs project scans in CI once the repository scaffold exists.
- The official action is for repository linting automation, not for local rule authoring iteration.

## API Usage

- ast-grep’s rule language is intentionally simple. Use the library API when the transformation is hard to express as rules or `fix`.
- Typical API escalation cases:
  - replace nodes individually based on their content
  - replace nodes conditionally based on both content and surrounding nodes
  - count matching nodes or use their order in later decisions
  - compute replacement strings programmatically
- Binding guidance:
  - JavaScript: most mature and reliable binding
  - Python: good for programmatic syntax-tree workflows
  - Rust `ast_grep_core`: most efficient and lowest-level option
- JS/Python support for applying ast-grep `fix` directly is still experimental, so prefer explicit patch generation when you need dependable automation.
- If a task crosses this boundary, stop trying to encode it as more YAML and switch to a proper programmatic implementation.

Switch to direct CLI work through `unified_exec` when the task needs:

- `ast-grep new`
- `ast-grep new rule`
- `ast-grep run --debug-query`
- `ast-grep run --json`
- `ast-grep run --stdin --lang <lang>`
- `ast-grep scan --rule <file> <path>`
- `ast-grep scan -r <rule.yml>`
- `ast-grep scan --inline-rules '...' <path>`
- `ast-grep scan --stdin --rule <rule.yml>`
- `ast-grep run --pattern <pattern> --rewrite <rewrite> [--interactive]`
- `ast-grep run --pattern <pattern> --rewrite <rewrite> --update-all`
- YAML `fix`, `template`, `expandStart`, or `expandEnd`
- `ast-grep lsp`
- `ast-grep completions`
- GitHub Action workflow setup
- Raw `--color never`
- `sg new`
- Rewrite/apply behavior
- Interactive flags
- Programmatic API exploration in JavaScript, Python, or Rust
- `transform`
- `rewriters`
- Custom `sgconfig.yml` authoring across `customLanguages`, `languageGlobs`, `languageInjections`, or `expandoChar`
- Iterative rule debugging that depends on unsupported ast-grep flags or output formats
