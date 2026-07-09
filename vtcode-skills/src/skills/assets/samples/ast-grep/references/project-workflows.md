# Ast-Grep Project Workflows

Use the public structural tool first for read-only project checks:

- `workflow="scan"` maps to `sg scan <path> --config <config_path> --json=stream --include-metadata --color=never`
- `workflow="test"` maps to `sg test --config <config_path>`
- `workflow="rewrite"` maps to `sg run --pattern=... --rewrite=... --json=compact --color=never` (dry-run preview, no files modified)
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
  - uses `all` to combine a `pub use $A::$B;` pattern with `inside: { kind: source_file }` and a `has` check for `pub mod $A;` with `stopBy: end`:

```yaml
id: avoid-duplicate-export
language: Rust
severity: warning
message: Item re-exported via `pub use` when `pub mod` already exposes the module.
rule:
  all:
    - pattern: "pub use $A::$B;"
    - inside:
        kind: source_file
    - has:
        pattern: "pub mod $A;"
        stopBy: end
```

- `chars().enumerate()` vs `char_indices()`:
  - good rewrite example when the code really needs byte offsets
  - do not apply blindly if the caller intentionally wants character positions
  - simple pattern-based rewrite rule:

```yaml
id: no-chars-enumerate
language: Rust
severity: warning
message: Use `.char_indices()` instead of `.chars().enumerate()` when byte offsets are needed.
rule:
  pattern: "$A.chars().enumerate()"
fix: "$A.char_indices()"
```

- `to_string().chars().count()` vs `checked_ilog10()`:
  - good Rust-specific performance rewrite when the expression is truly counting digits of an integer
  - do not over-apply if the expression is part of a more general formatting pipeline
  - simple pattern-based rewrite rule:

```yaml
id: no-alloc-digit-count
language: Rust
severity: info
message: Count integer digits without heap allocation.
rule:
  pattern: "$NUM.to_string().chars().count()"
fix: "$NUM.checked_ilog10().unwrap_or(0) + 1"
```

- Unsafe function without unsafe block:
  - good scan rule for Rust review workflows
  - the catalog form uses `kind: function_item`, checks `function_modifiers` with `regex: "^unsafe"`, and rejects bodies containing `unsafe_block`
  - keep this as a diagnostic rule unless the repository already has a codemod-safe policy for removing `unsafe`
  - uses `all` with `kind`, `has`, and `not` to detect the pattern:

```yaml
id: no-unsafe-fn-without-unsafe
language: Rust
severity: warning
message: Unsafe function contains no `unsafe` block.
rule:
  all:
    - kind: function_item
    - has:
        kind: function_modifiers
        regex: "^unsafe"
    - not:
        has:
          kind: unsafe_block
          stopBy: end
```

- Rust 2024 let-chain candidate:
  - good `hint`-severity suggestion rule for nested `if`/`if let` structures
  - uses `utils` to define reusable matchers for sole-child statements, no-else `if` expressions, and no-else `if let` expressions
  - the root rule matches an `if` whose block contains only another `if` statement
  - only refactor when the project's MSRV is Rust 2024 edition or later:

```yaml
id: let-chain-candidate
language: Rust
severity: hint
message: Nested `if`/`if let` can be collapsed into a Rust 2024 let-chain.
utils:
  sole-child:
    all:
      - nthChild: 1
      - nthChild: { position: 1, reverse: true }
  if-no-else:
    kind: if_expression
    not: { has: { field: alternative, kind: else_clause } }
  if-let-no-else:
    matches: if-no-else
    has: { field: condition, kind: let_condition }
  sole-inner-if-stmt:
    kind: expression_statement
    matches: sole-child
    has: { matches: if-no-else }
  sole-inner-if-let-stmt:
    kind: expression_statement
    matches: sole-child
    has: { matches: if-let-no-else }
rule:
  matches: if-no-else
  has:
    field: consequence
    kind: block
    has: { matches: sole-inner-if-stmt }
  any:
    - matches: if-let-no-else
    - has:
        field: consequence
        kind: block
        has: { matches: sole-inner-if-let-stmt }
```

