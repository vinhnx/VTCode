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
- Project discovery behavior:
  - raw ast-grep searches upward from the current working directory until it finds `sgconfig.yml`
  - `--config <file>` overrides discovery and pins the project root explicitly
  - a home-directory `sgconfig.yml` can act as a global fallback
  - XDG config-directory discovery is not part of ast-grep’s current behavior
- Scan versus run:
  - `ast-grep scan` requires project config and errors when no `sgconfig.yml` is found
  - `ast-grep run` can still operate without project config, but discovered config still affects features like `customLanguages` and `languageGlobs`
- Inspection:
  - use `ast-grep scan --inspect summary` to confirm the active project directory and config file path during discovery
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

### Rust Catalog Examples

- Avoid duplicated exports:
  - useful when a Rust crate exposes both `pub mod foo;` and `pub use foo::Foo;`
  - model it as a scan rule for API-surface review, not an automatic rewrite
- `chars().enumerate()` vs `char_indices()`:
  - good rewrite example when the code really needs byte offsets
  - do not apply blindly if the caller intentionally wants character positions
- `to_string().chars().count()` vs `checked_ilog10()`:
  - good Rust-specific performance rewrite when the expression is truly counting digits of an integer
  - do not over-apply if the expression is part of a more general formatting pipeline
- Unsafe function without unsafe block:
  - good scan rule for Rust review workflows
  - the catalog form uses `kind: function_item`, checks `function_modifiers` with `regex: "^unsafe"`, and rejects bodies containing `unsafe_block`
  - keep this as a diagnostic rule unless the repository already has a codemod-safe policy for removing `unsafe`
- `indoc!` rewrite:
  - useful as a formatting-sensitive rewrite example
  - keep this on the CLI skill path because the replacement should be reviewed for whitespace and raw-string fidelity
- When adapting Rust catalog rules in VT Code, preserve repository-specific API conventions, lint policy, and safe-by-default coding rules instead of importing the example verbatim.

### TypeScript Catalog Examples

- TypeScript vs TSX:
  - do not assume one parser fits both
  - use `languageGlobs` only when the repository intentionally wants `.ts` parsed as TSX
- Imports without file extensions:
  - good scan rule for ESM repositories that require explicit local file extensions
  - skip it in toolchains that intentionally resolve extensionless imports
- XState v4 to v5 migration:
  - strong example of multi-rule YAML with `utils`, `transform`, and multi-document config
  - keep it on the CLI skill path and review the migration diff carefully
- No `await` in `Promise.all([...])`:
  - good narrow rewrite when the `await` is directly inside the inline array
  - do not over-generalize it to logic that is intentionally sequential
- No console except allowed cases:
  - good scan rule for browser or client-facing TypeScript
  - adapt exceptions to repository policy before enabling it broadly
- Import usage and import identifier discovery:
  - useful for dependency analysis, cleanup, and codemod prep
  - often better as search/report rules than automatic rewrites
- Chai `should` to `expect`:
  - useful testing migration example
  - apply it only where the repository actually uses Chai and the target assertion style is settled
- Barrel-import splitting:
  - strong `rewriters` example for converting one import into many direct imports
  - keep it on the CLI skill path because export shape and path conventions vary
- Missing Angular `@Component()` decorator:
  - good labels example for framework-aware diagnostics
  - only use it in repositories that actually contain Angular lifecycle hooks
- Logical assignment operators:
  - useful concise rewrite when the project target and lint policy allow ES2021 operators
  - avoid it in compatibility-sensitive codebases
- When adapting TypeScript catalog rules in VT Code, preserve repository-specific module resolution, framework usage, transpilation target, and lint policy instead of importing the example verbatim.

### TSX Catalog Examples

- TSX vs TypeScript:
  - JSX-bearing rules should stay on the TSX parser
  - use `languageGlobs` only when the repository intentionally wants `.ts` parsed as TSX
