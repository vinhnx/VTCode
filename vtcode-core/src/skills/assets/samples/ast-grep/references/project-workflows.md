# Ast-Grep Project Workflows

Use the public structural tool first for read-only project checks:

- `workflow="scan"` maps to `sg scan <path> --config <config_path> --json=stream --include-metadata --color=never`
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

- `ast-grep` defaults to `run`, so `ast-grep -p 'foo()'` and `ast-grep run -p 'foo()'` are equivalent.
- `ast-grep run` handles ad-hoc search, `--debug-query`, and one-off rewrite flows.
- `run` defaults to path `.` and can accept multiple search paths in one invocation.
- `ast-grep scan` handles project scans and isolated rule runs.
- `scan` defaults to path `.` and can accept multiple search paths in one invocation.
- `ast-grep new` bootstraps projects and generates rules.
- `ast-grep test` runs rule tests.
- `ast-grep lsp` starts the language server for editor integration.
- `ast-grep completions` generates shell completion scripts.
- `ast-grep help` and `ast-grep --help` are the CLI discovery entry points when the right subcommand is unclear.

## Built-In Languages

- ast-grep has a large built-in language list.
  - common aliases include `bash`, `c`, `cc` / `cpp`, `cs`, `css`, `ex`, `go` / `golang`, `html`, `java`, `js` / `javascript` / `jsx`, `json`, `kt`, `lua`, `php`, `py` / `python`, `rb`, `rs` / `rust`, `swift`, `ts` / `typescript`, `tsx`, and `yml`
  - file discovery uses the built-in extension mapping unless the project overrides it
- `--lang <alias>` and YAML `language: <alias>` both use those aliases.
- In VT Code, public structural `lang` passes through to ast-grep, but VT Code only normalizes and infers the subset it can pre-parse locally.
  - current local inference subset: Rust, Python, JavaScript, TypeScript, TSX, Go, and Java
  - current alias/extension normalization examples: `golang`, `jsx`, `cjs`, `mjs`, `cts`, `mts`, `py3`, `pyi`
- Use `languageGlobs` when the repo wants different extension-to-language mapping from ast-grep’s defaults.

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

## sgconfig.yml

- `sgconfig.yml` is the project config file, not a single-rule YAML. Use it to define discovery, tests, parser overrides, and embedded-language behavior for the whole repo.
- Top-level keys to keep straight:
  - `ruleDirs`: required rule discovery paths, relative to `sgconfig.yml`
  - `testConfigs`: optional test discovery objects
  - `utilDirs`: optional global utility-rule directories
  - `languageGlobs`: parser remapping that overrides default extension detection
  - `customLanguages`: project-local parser registration
  - `languageInjections`: experimental embedded-language config
- `testConfigs` details:
  - each entry requires `testDir`
  - `snapshotDir` is optional
  - default snapshots live in `__snapshots__` under the `testDir`
- `languageGlobs` takes precedence over the built-in parser mapping, which is why it can be used for similar-language reuse or full parser reassignment.
- `customLanguages` details:
  - `libraryPath` can be a single relative path or a target-triple map
  - `extensions` is required
  - `expandoChar` is optional
  - `languageSymbol` defaults to `tree_sitter_{name}`
- `languageInjections` details:
  - `hostLanguage`, `rule`, and `injected` are required
  - `injected` can be one fixed language or dynamic injected candidates chosen through `$LANG`
- VT Code boundary:
  - public structural query / scan / test can use an existing `config_path`
  - detailed `sgconfig.yml` authoring or debugging remains skill-driven work

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
- A root rule object needs at least one positive anchor. In practice, start with `pattern` or `kind`; do not expect `regex` alone to define the target node shape.
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
  - `pattern` can be an object with `context`, `selector`, and optional `strictness`.
  - `kind` also accepts limited ESQuery-style syntax in newer ast-grep versions.
  - `regex` matches the whole node text.
  - Use `nthChild` when the target is defined by its named-sibling position.
  - `nthChild` accepts a number, an `An+B` formula string, or an object with `position`, `reverse`, and `ofRule`.
  - Use `range` when the match must stay inside a specific source span.