- `indoc!` rewrite:
  - useful as a formatting-sensitive rewrite example
  - keep this on the CLI skill path because the replacement should be reviewed for whitespace and raw-string fidelity
  - CLI pattern: `ast-grep --pattern 'indoc! { r#"$$$A"# }' --rewrite '`$$$A`'`
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
  - uses three rules separated by `---`: import rewrite, client initialization, and completion method update
- Generator-expression rewrites:
  - good example of restricting a rewrite to iterator-accepting contexts such as `any`, `all`, or `sum`
  - do not expand it to every list comprehension unless the surrounding use site is proven generator-safe
  - the constraint-based variant restricts `$FUNC` with `regex: ^(any|all|sum)$` and `$LIST` with `kind: list_comprehension`, then strips brackets with a `substring` transform:

```yaml
id: prefer-generator-in-builtins
language: python
rule:
  pattern: $FUNC($LIST)
  constraints:
    FUNC:
      regex: ^(any|all|sum)$
    LIST:
      kind: list_comprehension
transform:
  INNER:
    substring:
      source: $LIST
      startChar: 1
      endChar: -1
fix: $FUNC($INNER)
```

- Walrus-operator rewrite:
  - useful paired-rule example that rewrites the `if` and removes the preceding temporary assignment
  - only apply it where the repository requires Python 3.8+ and accepts assignment-expression style
  - uses `follows` to detect the assignment before the `if`, and a second rule with `precedes` to delete the redundant assignment:

```yaml
id: use-walrus-operator
language: python
rule:
  follows:
    pattern:
      context: $VAR = $$$EXPR
      selector: expression_statement
  pattern: "if $VAR: $$$B"
fix: |-
  if $VAR := $$$EXPR:
      $$$B
---
id: remove-walrus-source
language: python
rule:
  pattern: $VAR = $$$EXPR
  kind: expression_statement
  precedes:
    pattern: "if $VAR: $$$B"
fix: ‘’
```

- Remove async function:
  - strong `rewriters` example for transforming nested `await` sites inside a matched async function body
  - treat it as a semantics-changing migration and review all callers before using it broadly
  - uses a `rewriter` named `remove-await-call` to strip `await` from each call inside the body, then the outer rule removes `async`:

```yaml
id: remove-async
language: python
rule:
  pattern:
    context: ‘async def $FUNC($$$ARGS): $$$BODY’
    selector: function_definition
rewriters:
  remove-await-call:
    pattern: ‘await $$$CALL’
    fix: $$$CALL
transform:
  REMOVED_BODY:
    rewrite:
      rewriters: [remove-await-call]
      source: $$$BODY
fix: |-
  def $FUNC($$$ARGS):
      $REMOVED_BODY
```

- Pytest fixture refactors:
  - useful `utils`-driven example for renaming fixtures or adding fixture type hints without touching similarly named non-test code
  - keep it scoped to real pytest contexts so ordinary production functions are not swept in
  - uses `utils` to define `is-fixture-function` (function following a `@pytest.fixture` decorator) and `is-test-function` (function whose name starts with `test_`)
- PEP 604 typing rewrites:
  - useful modernization examples for `Optional[T]` and nested `Union[...]` rewrites
  - only use them when the repository’s Python version floor and typing policy explicitly support `T | None` and `|`
  - the simple variant uses `context` and `selector` to disambiguate `Optional[$T]` as a generic type:

```yaml
id: optional-to-union
language: python
rule:
  pattern:
    context: ‘a: Optional[$T]’
    selector: generic_type
fix: $T | None
```

  - the recursive variant handles nested `Union` and `Optional` types using multiple `rewriters` that call each other
- SQLAlchemy annotated mapping rewrite:
  - useful ORM migration example for moving `mapped_column(String, nullable=True)` toward `Mapped[str | None]`
  - keep it reviewable because SQLAlchemy version, declarative style, and column semantics vary across repositories
  - uses `rewriters` to filter out `String` positional args and `nullable=True` keyword args from the argument list
- `print` detection:
  - use `kind: call` with `has: { field: function, pattern: print }` to match `print()` calls
  - scope with `files` to exclude test directories and scripts where console output is acceptable