- Unnecessary `useState<T>` primitives:
  - good cleanup rewrite for primitive generic arguments when inference already works
  - do not apply it when the generic is carrying real information that inference would lose
- `&&` short-circuit in JSX:
  - useful React-oriented rewrite from `{cond && <Node />}` to `{cond ? <Node /> : null}`
  - especially valuable when the left side can be `0` or another renderable falsy value
- MobX `observer` component style rewrite:
  - useful migration example for making hook linting more visible to tooling
  - keep it on the CLI skill path because the rewrite changes component shape and naming
- Unnecessary React hook:
  - good diagnostic rule for `use*` functions that do not actually call hooks
  - treat it as API-sensitive cleanup, not a blind rewrite
- Reverse React Compiler:
  - intentionally opinionated de-memoization rewrite
  - only use it when the user explicitly wants that behavior
- Nested links:
  - good JSX correctness and accessibility scan rule
  - especially useful in React or router-heavy component trees
- SVG attribute renaming:
  - good JSX compatibility rewrite for hyphenated SVG props such as `stroke-linecap`
  - keep it reviewable because generated markup and formatting may need cleanup
- When adapting TSX catalog rules in VT Code, preserve repository-specific React conventions, JSX runtime, lint rules, and browser-support policy instead of importing the example verbatim.

### YAML Catalog Examples

- Host/port message rule:
  - good example of a simple configuration scan rule that emits a message when matching `host` or `port` entries
  - treat hard-coded values like `8000` as repository policy, not as generally correct defaults
- YAML config checks:
  - useful for enforcing deployment, service, or environment conventions
  - if the policy depends on a key/value relationship rather than either field alone, strengthen the rule instead of relying on separate loose matches
- When adapting YAML catalog rules in VT Code, preserve repository-specific config policy and avoid baking arbitrary environment assumptions into the rule text.

### Ruby Catalog Examples

- Rails `before_filter` / `after_filter` / `around_filter` migration:
  - useful Rails upgrade rewrite from deprecated `*_filter` names to `*_action`
  - keep it on the CLI skill path and review the diff because framework version and controller conventions vary
- Symbol over proc:
  - good Ruby cleanup rewrite for enumerable calls like `map`, `select`, or `each`
  - apply it only when the shorthand stays idiomatic and readable in the target codebase
- Path traversal detection:
  - good security-oriented scan rule for Rails path construction and `send_file`
  - treat matches as review candidates, not conclusive vulnerabilities, because sanitization or allowlisting may happen elsewhere
- When adapting Ruby catalog rules in VT Code, preserve repository-specific Rails versioning, Ruby style conventions, and security policy instead of importing the example verbatim.

### Python Catalog Examples

- OpenAI SDK migration:
  - good multi-rule migration example for updating legacy `openai` imports, client initialization, and completion calls
  - keep it on the CLI skill path because API migrations often require response-shape, auth, and application-flow review beyond a blind rewrite
- Generator-expression rewrites:
  - good example of restricting a rewrite to iterator-accepting contexts such as `any`, `all`, or `sum`
  - do not expand it to every list comprehension unless the surrounding use site is proven generator-safe
- Walrus-operator rewrite:
  - useful paired-rule example that rewrites the `if` and removes the preceding temporary assignment
  - only apply it where the repository requires Python 3.8+ and accepts assignment-expression style
- Remove async function:
  - strong `rewriters` example for transforming nested `await` sites inside a matched async function body
  - treat it as a semantics-changing migration and review all callers before using it broadly
- Pytest fixture refactors:
  - useful `utils`-driven example for renaming fixtures or adding fixture type hints without touching similarly named non-test code
  - keep it scoped to real pytest contexts so ordinary production functions are not swept in
- PEP 604 typing rewrites:
  - useful modernization examples for `Optional[T]` and nested `Union[...]` rewrites
  - only use them when the repository’s Python version floor and typing policy explicitly support `T | None` and `|`
- SQLAlchemy annotated mapping rewrite:
  - useful ORM migration example for moving `mapped_column(String, nullable=True)` toward `Mapped[str | None]`
  - keep it reviewable because SQLAlchemy version, declarative style, and column semantics vary across repositories