- Relational rules describe where the target sits relative to other nodes.
  - Use `inside` and `has` for ancestor or descendant requirements.
  - Use relational `field` when the semantic role matters, such as matching only a `body`. `field` is only available on `inside` and `has`.
  - Use `stopBy` when traversal should continue past the nearest boundary instead of stopping at the default scope edge.
  - `stopBy: neighbor` is the default. `end` searches to the edge, and a rule-object stop is inclusive.
  - Use `follows` and `precedes` when relative order matters.
- Composite rules combine multiple checks on the same target node.
  - `all` means every sub-rule must match.
  - `any` means at least one sub-rule must match.
  - `not` excludes a sub-rule.
  - `matches` reuses a named utility rule.
  - `all` / `any` combine rules for one node, not multiple target nodes.
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
- One YAML file can contain multiple rule documents separated by `---`.
- Finding keys define what the rule matches.
  - `rule` is the main matcher.
  - `constraints` filters meta-variable captures after the core match succeeds.
  - `constraints` only applies to single meta variables, not `$$$ARGS`-style multi captures.
  - constrained meta variables are usually a poor fit inside `not`.
  - `utils` stores reusable helper rules that other parts of the config can reach through `matches`.
- Patching keys define automatic fixes.
  - `transform` derives replacement variables before `fix`.
  - `fix` can be a plain replacement string or a `FixConfig` object using `template`, `expandStart`, and `expandEnd`.
  - `rewriters` are for more complex reusable rewrite building blocks.
- Linting keys define how the rule reports findings.
  - `severity`, `message`, `note`, and `labels` shape the diagnostic output.
  - `severity: off` disables the rule in scan results.
  - `note` supports Markdown but does not interpolate meta variables.
  - `labels` must reference meta variables defined by the rule or `constraints`.
  - `files` scopes the included files and `ignores` scopes the excluded files.
  - `files` also supports object syntax when a glob needs flags such as `caseInsensitive`.
  - `ignores` is checked before `files`.
  - both `files` and `ignores` are relative to the directory containing `sgconfig.yml`
  - do not prefix these globs with `./`
  - YAML `ignores` is different from CLI `--no-ignore`
- Rule `metadata` only appears in ast-grep JSON output when metadata inclusion is enabled, for example with `--include-metadata`.
- Keep this YAML-config work on the skill path. VT Code’s public structural tool can run read-only scans and tests against these configs, but it does not surface the config schema as tool-call arguments.

## Transformation Objects

- `transform` derives strings from captured meta variables before `fix` applies.
- `replace` is regex-based text replacement over one captured meta variable.
  - `source` must be `$VAR` style
  - `replace` is the Rust regex
  - `by` is the replacement string
  - capture groups from the regex can be reused in `by`
- `substring` is Python-style slicing over Unicode characters.
  - `startChar` is inclusive
  - `endChar` is exclusive
  - negative indexes count backward from the end of the string
- `convert` changes identifier casing through `toCase`.
  - common targets: `camelCase`, `snakeCase`, `kebabCase`, `pascalCase`, `lowerCase`, `upperCase`, `capitalize`
  - `separatedBy` controls how the source string is split into words before conversion
- `CaseChange` is the separator for transitions like `astGrep`, `ASTGrep`, or `XMLHttpRequest`
- Ast-grep also accepts string-form transforms such as `replace(...)`, `substring(...)`, `convert(...)`, and `rewrite(...)`.

## Rewriters

- `rewriters` is an experimental feature and should be treated as advanced YAML, not the default way to structure rewrites.
- A rewriter only accepts:
  - `id`
  - `rule`
  - `constraints`
  - `transform`
  - `utils`
  - `fix`
- `id`, `rule`, and `fix` are required.
- Rewriters are only reachable through `transform.rewrite`.
- Captured meta variables do not cross rewriter boundaries.
  - one rewriter cannot read another rewriter’s captures
  - the outer rule cannot read captures local to a rewriter
- Rewriter-local `transform` variables and `utils` stay local the same way.
- A rewriter can still call other rewriters from the same list inside its own `transform` section.

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

## Custom Languages

- Use this path when the language has a tree-sitter grammar but is not built into ast-grep.
- Setup flow:
  - install `tree-sitter` CLI and fetch or author the grammar
  - compile the parser as a dynamic library
  - register it in workspace `sgconfig.yml` `customLanguages`