- `isinstance` tuple consolidation:
  - pattern `isinstance($X, $A) or isinstance($X, $B)` can be rewritten to `isinstance($X, ($A, $B))`
  - safe autofix when both `isinstance` calls check the same variable
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
  - the rule uses `all` to guarantee that `$IDENT` is captured by the first `has` before the `not`/`precedes` check runs:

```yaml
id: no-unused-vars
language: java
rule:
  kind: local_variable_declaration
  all:
    - has:
        has:
          kind: identifier
          pattern: $IDENT
    - not:
        precedes:
          stopBy: end
          has:
            stopBy: end
            any:
              - { kind: identifier, pattern: $IDENT }
              - { has: { kind: identifier, pattern: $IDENT, stopBy: end } }
fix: ''
```

  - treat matches as review candidates because Java variable scopes are broader than this sample covers

- Find `String` field declarations:
  - good structural scan example for matching `field_declaration` nodes by their typed child instead of relying on fragile surface syntax
  - especially useful when modifiers or annotations make naive patterns fail to parse
  - a naive `String $F;` pattern fails because it ignores modifiers; `$MOD String $F;` also fails because tree-sitter rejects `$MOD` as an invalid modifier and produces an `ERROR` node
  - the structural approach with `kind` plus `has` plus `field` plus `regex` works regardless of how many modifiers or annotations precede the type:

```yaml
id: find-field-with-type
language: java
rule:
  kind: field_declaration
  has:
    field: type
    regex: ^String$
```

  - use this pattern whenever a naive code pattern fails because Java modifiers, annotations, or access qualifiers change the surface syntax

- Java catalog usage in VT Code:
  - keep these rules review-oriented unless the repository explicitly wants ast-grep-based cleanup
  - preserve repository-specific annotation style, package structure, and static-analysis tooling

### HTML Catalog Examples

- HTML parser for framework templates:
  - useful for Vue, Svelte, Astro, and similar template files when the syntax is mostly HTML
  - do not assume it covers framework-specific frontmatter or control-flow syntax; switch to a custom language when parser gaps matter
  - use `languageGlobs` in `sgconfig.yml` to map framework extensions (`.vue`, `.svelte`, `.astro`) to the HTML parser when the content is mostly HTML
- Key HTML node kinds: `element`, `tag_name`, `attribute_name`, `attribute_value`, `text`, `comment`. Use these with `kind` to match specific HTML structures without writing full pattern syntax.
- Matching elements by tag name:
  - use `kind: element` with `has: { field: tag_name, pattern: $TAG }` to match elements by tag
  - for regex-based tag matching, use `kind: tag_name` with `regex` and `inside: { kind: element }`
  - example: find all heading elements:

```yaml
id: find-headings
language: html
rule:
  kind: element
  has:
    field: tag_name
    regex: "^h[1-6]$"
```

- Matching elements by attribute:
  - use `kind: element` with `has: { kind: attribute_name, regex: "^class$" }` to find elements with a specific attribute
  - to also match the value, add a nested `has` on the attribute node to capture `attribute_value`
  - example: find elements with `data-testid` attributes:

```yaml
id: find-test-elements
language: html
rule:
  kind: element
  has:
    kind: attribute_name
    regex: "^data-testid$"
```

- Scoping with `inside` and `stopBy`:
  - HTML `inside` with `stopBy: { kind: element }` scopes matches to the nearest enclosing element
  - the catalog's `inside-tag` utility demonstrates wrapping `inside` with `kind: element` and `has` to capture the enclosing tag name, then using `constraints` to restrict which tags match
  - example utility rule:

```yaml
utils:
  inside-tag:
    inside:
      kind: element
      stopBy: { kind: element }
      has:
        field: tag_name
        pattern: $TAG_NAME
```

- Ant Design Vue attribute migration:
  - good enclosing-tag rewrite example for renaming `:visible` to `:open` only on the intended popup components
  - uses `kind: attribute_name` with `regex: :visible`, `inside` to find the enclosing `element`, `has` to capture the `tag_name`, and `constraints` to restrict to `a-modal|a-tooltip`
  - keep it on the CLI skill path because framework version and component-library conventions need confirmation first
  - example YAML:

