# Ast-Grep Project Workflows

Use the public structural tool first for read-only project checks:

- `workflow="scan"` maps to `sg scan <path> --config <config_path>`
- `workflow="test"` maps to `sg test --config <config_path>`
- Default config path is workspace `sgconfig.yml`
- `filter` narrows rules in `scan` and test cases in `test`
- `globs`, `context_lines`, and `max_results` apply to `scan`
- `skip_snapshot_tests` applies only to `test`

## Quick Start

- In VT Code, the preferred install path is `vtcode dependencies install ast-grep`.
- If the user explicitly wants a system-managed install, common external options include Homebrew, Cargo, npm, pip, MacPorts, and Nix.
- Validate installation with `ast-grep --help`.
- On Linux, prefer `ast-grep` over `sg` because `sg` can conflict with the system `setgroups` command.
- For shell usage, quote patterns containing `$VAR` with single quotes so the shell does not strip metavariables before ast-grep runs.
- A representative first refactor is optional chaining:
  - search pattern: `'$PROP && $PROP()'`
  - rewrite: `'$PROP?.()'`
  - interactive apply: `--interactive`

## Command Overview

- `ast-grep run` handles ad-hoc search, `--debug-query`, and one-off rewrite flows.
- `ast-grep scan` handles project scans and isolated rule runs.
- `ast-grep new` bootstraps projects and generates rules.
- `ast-grep test` runs rule tests.
- `ast-grep lsp` starts the language server for editor integration.

## How Ast-Grep Works

- ast-grep accepts multiple query formats.
  - Pattern query for direct structural search
  - YAML rule for linting and reusable project checks
  - Programmatic API for code-driven workflows
- The core engine has two phases.
  - Tree-Sitter parses source code into syntax trees
  - ast-grep’s Rust matcher traverses those trees to search, rewrite, lint, or analyze
- The main usage scenarios are search, rewrite, lint, and analyze.
- Performance comes from processing many files in parallel across available CPU cores.
- In VT Code, keep the bird’s-eye overview simple:
  - public structural tool for read-only query / scan / test
  - bundled ast-grep skill for rule authoring, rewrite/apply work, and API workflows

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

## Rule Catalog

- The catalog is best used as an example bank for rule design and rewrite inspiration.
- Start from examples in the target language before borrowing from another language.
- Catalog signals are useful shorthand:
  - `Simple Pattern Example` means the example is mostly pattern-driven
  - `Fix` means the example includes a rewrite
  - `constraints`, `labels`, `utils`, `transform`, and `rewriters` indicate more advanced features
- Prefer adapting catalog ideas to the current repository over copying them verbatim.
- If a catalog rule relies on features like `transform`, `rewriters`, or utility rules, keep that work in the skill-driven workflow instead of trying to squeeze it into the public read-only structural tool.

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

## Rule Cheat Sheet

- Atomic rules are the narrowest checks on one target node.
  - Use `pattern` for syntax shape, `kind` for AST node type, and `regex` for node text.
  - Use `nthChild` when the target is defined by its named-sibling position.
  - Use `range` when the match must stay inside a specific source span.
- Relational rules describe where the target sits relative to other nodes.
  - Use `inside` and `has` for ancestor or descendant requirements.
  - Use relational `field` when the semantic role matters, such as matching only a `body`.
  - Use `stopBy` when traversal should continue past the nearest boundary instead of stopping at the default scope edge.
  - Use `follows` and `precedes` when relative order matters.
- Composite rules combine multiple checks on the same target node.
  - `all` means every sub-rule must match.
  - `any` means at least one sub-rule must match.
  - `not` excludes a sub-rule.
  - `matches` reuses a named utility rule.
- Utility rules are reusable rule definitions.
  - Use local `utils` in the current config file for nearby reuse.
  - Use global utility-rule files when several rules across the project need the same logic.
- Move from a simple `pattern` to a full rule object when the task needs positional constraints, semantic roles, reusable sub-rules, or several structural conditions on the same node.

## Config Cheat Sheet