- Preferred compiler path:
  - `tree-sitter build --output <lib>`
  - if that subcommand is unavailable, use `TREE_SITTER_LIBDIR=<dir> tree-sitter test`
- Existing dynamic libraries can be reused, including parser builds produced by Neovim, if the grammar is the right one.
- Register:
  - `libraryPath` for the compiled library
  - `extensions` for file detection
  - optional `expandoChar` when `$VAR` is not valid syntax in the target language
- Use `tree-sitter parse <file>` to inspect parser output when the grammar or extension mapping is in doubt.
- VT Code boundary:
  - public structural queries can consume the configured custom language
  - parser compilation and `sgconfig.yml` authoring remain skill-driven work

## Language Injection

- ast-grep can search embedded-language regions inside a host document.
  - built-in behavior already covers HTML with CSS in `<style>` and JavaScript in `<script>`
  - project-specific cases should be configured with `languageInjections`
- A `languageInjections` entry should define:
  - `hostLanguage` for the outer file language
  - `rule` for the embedded region
  - `injected` for the parser to use inside that region
- The injection `rule` should capture the subregion with a meta variable such as `$CONTENT`.
  - styled-components example: `styled.$TAG\`$CONTENT\``
  - GraphQL tagged template example: `graphql\`$CONTENT\``
- Use `languageGlobs` when a whole file should be parsed through a different or superset language.
- Use `languageInjections` when only a nested fragment switches language inside the same file.
- VT Code boundary:
  - public structural query / scan / test can consume existing injection config
  - authoring or debugging `languageInjections` remains skill-driven work

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
- String `fix` is plain replacement text, not a parsed Tree-Sitter pattern, so meta variables can appear anywhere in the replacement.
- Unmatched meta variables become empty strings in `fix`.
- `fix` is indentation-sensitive. Multiline templates preserve their authored indentation relative to the matched source position.
- If `$VARName` would be parsed as a larger meta variable, use a transform instead of concatenating uppercase suffixes directly.
- Use `transform.rewrite` when rewriting the outer node depends on rewriting a list of child nodes first.
- Use `joinBy` to stitch rewritten child results together before the outer `fix`.
- `transform.rewrite` is still experimental.
  - it rewrites descendants under the captured source
  - overlapping matches are not allowed
  - higher-level AST matches win before nested ones
  - for one node, the first matching rewriter in declaration order is the only one applied
- Rewriter example to keep in mind:
  - capture a barrel import once
  - use a rewriter over the identifier descendants
  - optional `convert` inside the rewriter can derive lowercase import paths
  - `joinBy: "\n"` stitches the generated import statements together
- Use advanced `FixConfig` when replacing the target node is not enough:
  - `template` is the replacement text
  - `expandStart` expands the rewritten range backward while its rule keeps matching
  - `expandEnd` expands the rewritten range forward while its rule keeps matching
- `expandStart` / `expandEnd` are the right tool for deleting adjacent commas or other surrounding syntax that is not part of the target node itself, especially list items or object pairs.
- Keep `transform` and `rewriters` in the same skill-driven rewrite workflow.

## CLI Modes

- `--interactive` reviews rewrite results one-by-one. Interactive controls are `y` accept, `n` skip, `e` open in editor, and `q` quit.
- `--json=pretty|stream|compact` emits raw ast-grep JSON output. `pretty` is the default when a style is not specified. Use it when a shell pipeline needs native ast-grep data instead of VT Code’s normalized result objects.
- `--debug-query` is useful for isolated query iteration. In VT Code, prefer the public structural `debug_query` field before dropping to raw CLI.
- `--config <file>` runs project-config scan from that ast-grep root.
- `--rule <file>` runs one YAML rule file without project setup and conflicts with `--config`.
- `--inline-rules '...'` runs one or more inline YAML rules without writing a file and conflicts with `--rule`.
- `--filter <regex>` narrows project-config scan to matching rule ids and conflicts with `--rule`.
- `--include-metadata` only affects JSON output and is required when downstream tooling needs rule `metadata`.
- `ast-grep test --config <file>` runs tests from that ast-grep root config.
- `--test-dir <dir>` narrows test YAML discovery.
- `--snapshot-dir <dir>` changes the snapshot directory name from the default `__snapshots__`.
- `--filter <glob>` on `ast-grep test` narrows which test cases run.
- `--skip-snapshot-tests` is the fast validity-only mode and is the part VT Code exposes publicly on `workflow="test"`.
- `--include-off`, `--update-all`, and interactive snapshot review stay on the CLI path.
- `--stdin` lets ast-grep parse piped code, but:
  - it conflicts with `--interactive`
  - `run --stdin` requires `--lang`
  - `scan --stdin` requires exactly one rule via `--rule` / `-r`
  - stdin mode needs both `--stdin` and a non-TTY execution context