```yaml
id: antd-visible-to-open
language: html
rule:
  kind: attribute_name
  regex: ":visible"
  matches: inside-tag
constraints:
  TAG_NAME:
    regex: "a-modal|a-tooltip"
fix: ":open"
utils:
  inside-tag:
    inside:
      kind: element
      has:
        field: tag_name
        pattern: $TAG_NAME
```

- i18n extraction:
  - good template rewrite example for static text while skipping mustache expressions
  - uses `kind: text` with `pattern: $T` to capture text content, `not: { regex: '{{.*}}' }` to skip mustache interpolation
  - keep it reviewable because real i18n flows usually need key naming, dictionary generation, and whitespace cleanup around the rewrite
  - example YAML:

```yaml
id: extract-i18n-keys
language: html
rule:
  kind: text
  pattern: $T
  not:
    regex: "\\{\\{.*\\}\\}"
fix: "{{ $('$T') }}"
```

- HTML comment scanning:
  - use `kind: comment` with `regex` to find comments containing specific markers like TODO, FIXME, or deprecated notices
  - useful for documentation audits and cleanup tasks

- Embedded language regions:
  - HTML `<script>` content is parsed as JavaScript; `<style>` content is parsed as CSS
  - search inside these regions with `lang: javascript` or `lang: css` rules
  - for custom embedded languages (e.g. TypeScript in `<script lang="ts">`), configure `languageInjections` in `sgconfig.yml`

- When adapting HTML catalog rules in VT Code, preserve repository-specific template framework conventions, parser limitations, i18n workflow, and component-library versions instead of importing the example verbatim.

### Go Catalog Examples

- Problematic `defer` statements:
  - good Go-specific correctness scan for deferred calls whose nested arguments run immediately instead of at function exit
  - Go evaluates `defer` arguments at the defer statement, not at function exit, so `defer require.NoError(t, failpoint.Disable(...))` disables the failpoint immediately
  - uses `context` plus `selector: defer_statement` to match deferred calls with nested function arguments:

```yaml
id: problematic-defer-call
language: go
rule:
  pattern:
    context: ‘{ defer $A.$B(t, failpoint.$M($$$)) }’
    selector: defer_statement
```

  - treat matches as review candidates and prefer closure-based rewrites: `defer func() { ... }()`
  - contextual patterns require the CLI skill path; the public structural surface does not support the `context` field

- Function-name pattern scans:
  - good example of combining `kind`, `has`, and `regex` to find declarations such as `Test.*`
  - a plain `Test$_` meta-variable pattern is not valid syntax; use a YAML rule with `regex` instead:

```yaml
id: test-functions
language: go
rule:
  kind: function_declaration
  has:
    field: name
    regex: Test.*
```

  - useful for test discovery, migration targeting, or repository audits where meta-variable patterns are too limited

- Contextual function-call matching:
  - tree-sitter-go parses `fmt.Println($A)` as a type conversion, not a call expression
  - use `context` plus `selector: call_expression` to disambiguate:

```yaml
id: match-function-call
language: go
rule:
  pattern:
    context: ‘func t() { fmt.Println($A) }’
    selector: call_expression
```

  - contextual patterns require the CLI skill path; the public structural surface’s `selector` field works with simple string patterns but does not support `context`

- Package-import scans:
  - useful for dependency auditing, banned-import enforcement, or migration prep
  - adapt the import regex to the repository’s real package policy instead of copying the sample package name:

```yaml
id: match-package-import
language: go
rule:
  kind: import_spec
  has:
    regex: github.com/golang-jwt/jwt
```

- JSON tag `-,` detection:
  - high-signal security scan for Go struct tags that look omitted but still allow unmarshaling through the `-` key
  - a tag like `json:"-,"` is intended to omit the field, but the `-,` form still allows unmarshaling with `{"-": true}`:

```yaml
id: unmarshal-tag-is-dash
severity: error
message: Struct field can be decoded with the `-` key because the JSON tag
  starts with a `-` but is followed by a comma.
rule:
  pattern: ‘`$TAG`’
  inside:
    kind: field_declaration
constraints:
  TAG:
    regex: json:"-,.+
```

  - treat these matches as actionable security review items rather than optional style cleanup
  - the fix is using just `json:"-"` without the trailing comma