- When adapting Python catalog rules in VT Code, preserve repository-specific Python version support, framework conventions, typing standards, async behavior, and migration boundaries instead of importing the example verbatim.

### Kotlin Catalog Examples

- Clean-architecture import rule:
  - good example of enforcing architecture boundaries by combining import matching with `files`-scoped targeting
  - treat it as repository-policy enforcement and adapt both the package regexes and file globs to the real module layout
- Kotlin architectural scans:
  - best suited for diagnostic use, not blind rewrites
  - route violations through review because imports across layers often reflect design decisions that need context
- When adapting Kotlin catalog rules in VT Code, preserve repository-specific package names, architecture boundaries, Android layering, and ownership of lint policy instead of importing the example verbatim.

### Java Catalog Examples

- Unused local variable rule:
  - useful educational example of ordered `all` constraints with `has` and `precedes`
  - do not treat it as a replacement for compiler or IDE diagnostics because it intentionally simplifies Java variable-declaration coverage
- Find `String` field declarations:
  - good structural scan example for matching `field_declaration` nodes by their typed child instead of relying on fragile surface syntax
  - especially useful when modifiers or annotations make naive patterns fail to parse
- Java catalog usage in VT Code:
  - keep these rules review-oriented unless the repository explicitly wants ast-grep-based cleanup
  - preserve repository-specific annotation style, package structure, and static-analysis tooling

### HTML Catalog Examples

- HTML parser for framework templates:
  - useful for Vue, Svelte, Astro, and similar template files when the syntax is mostly HTML
  - do not assume it covers framework-specific frontmatter or control-flow syntax; switch to a custom language when parser gaps matter
- Ant Design Vue attribute migration:
  - good enclosing-tag rewrite example for renaming `:visible` to `:open` only on the intended popup components
  - keep it on the CLI skill path because framework version and component-library conventions need confirmation first
- i18n extraction:
  - good template rewrite example for static text while skipping mustache expressions
  - keep it reviewable because real i18n flows usually need key naming, dictionary generation, and whitespace cleanup around the rewrite
- When adapting HTML catalog rules in VT Code, preserve repository-specific template framework conventions, parser limitations, i18n workflow, and component-library versions instead of importing the example verbatim.

### Go Catalog Examples

- Problematic `defer` statements:
  - good Go-specific correctness scan for deferred calls whose nested arguments run immediately instead of at function exit
  - treat matches as review candidates and prefer closure-based rewrites only after checking local test and style conventions
- Function-name pattern scans:
  - good example of combining `kind`, `has`, and `regex` to find declarations such as `Test.*`
  - useful for test discovery, migration targeting, or repository audits where meta-variable patterns are too limited
- Contextual function-call matching:
  - good example of using `context` plus `selector: call_expression` to avoid Go parser ambiguity between calls and type conversions
  - use this pattern whenever a plain call-expression pattern under-matches or parses unexpectedly
- Package-import scans:
  - useful for dependency auditing, banned-import enforcement, or migration prep
  - adapt the import regex to the repository’s real package policy instead of copying the sample package name
- JSON tag `-,` detection:
  - high-signal security scan for Go struct tags that look omitted but still allow unmarshaling through the `-` key
  - treat these matches as actionable security review items rather than optional style cleanup
- When adapting Go catalog rules in VT Code, preserve repository-specific Go version support, test conventions, package structure, security policy, and existing linter coverage instead of importing the example verbatim.

### Cpp Catalog Examples

- Format-string vulnerability rewrite:
  - good security-focused rewrite example for `fprintf` or `sprintf` calls that pass attacker-controlled or variable text as the format argument
  - keep it reviewable because some repositories should migrate to safer APIs or broader hardening patterns instead of applying only a local `"%s"` rewrite