- Basic information keys identify the rule and carry project metadata.
  - `id` is the unique rule identifier.
  - `language` selects the parser target.
  - `url` links to the rule’s docs or policy page.
  - `metadata` stores arbitrary project-specific data that rides along with the rule.
- Finding keys define what the rule matches.
  - `rule` is the main matcher.
  - `constraints` filters meta-variable captures after the core match succeeds.
  - `utils` stores reusable helper rules that other parts of the config can reach through `matches`.
- Patching keys define automatic fixes.
  - `transform` derives replacement variables before `fix`.
  - `fix` can be a plain replacement string or a `FixConfig` object using `template`, `expandStart`, and `expandEnd`.
  - `rewriters` are for more complex reusable rewrite building blocks.
- Linting keys define how the rule reports findings.
  - `severity`, `message`, `note`, and `labels` shape the diagnostic output.
  - `files` scopes the included files and `ignores` scopes the excluded files.
  - `files` also supports object syntax when a glob needs flags such as `caseInsensitive`.
- Keep this YAML-config work on the skill path. VT Code’s public structural tool can run read-only scans and tests against these configs, but it does not surface the config schema as tool-call arguments.

## Pattern Syntax

- Pattern text must be valid parseable code for the target language.
- Patterns match syntax trees, so a fragment like `a + 1` can match nested expressions, not just top-level lines.
- `$VAR` matches one named AST node.
- `$$$ARGS` matches zero or more AST nodes, which is useful for arguments, parameters, or statement lists.
- Reusing the same captured meta variable name means the syntax must match identically in each position.
- Names starting with `_` are non-capturing, so repeated `$_VAR` occurrences can match different content.
- `$$VAR` captures unnamed nodes when punctuation or other anonymous syntax matters.
- If a snippet is ambiguous or too short to parse cleanly, switch to an object-style `pattern` with more `context` and a precise `selector`.

## Pattern Parsing Deep Dive

- ast-grep builds a pattern in stages.
  - preprocess meta variables when the language needs a custom `expandoChar`
  - parse the snippet with tree-sitter
  - extract the effective node by builtin heuristic or explicit `selector`
  - detect meta variables inside that effective node
- Invalid patterns usually fail when a meta variable is pretending to be an operator or keyword.
  - use parseable code plus rule fields like `kind`, `regex`, or relational checks instead of patterns such as `$LEFT $OP $RIGHT`
  - do the same when trying to abstract method modifiers like `get` or `set`
- Incomplete and ambiguous snippets can work only because tree-sitter recovered from an error.
  - prefer valid `context` plus `selector`
  - do not rely on recovery staying identical across parser or ast-grep upgrades
- The builtin effective-node heuristic picks the leaf node or the innermost node with more than one child.
  - use `selector` when the real match should be the outer statement instead of the inner expression
  - this matters most when combining a pattern with `follows` or `precedes`
- Meta variables are recognized only when the whole AST node text matches meta-variable syntax.
  - mixed text like `obj.on$EVENT` will not work
  - lowercase names like `$jq` will not work
  - `$$VAR` is for unnamed nodes and `$$$ARGS` is lazy
- VT Code boundary stays the same.
  - public structural tool for read-only query / scan / test and `debug_query`
  - bundled ast-grep skill for deeper pattern authoring and iteration

## Pattern Core Concepts

- ast-grep is structural matching over syntax trees, not plain text matching over source bytes. Use `regex` only when node text itself is the thing you need to filter.
- Tree-Sitter provides a CST-shaped view of the code. That is why operators, punctuation, and modifiers can still affect matching even though ast-grep skips trivial syntax where it safely can.
- Named nodes have a `kind`. Unnamed nodes are literal tokens such as punctuation or operators, and `$$VAR` is the opt-in when those unnamed nodes must be captured.
- `kind` is a property of one node. `field` is a property of the edge between a parent and child, so reach for relational `has` / `inside` plus `field` when semantic role matters.
- Significant nodes are either named or attached through a `field`. That distinction is useful when a token looks minor in Tree-Sitter but still changes how precise a pattern must be.

## Match Algorithm

- The default match mode is `smart`.
  - all pattern nodes must match
  - unnamed nodes in the code being searched may be skipped