- When adapting Go catalog rules in VT Code, preserve repository-specific Go version support, test conventions, package structure, security policy, and existing linter coverage instead of importing the example verbatim.

### Cpp Catalog Examples

- Format-string vulnerability rewrite:
  - good security-focused rewrite example for `fprintf` or `sprintf` calls that pass attacker-controlled or variable text as the format argument
  - uses `constraints` with regex on `$PRINTF` to match only `sprintf`/`fprintf`, and `not` kind on `$VAR` to exclude string literals and concatenated strings
  - the fix inserts `"%s"` as the format string argument
  - keep it reviewable because some repositories should migrate to safer APIs (e.g. `snprintf`) or broader hardening patterns instead of applying only a local `"%s"` rewrite
  - example YAML:

```yaml
id: fix-format-string
language: cpp
rule:
  pattern: $PRINTF($S, $VAR)
  constraints:
    PRINTF:
      regex: ^sprintf|fprintf$
    VAR:
      not: { kind: string_literal }
      not: { kind: concatenated_string }
fix: $PRINTF($S, "%s", $VAR)
```

- Struct inheritance matching:
  - good example of why C++ AST-based patterns often need the full syntactic shape instead of a shortened surface snippet
  - a bare `struct $SOMETHING: $INHERITS` produces an `ERROR` node because tree-sitter-cpp requires the body block
  - use the full `struct ... : ... { ... }` pattern to get a valid `struct_specifier` node
  - example YAML:

```yaml
id: find-struct-inheritance
language: cpp
rule:
  pattern: struct $NAME : $BASE { $$$BODY; }
```

- C++ has no local tree-sitter parser in VT Code, so preflight pattern validation is skipped; patterns go directly to `sg` for parsing
  - use `debug_query` to inspect parse output when matching is surprising
- Reusing Cpp rules with C:
  - only do this when the repository intentionally parses `.c` as Cpp via `languageGlobs`
  - preserve repository-specific parser expectations in mixed-language trees because C and C++ syntax compatibility is not free
- When adapting Cpp catalog rules in VT Code, preserve repository-specific parser choice, security policy, libc conventions, and style/tooling expectations instead of importing the example verbatim.

### C Catalog Examples

- Match function-call patterns:
  - good example of using `context` plus `selector: call_expression` to work around tree-sitter-c fragment ambiguity
  - tree-sitter-c parses `test($A)` as `macro_type_specifier`, but `test($A);` as `expression_statement -> call_expression`
  - prefer this form whenever a plain `foo($A)` pattern parses incorrectly as a macro-related node instead of a call
  - contextual patterns are pattern objects (`context` + `selector` inside `pattern`), which require the CLI skill path via `exec_command`; the public structural surface's `selector` field works with simple string patterns but does not support the `context` field
  - example YAML:

```yaml
id: match-function-call
language: c
rule:
  pattern:
    context: $M($$$);
    selector: call_expression
```

- Method-style to function-call rewrites:
  - useful migration example for C codebases that simulate methods on structs
  - uses `transform` with `replace` to derive a conditional comma from `$$$ARGS`
  - keep it on the CLI skill path because rewriting `$R.$METHOD(...)` to `$METHOD(&$R, ...)` changes the call shape and should be reviewed with local pointer and API conventions in mind
  - example YAML:

```yaml
id: method_receiver
language: c
rule:
  pattern: $R.$METHOD($$$ARGS)
transform:
  MAYBE_COMMA:
    replace:
      source: $$$ARGS
      replace: '^.+'
      by: ', '
fix:
  $METHOD(&$R$MAYBE_COMMA$$$ARGS)
```

- Yoda-condition rewrite:
  - clearly style-oriented example for `if` comparisons with numeric literals
  - uses `constraints` to restrict `$B` to `number_literal` and `inside` to scope within `if_statement`
  - only use it when the repository explicitly prefers that comparison style, not as a default cleanup
  - example YAML:

```yaml
id: may-the-force-be-with-you
language: c
rule:
  pattern: $A == $B
  inside:
    kind: parenthesized_expression
    inside: {kind: if_statement}
constraints:
  B: { kind: number_literal }
fix: $B == $A
```

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

## ESQuery-Style Kind Selectors