- `--globs` includes or excludes paths and overrides ignore-file behavior. Use `!pattern` to exclude, and expect the last matching glob to win.
- `--no-ignore` controls whether ast-grep honors `hidden`, `dot`, `exclude`, `global`, `parent`, or `vcs` ignore sources.
- `--follow` traverses symlinks and surfaces loop or broken-link errors directly.
- `--color never` is the direct CLI switch when raw ast-grep output must be plain and non-ANSI.
- `--format=github|sarif` is for CI/reporting integrations rather than VT Code’s normalized scan result objects.
- `--report-style=rich|medium|short` only changes ast-grep’s human-readable diagnostics.
- `--heading=auto|always|never` only changes human-readable output layout.
- `--error`, `--warning`, `--info`, `--hint`, and `--off` override rule severities for one scan run.
- `--inspect=summary|entity` prints discovery diagnostics to stderr without changing result data.
- `--threads <NUM>` sets approximate parallelism. `0` keeps ast-grep’s default heuristics.
- `-C/--context` shows symmetric surrounding lines. `-A/--after` and `-B/--before` are asymmetric alternatives and conflict with `--context`.
- `ast-grep run` exits `0` when at least one match is found and `1` when no matches are found. VT Code’s public structural query path normalizes that no-match case to an empty `matches` list.
- `ast-grep scan` exits `1` when at least one error-severity rule matches and `0` when no rules match. VT Code’s public structural scan path normalizes that result to structured `findings`.

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
  - Node.js NAPI: main experimental API surface today
  - Python: good for programmatic syntax-tree workflows
  - Rust `ast_grep_core`: most efficient and lowest-level option
- NAPI reference points:
  - use `parse(Lang.<X>, src)` to get `SgRoot`
  - use `kind(...)` and `pattern(...)` for kind lookup and compiled matcher helpers
  - `SgRoot.root()` returns `SgNode`
  - `SgNode` carries traversal, matcher checks, meta-variable access, and edit APIs such as `find`, `findAll`, `field`, `matches`, `inside`, `has`, `replace`, and `commitEdits`
  - `NapiConfig` is the programmatic config shape for `find` / `findAll`
- Python reference points:
  - `SgRoot(src, language)` is the entry point
  - `SgNode` mirrors the main inspection, refinement, traversal, search, and edit flows
- Deprecated API note:
  - language-specific JS objects like `js.parse(...)` are deprecated
  - prefer unified NAPI functions such as `parse(Lang.JavaScript, src)`
- JS/Python support for applying ast-grep `fix` directly is still experimental, so prefer explicit patch generation when you need dependable automation.
- If a task crosses this boundary, stop trying to encode it as more YAML and switch to a proper programmatic implementation.

Switch to direct CLI work through `unified_exec` when the task needs:

- `ast-grep --help`
- `ast-grep help`
- `ast-grep new`
- `ast-grep new project`
- `ast-grep new rule`
- `ast-grep new test`
- `ast-grep new util`
- `ast-grep run --debug-query`
- `ast-grep run --json`
- `ast-grep run --stdin --lang <lang>`
- `ast-grep scan --rule <file> <path>`
- `ast-grep scan -r <rule.yml>`
- `ast-grep scan --inline-rules '...' <path>`
- `ast-grep scan --stdin --rule <rule.yml>`
- `ast-grep test --test-dir <dir>`
- `ast-grep test --snapshot-dir <dir>`
- `ast-grep test --include-off`
- `ast-grep test --update-all`
- `ast-grep test --interactive`
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