- Unnamed nodes written in the pattern are still respected.
  - `function $A() {}` can match `async function`
  - `async function $A() {}` requires the `async` token
- Strictness controls what matching may skip.
  - `cst`: skip nothing
  - `smart`: skip unnamed code nodes only
  - `ast`: skip unnamed nodes on both sides
  - `relaxed`: also ignore comments
  - `signature`: compare mostly named-node kinds and ignore text
- Practical effect:
  - quote differences can stop mattering under `ast`
  - comments can stop mattering under `relaxed`
  - text differences can stop mattering under `signature`
- VT Code exposes `strictness` publicly on read-only structural queries. Use the bundled skill when the task is about picking the right strictness, or when the user needs CLI `--strictness` or YAML pattern-object `strictness`.

## FAQ Highlights

- If a fragment pattern does not match, provide a larger valid `context` snippet and use `selector` to focus the real node you care about.
- If a rule is confusing, shrink it to a minimal repro and use `all` when meta variables captured by one rule must feed later rules. This makes rule order explicit instead of relying on implementation details.
- CLI and Playground can disagree because of parser-version drift and utf-8 vs utf-16 error recovery. Use `--debug-query` or VT Code’s public structural `debug_query` to compare the parsed nodes first.
- Meta variables must be a whole AST node. Prefix or suffix matching like `use$HOOK` should become `$HOOK(...)` plus `constraints.regex`, and `$$$MULTI` stays lazy by design.
- Separate `kind` and `pattern` rules do not change how a pattern is parsed. If the desired node kind needs more context, use a pattern object with `context` and `selector`.
- ast-grep rules are single-language. For shared JS/TS-style cases, either parse both through the superset language via `languageGlobs` or write separate rules.
- ast-grep is syntax-aware only. It does not perform scope, type, control-flow, data-flow, taint, or constant-propagation analysis.

## Find & Patch

- The rewrite pipeline is still find, generate, and patch.
  - `rule` and optional `constraints` find the outer target
  - `transform` derives intermediate strings
  - `fix` patches the final text
- `rewriters` and `transform.rewrite` extend that workflow to matched sub-nodes.
  - apply sub-rules under a meta-variable
  - generate one fix per sub-node
  - join the results with `joinBy`
- This is the declarative way to do one-to-many rewrites such as splitting a barrel import into separate import lines.
- In VT Code, keep this on the bundled ast-grep skill path.
  - public structural tool stays read-only
  - rewrite/apply flows still belong to CLI-driven skill work

## Rewrite Essentials

- Use `ast-grep run --pattern ... --rewrite ...` for ad-hoc rewrites.
- Add YAML `fix` to rules when the rewrite should be versioned with the rule.
- You can keep related rewrite rules in one file with YAML document separators `---`.
- Meta variables used in `pattern` can be reused in `fix`.
- Unmatched meta variables become empty strings in `fix`.
- `fix` is indentation-sensitive. Multiline templates preserve their authored indentation relative to the matched source position.
- If `$VARName` would be parsed as a larger meta variable, use a transform instead of concatenating uppercase suffixes directly.
- Use `transform.rewrite` when rewriting the outer node depends on rewriting a list of child nodes first.
- Use `joinBy` to stitch rewritten child results together before the outer `fix`.
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

- `ast-grep --help`
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
- External install commands via brew, cargo, npm, pip, MacPorts, or Nix
- Adapting catalog examples that depend on rewrite/apply flows or advanced rule features
- `sg new`
- Rewrite/apply behavior
- Interactive flags
- Programmatic API exploration in JavaScript, Python, or Rust
- `transform`
- `rewrite`
- `joinBy`
- `rewriters`
- Custom `sgconfig.yml` authoring across `customLanguages`, `languageGlobs`, `languageInjections`, or `expandoChar`
- Advanced rule-object authoring with `nthChild`, `range`, relational `field`, `stopBy`, or local/global utility rules
- Rule-config authoring with `url`, `metadata`, `constraints`, `labels`, `files`, `ignores`, `transform`, `fix`, `rewriters`, or `caseInsensitive` glob objects
- Iterative rule debugging that depends on unsupported ast-grep flags or output formats