ast-grep supports ESQuery-style selectors in the `kind` field. This syntax works in YAML rule `kind` fields, the CLI `--kind` / `-k` flag, and VT Code's public structural `kind` parameter. The selector is written in the `kind` field and ast-grep parses it internally.

### Relationship Selectors

- **Child selector (`>`)**: matches a direct child node. `kind: call_expression > identifier` is equivalent to `kind: identifier` with `inside: { kind: call_expression }`.
- **Descendant selector (space)**: matches a descendant node. `kind: call_expression identifier` is equivalent to `kind: identifier` with `inside: { kind: call_expression, stopBy: end }`.
- **Adjacent sibling selector (`+`)**: matches the next sibling node. `kind: decorator + method_definition` is equivalent to `kind: method_definition` with `follows: { kind: decorator }`.
- **Following sibling selector (`~`)**: matches any following sibling node. `kind: decorator ~ method_definition` is equivalent to `kind: method_definition` with `follows: { kind: decorator, stopBy: end }`.

### Comma Selector

- Comma-separated selectors are converted to `any`. `kind: identifier, number` is equivalent to `any: [{ kind: identifier }, { kind: number }]`.

### Pseudo-classes

- **`:has(selector)`**: matches a node if it has a descendant matching the inner selector. `kind: function_declaration:has(return_statement)` matches function declarations that contain a return statement. Use `>` inside `:has` to match a direct child: `kind: expression_statement:has(> call_expression)`.
- **`:not(selector)`**: negates the inner selector. `kind: identifier:not(number)` matches identifiers that are not numbers.
- **`:is(selector, ...)`**: accepts comma-separated selectors and is converted to `any`. `kind: :is(identifier, number)` matches either identifiers or numbers. Can be combined with relationship selectors: `kind: call_expression > :is(identifier, number)`.
- **`:nth-child(An+B)`**: maps to ast-grep's `nthChild` rule. `kind: array > number:nth-child(2n+1)` matches odd-numbered number elements in arrays.
- **`:nth-child(An+B of selector)`**: supports `of` syntax for filtering. `kind: array > :nth-child(1 of number)` matches the first number element in an array.
- **`:nth-last-child(position)`**: equivalent to `nthChild` with `reverse: true`. `kind: array > number:nth-last-child(1)` matches the last number element in an array.

### Compound Selectors

Compound selectors are combined with `all`. `kind: function_declaration:has(return_statement):not(generator_function)` is equivalent to `all: [{ kind: function_declaration }, { has: { kind: return_statement, stopBy: end } }, { not: { kind: generator_function } }]`.

### Examples

```yaml
# Match identifiers that are direct children of call expressions
kind: call_expression > identifier

# Match any identifier or number node
kind: identifier, number

# Match function declarations containing return statements
kind: function_declaration:has(return_statement)

# Match identifiers that are not numbers
kind: identifier:not(number)

# Match either identifiers or numbers
kind: :is(identifier, number)

# Match the first number element in an array
kind: array > :nth-child(1 of number)

# Match odd-indexed elements
kind: array > number:nth-child(2n+1)

# Combine with pattern: match fn declarations that have return statements
pattern: "fn $NAME() {}"
kind: function_item:has(return_statement)

# C++: match class definitions that have virtual methods
kind: class_specifier:has(virtual_function_specifier)

# C++: match template function declarations
kind: template_declaration > function_definition

# C++: match delete expressions (potential memory management issues)
kind: delete_expression

# Python: match function definitions with decorators
kind: decorated_definition > function_definition
```

### Current Limitations

- Class selectors like `.body` are tokenized but rejected as unsupported.
- Supported pseudo-classes are only `:has`, `:not`, `:is`, `:nth-child`, and `:nth-last-child`.
- `:has(...)`, `:not(...)`, and `of ...` parse a single complex selector, not a comma selector list.
- `:is(...)` is the one pseudo-class that accepts comma-separated selector lists.
- Identifiers can include letters, digits, `_` and `-`, but cannot start with a digit.

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
- Transforms are evaluated in declaration order. A transform that references a variable created by an earlier transform in the same `transform` block will see the already-transformed value.
- Transforms only run after the rule matches. If the rule does not match, no transforms execute and no `fix` is applied.

### replace