- Struct inheritance matching:
  - good example of why C++ AST-based patterns often need the full syntactic shape instead of a shortened surface snippet
  - use the full `struct ... : ... { ... }` pattern or switch to a YAML rule when a smaller pattern parses as `ERROR`
- Reusing Cpp rules with C:
  - only do this when the repository intentionally parses `.c` as Cpp via `languageGlobs`
  - preserve repository-specific parser expectations in mixed-language trees because C and C++ syntax compatibility is not free
- When adapting Cpp catalog rules in VT Code, preserve repository-specific parser choice, security policy, libc conventions, and style/tooling expectations instead of importing the example verbatim.

### C Catalog Examples

- Match function-call patterns:
  - good example of using `context` plus `selector: call_expression` to work around tree-sitter-c fragment ambiguity
  - prefer this form whenever a plain `foo($A)` pattern parses incorrectly as a macro-related node instead of a call
- Method-style to function-call rewrites:
  - useful migration example for C codebases that simulate methods on structs
  - keep it on the CLI skill path because rewriting `$R.$METHOD(...)` to `$METHOD(&$R, ...)` changes the call shape and should be reviewed with local pointer and API conventions in mind
- Yoda-condition rewrite:
  - clearly style-oriented example for `if` comparisons with numeric literals
  - only use it when the repository explicitly prefers that comparison style, not as a default cleanup
- Parsing C as Cpp:
  - acceptable when the repository intentionally uses `languageGlobs` to share rule definitions
  - preserve parser expectations carefully in mixed C/C++ trees because upstream parse behavior differs
- When adapting C catalog rules in VT Code, preserve repository-specific parser choice, macro behavior, pointer conventions, and coding-style policy instead of importing the example verbatim.

### JavaScript API Usage

- Escalate from YAML or CLI rules to `@ast-grep/napi` when the task needs:
  - conditional replacements based on surrounding nodes
  - match ordering or counting in later decisions
  - replacement strings computed in host code instead of static `fix` text
  - explicit edit batching and source regeneration
- Core JS API flow:
  - use `parse(Lang.<X>, source)` to build `SgRoot`
  - call `root()` to get `SgNode`
  - use `find` or `findAll` with a pattern, kind id, or `NapiConfig`
  - inspect captures with `getMatch` or `getMultipleMatches`
  - generate edits with `replace(...)` and apply them with `commitEdits(...)`
- Important VT Code caveat:
  - JS API `replace` does not interpolate metavariables the way CLI `fix` does
  - build replacement strings explicitly from matched nodes before committing edits
- Traversal and refinement:
  - use `children`, `parent`, `field`, `ancestors`, `next`, and related methods for structural navigation
  - use `matches`, `inside`, `has`, `precedes`, and `follows` as post-match refinement checks
- Keep the boundary clear in VT Code:
  - prefer the public structural tool for ordinary query/scan flows
  - prefer CLI execution for standard ast-grep rule files and rewrites
  - use NAPI only when the task is truly programmatic
- Dynamic-language registration exists through `registerDynamicLanguage`, but treat it as experimental and prefer established language support first.

### Python API Usage

- Escalate from YAML or CLI rules to `ast-grep-py` when:
  - the repository already has strong Python automation around the change
  - replacement text must be computed in host code
  - traversal and post-match filtering are easier to express imperatively than in rule YAML
  - explicit edit objects and source regeneration are preferable to shelling out through the CLI
- Core Python API flow:
  - use `SgRoot(source, language)` to parse source text
  - call `root()` to get `SgNode`
  - use `find` or `find_all` with direct rule kwargs or a config object
  - inspect captures with `get_match`, `get_multiple_matches`, or `__getitem__`
  - generate edits with `replace(...)` and apply them with `commit_edits(...)`
- Important VT Code caveat:
  - Python API `replace` does not interpolate metavariables the way CLI `fix` does
  - build replacement strings explicitly from matched nodes before committing edits
- Traversal and refinement:
  - use `field`, `parent`, `child`, `children`, `ancestors`, `next`, and `prev` for structural navigation
  - use `matches`, `inside`, `has`, `precedes`, and `follows` as refinement helpers after locating a node