- `replace` is regex-based text replacement over one captured meta variable.
  - `source` must be `$VAR` style
  - `replace` is the Rust regex
  - `by` is the replacement string
  - capture groups from the regex can be reused in `by` as `$1`, `$2`, etc.
  - those capture groups are only available inside that same `replace` transform; regular `regex` rules do not expose them
  - Rust regex syntax: no look-around, no PCRE backreferences, but capture groups work

```yaml
# Strip leading underscore from identifier
transform:
  CLEAN:
    replace:
      replace: "^_"
      by: ""
      source: $VAR

# Extract file extension using capture group
transform:
  EXT:
    replace:
      replace: ".*\\.(.+)$"
      by: "$1"
      source: $FILENAME

# String-form (ast-grep 0.38.3+)
transform:
  CLEAN: replace($VAR, replace="^_", by="")
```

### substring

- `substring` is Python-style slicing over Unicode characters.
  - `startChar` is inclusive
  - `endChar` is exclusive
  - negative indexes count backward from the end of the string
  - omit either bound for open-ended slicing

```yaml
# Strip first and last character (e.g., remove quotes)
transform:
  UNQUOTED:
    substring:
      startChar: 1
      endChar: -1
      source: $STR

# Get first 3 characters
transform:
  PREFIX:
    substring:
      endChar: 3
      source: $ID

# String-form (ast-grep 0.38.3+)
transform:
  UNQUOTED: substring($STR, startChar=1, endChar=-1)
```

### convert

- `convert` changes identifier casing through `toCase`.
  - common targets: `camelCase`, `snakeCase`, `kebabCase`, `pascalCase`, `lowerCase`, `upperCase`, `capitalize`
  - `separatedBy` controls how the source string is split into words before conversion
  - when `separatedBy` is omitted, all known separators are used
- `CaseChange` is the separator for transitions like `astGrep`, `ASTGrep`, or `XMLHttpRequest`

```yaml
# Convert camelCase to snake_case
transform:
  SNAKE:
    convert:
      toCase: snakeCase
      source: $CAMEL

# Convert only by underscore, preserving internal casing
transform:
  KEBAB:
    convert:
      toCase: kebabCase
      separatedBy: [underscore]
      source: $NAME

# String-form (ast-grep 0.38.3+)
transform:
  SNAKE: convert($CAMEL, toCase=snakeCase)
```

### Chaining Transforms

- Later transforms can consume variables created by earlier transforms. This is the standard way to build multi-step string pipelines.

```yaml
# Pipeline: strip prefix, then convert case
transform:
  RAW:
    replace:
      replace: "^get"
      by: ""
      source: $METHOD
  SNAKE:
    convert:
      toCase: snakeCase
      source: $RAW
# Input: "getUserName" -> RAW="UserName" -> SNAKE="user_name"
```

### Conditional Separators from Multi-Capture

- For conditional commas, whitespace, or similar glue text, derive a helper transform such as `MAYBE_COMMA` from a possibly empty multi-capture instead of hard-coding punctuation directly into `fix`.

```yaml
# Add comma only when there are existing arguments
rule:
  pattern: "foo($$$ARGS)"
transform:
  MAYBE_COMMA:
    replace:
      replace: ".+"
      by: ", "
      source: $ARGS
fix: "bar($MAYBE_COMMA$newArg)"
```

### String-Form Transforms

- Ast-grep also accepts string-form transforms such as `replace(...)`, `substring(...)`, `convert(...)`, and `rewrite(...)`.
- String-form transforms require ast-grep 0.38.3+. Use object form when version compatibility or debugging clarity matters.
- String-form syntax: `operator($SOURCE, key1=value1, key2=value2)`.

## Rewriters

- `rewriters` is an experimental feature and should be treated as advanced YAML, not the default way to structure rewrites.
- Rewriters allow replacing multiple sub-nodes with different fixes in one rule. The normal `fix` replaces one matched node at a time; `rewriters` plus `transform.rewrite` handle the one-to-many case.
- A rewriter only accepts:
  - `id`
  - `rule`
  - `constraints`
  - `transform`
  - `utils`
  - `fix`