- Keep the boundary clear in VT Code:
  - prefer the public structural tool for routine query/scan behavior
  - prefer CLI execution for normal ast-grep rule authoring and rewrites
  - use Python API only when the task is truly programmatic and Python is the most practical host

### NAPI Performance Usage

- Optimize for fewer FFI crossings first:
  - prefer one `findAll(...)` over JavaScript-side recursive traversal that repeatedly calls `kind()`, `children()`, and other node methods
  - prefer letting ast-grep evaluate the matcher in Rust instead of walking the tree imperatively in host code
- Parsing strategy:
  - use `parseAsync(...)` when parsing many sources can benefit from Node’s libuv thread pool
  - keep simple one-off tasks on synchronous `parse(...)` unless concurrency actually helps
- Multi-file scanning:
  - prefer `findInFiles(...)` when the task is fundamentally file-oriented and you want Rust-side parallel parsing and matching
  - use `FindConfig.paths` plus `matcher` instead of enumerating files in JavaScript and handing each source back to Rust one by one
- Important `findInFiles` caveat:
  - the returned promise may resolve before every callback has fired
  - if exact completion matters, maintain your own callback counter and wait until processed callbacks equal the returned file count
- VT Code boundary:
  - apply these optimizations only for large scans or performance-sensitive automations
  - for small repos, one-off scripts, or debugging tasks, prefer simpler code over premature concurrency or callback bookkeeping

## Rule Essentials

- A minimal rule file starts with `id`, `language`, and root `rule`.
- The root `rule` matches one target node per result. Meta variables come from that matched node and its substructure.
- A root rule object needs at least one positive anchor. In practice, start with `pattern` or `kind`; do not expect `regex` alone to define the target node shape.
- Atomic rules are `pattern`, `kind`, `regex`, `nthChild`, and `range`.
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
  - use pattern objects when the raw snippet is invalid, incomplete, or ambiguous for the parser
  - `context` is required, `selector` chooses the actual target node inside that context, and `strictness` tunes how literally the match behaves
  - `kind` also accepts limited ESQuery-style syntax in newer ast-grep versions.
  - separate `kind` and `pattern` clauses do not change how the pattern parses; if parse shape is wrong, move to one pattern object with `context` and `selector`
  - `regex` matches the whole node text.
  - regex syntax is Rust `regex`, not PCRE, so do not assume arbitrary look-around or backreferences
  - pair `regex` with a structural anchor like `kind` or `pattern` when possible so the expensive text match runs on the right nodes
  - Use `nthChild` when the target is defined by its named-sibling position.
  - `nthChild` accepts a number, an `An+B` formula string, or an object with `position`, `reverse`, and `ofRule`.
  - Use `range` when the match must stay inside a specific source span.
  - `range` uses 0-based `line` and `column`, with inclusive `start` and exclusive `end`
- Relational rules describe where the target sits relative to other nodes.
  - read them as target relates to surrounding
  - the top-level rule still matches the target node, while the relational subrule matches the surrounding node that filters it
  - Use `inside` and `has` for ancestor or descendant requirements.
  - relational subrules can still use patterns, composites, and captures, and those captures can be reused later in `fix`
  - Use relational `field` when the semantic role matters, such as matching only a `body`. `field` is only available on `inside` and `has`.
  - Use `stopBy` when traversal should continue past the nearest boundary instead of stopping at the default scope edge.
  - `stopBy: neighbor` is the default. `end` searches to the edge, and a rule-object stop is inclusive.
  - `inside` means target under matching ancestor; `has` means target contains matching descendant; `follows` means target comes after matching surrounding node; `precedes` means target comes before one
- Composite rules combine multiple checks on the same target node.
  - `all` means every sub-rule must match.
  - `any` means at least one sub-rule must match.
  - `not` excludes a sub-rule.
  - `matches` reuses a named utility rule.
  - `all` / `any` combine rules for one node, not multiple target nodes.
  - `all` is the ordered composite; use it when later checks depend on meta-variable captures from earlier pattern checks.
  - `any` is for alternatives, not for collecting multiple matching nodes.
  - `has: { all: [...] }` still means one child satisfies every listed rule, which is often impossible for incompatible kinds like `number` and `string`.
  - If the intent is "target has X child and has Y child", keep `all` outside and place one `has` clause per required child shape.
- Utility rules are reusable rule definitions.
  - Use local `utils` in the current config file for nearby reuse.
  - Use global utility-rule files when several rules across the project need the same logic.
  - Local utilities only exist inside the file where they are declared, inherit that file's language, and do not get their own separate `constraints`.
  - Global utility-rule files are loaded through `utilDirs` and are limited to `id`, `language`, `rule`, `constraints`, and local `utils`.
  - Local utility names shadow global utilities of the same name, so resolve `matches` from the current file outward.
  - Utilities can reuse other utilities through `matches`, including recursive patterns for nested syntax, but cyclic `matches` dependency graphs are invalid.
  - Recursive structure through relational traversal like `inside` or `has` is still valid when the search moves across AST nodes instead of re-entering the same `matches` expansion.
- Move from a simple `pattern` to a full rule object when the task needs positional constraints, semantic roles, reusable sub-rules, or several structural conditions on the same node.
  - Rule-object fields are effectively an unordered `all`, so use them to flatten independent checks.
  - When capture or evaluation order matters, keep an explicit `all` list instead of relying on rule-object field order.

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
  - prefer file-local `utils` first; promote helpers into global utility files only when reuse crosses rule-file boundaries
- Patching keys define automatic fixes.
  - `transform` derives replacement variables before `fix`.
  - `fix` can be a plain replacement string or a `FixConfig` object using `template`, `expandStart`, and `expandEnd`.
  - `rewriters` are for more complex reusable rewrite building blocks.
- Linting keys define how the rule reports findings.
  - `severity`, `message`, `note`, and `labels` shape the diagnostic output.
  - severity levels are `error`, `warning`, `info`, `hint`, and `off`
  - `hint` is the default severity in ast-grep scans
  - `error` findings make raw `ast-grep scan` exit non-zero
  - `severity: off` disables the rule in scan results.
  - `note` supports Markdown but does not interpolate meta variables.
  - `labels` must reference meta variables defined by the rule or `constraints`.
  - `files` scopes the included files and `ignores` scopes the excluded files.
  - `files` also supports object syntax when a glob needs flags such as `caseInsensitive`.
  - `ignores` is checked before `files`.
  - both `files` and `ignores` are relative to the directory containing `sgconfig.yml`
  - do not prefix these globs with `./`
  - YAML `ignores` is different from CLI `--no-ignore`
- Suppression comments:
  - `ast-grep-ignore` suppresses all diagnostics for the same line or next line
  - `ast-grep-ignore: rule-id` suppresses one named rule
  - comma-separated ids suppress multiple named rules
  - next-line suppression only works when the suppression line has no preceding AST node
- File-level suppression:
  - requires the suppression comment on the first line
  - requires the second line to be empty
- Unused suppressions:
  - `unused-suppression` behaves like a hint-style built-in rule with autofix
  - it only appears on broad `scan` runs when rules are not narrowed or disabled through flags like `--off`, `--rule`, `--inline-rules`, or `--filter`
- Rule `metadata` only appears in ast-grep JSON output when metadata inclusion is enabled, for example with `--include-metadata`.
- Keep this YAML-config work on the skill path. VT Code’s public structural tool can run read-only scans and tests against these configs, but it does not surface the config schema as tool-call arguments.

## Transformation Objects