- `id`, `rule`, and `fix` are required.
- The three-step workflow is:
  1. Define `rewriters` at the YAML rule root. Each rewriter has an `id`, a `rule` to match sub-nodes, and a `fix` to transform each matched sub-node.
  2. Apply the rewriter to a metavariable via `transform` using the `rewrite` operator. `rewriters` lists which rewriter ids to try; `source` points at the metavariable whose sub-nodes are rewritten.
  3. Use the resulting transformed metavariable in the outer `fix`.
- Concrete example converting Python `dict(a=1, b=2)` to `{‘a’: 1, ‘b’: 2}`:
  - Define a rewriter that matches `keyword_argument` nodes: extract `$KEY` and `$VAL` from the name and value fields, fix as `’$KEY’: $VAL`.
  - Apply it to `$$$ARGS` captured from `dict($$$ARGS)` via `transform: { LITERAL: { rewrite: { rewriters: [dict-rewrite], source: $$$ARGS } } }`.
  - Use `fix: ‘{ $LITERAL }’` on the outer rule to wrap the rewritten arguments in braces.
- Multiple rewriters can be listed in one `transform.rewrite` call. Each sub-node is transformed by the first matching rewriter in declaration order. If two rewriters could match the same node, only the one that appears earlier in the `rewriters` list is applied. Order matters.
- `joinBy` controls how transformed sub-nodes are stitched together. By default, sub-nodes are replaced in-place preserving original separators. Set `joinBy` to a string like `’ + ‘` or `’\n’` to override the joiner. For example, `joinBy: "\n"` converts comma-separated imports into newline-separated direct imports.
- Rewriters are only reachable through `transform.rewrite`.
- Captured meta variables do not cross rewriter boundaries.
  - one rewriter cannot read another rewriter’s captures
  - the outer rule cannot read captures local to a rewriter
- Rewriter-local `transform` variables and `utils` stay local the same way.
- A rewriter can still call other rewriters from the same list inside its own `transform` section, enabling multi-pass rewrite pipelines.
- For simple pattern-to-pattern rewrites, use `workflow="rewrite"` on the public structural surface to preview replacements without applying them. This runs `ast-grep run --pattern=... --rewrite=... --json=compact --color=never` and returns each match with its proposed `replacement` and `replacementOffsets`. The surface remains read-only; no files are modified.
- For advanced rewrite operations using `rewriters`, `transform.rewrite`, `joinBy`, or `FixConfig` with `expandStart`/`expandEnd`, use the CLI skill path via `exec_command`.

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
  - SQL tagged template example: `sql\`$CONTENT\``
- The `$CONTENT` meta variable is required in the injection rule. It designates which part of the host match should be re-parsed as the injected language.
- Use dynamic `injected` candidates when the rule captures `$LANG` and the embedded language varies (e.g., `css` vs `scss` vs `less`).
- Use `languageGlobs` when a whole file should be parsed through a different or superset language.
- Use `languageInjections` when only a nested fragment switches language inside the same file.
- Use `workflow='inspect'` on VT Code's public structural surface to see configured injections, custom languages, and language globs from the project's `sgconfig.yml`.
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
- This is the declarative way to do one-to-many rewrites such as splitting a barrel import into separate import lines, converting `dict(a=1, b=2)` to `{'a': 1, 'b': 2}`, or transforming heterogeneous lists where each element type needs a different rewrite rule.
- `transform.rewrite` has three important behavioral properties: (1) it rewrites descendants of the captured source metavariable, not the source itself; (2) overlapping rewriter matches are prevented so each sub-node is rewritten at most once; (3) higher-level AST matches are preferred before nested ones, and for one node only the first matching rewriter in declaration order is applied.
- Use `joinBy` when the rewritten sub-nodes must be stitched with a different separator than the original source text. For example, `joinBy: "\n"` converts comma-separated imports into newline-separated direct imports.
- For simple pattern-to-pattern rewrites, use `workflow="rewrite"` on the public structural surface to preview replacements without applying them. Each result includes the original `text`, proposed `replacement`, `replacementOffsets`, and `metaVariables`.
- For advanced `transform.rewrite`, `rewriters`, `joinBy`, and `FixConfig` operations, use the CLI skill path via `exec_command`.

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

Switch to direct CLI work through `exec_command` when the task needs:

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