- `transform` derives strings from captured meta variables before `fix` applies.
- Each transform key creates a new variable name without `$`; `source` still references captured or previously transformed values with `$VAR` syntax.
- Transform order matters. Later transforms can consume earlier transform outputs, which is how multi-step rewrites like case-convert -> regex replace -> case-convert work.
- `replace` is regex-based text replacement over one captured meta variable.
  - `source` must be `$VAR` style
  - `replace` is the Rust regex
  - `by` is the replacement string
  - capture groups from the regex can be reused in `by`
  - those capture groups are only available inside that same `replace` transform; regular `regex` rules do not expose them
- `substring` is Python-style slicing over Unicode characters.
  - `startChar` is inclusive
  - `endChar` is exclusive
  - negative indexes count backward from the end of the string
- `convert` changes identifier casing through `toCase`.
  - common targets: `camelCase`, `snakeCase`, `kebabCase`, `pascalCase`, `lowerCase`, `upperCase`, `capitalize`
  - `separatedBy` controls how the source string is split into words before conversion
- `CaseChange` is the separator for transitions like `astGrep`, `ASTGrep`, or `XMLHttpRequest`
- Ast-grep also accepts string-form transforms such as `replace(...)`, `substring(...)`, `convert(...)`, and `rewrite(...)`.
- String-form transforms require ast-grep 0.38.3+. Use object form when version compatibility or debugging clarity matters.
- For conditional commas, whitespace, or similar glue text, derive a helper transform such as `MAYBE_COMMA` from a possibly empty multi-capture instead of hard-coding punctuation directly into `fix`.

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
- Raw JSON match objects include `text`, `range`, `file`, `lines`, optional `replacement`, optional `replacementOffsets`, and optional `metaVariables`. Scan-mode rule matches add `ruleId`, `severity`, `message`, optional `note`, and optionally `metadata` when metadata output is enabled.
- ast-grep JSON range positions are zero-based for line, column, and byte offsets. Preserve that convention when adapting output into editor or diagnostic surfaces.
- Prefer `--json=stream` for large result sets or downstream line-by-line processing. `pretty` and `compact` both emit a full JSON array, which is simpler to inspect but less streaming-friendly.
- Use the equals-sign form for styled JSON output. `--json stream` is not the same as `--json=stream`.
- `--debug-query` is useful for isolated query iteration. In VT Code, prefer the public structural `debug_query` field before dropping to raw CLI.
- `--config <file>` runs project-config scan from that ast-grep root.
- `--rule <file>` runs one YAML rule file without project setup and conflicts with `--config`.
- `--inline-rules '...'` runs one or more inline YAML rules without writing a file and conflicts with `--rule`.
- `--filter <regex>` narrows project-config scan to matching rule ids and conflicts with `--rule`.
- `--include-metadata` only affects JSON output and is required when downstream tooling needs rule `metadata`.
- `ast-grep test --config <file>` runs tests from that ast-grep root config.
- Test files are keyed by rule `id` and use `valid` plus `invalid` code lists. `valid` should stay quiet; `invalid` should trigger the rule.
- Ast-grep test failures are categorized, so expect terms like `noisy` for false positives and `missing` for false negatives when debugging rule behavior.
- `--test-dir <dir>` narrows test YAML discovery.
- `--snapshot-dir <dir>` changes the snapshot directory name from the default `__snapshots__`.
- `--filter <glob>` on `ast-grep test` narrows which test cases run.
- `--skip-snapshot-tests` is the fast validity-only mode and is the part VT Code exposes publicly on `workflow="test"`.
- `--include-off`, `--update-all`, and interactive snapshot review stay on the CLI path.
- `--update-all` creates or refreshes snapshot baselines, and `--interactive` is the selective review flow for accepting changed snapshots.
- Snapshot tests cover output details in addition to pass/fail validity, which is why skipping them is useful while a rule’s matching logic is still in flux.
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
- `--error`, `--warning`, `--info`, `--hint`, and `--off` override severities for one scan run and stay on the CLI path.
- `--inspect entity` is the fastest way to debug each rule’s final active severity after config and CLI overrides.
- `unused-suppression` can itself be severity-overridden on the CLI when a repo wants to escalate stale ignores in CI.
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
