---
name: ast-grep
description: "Use for ast-grep: ast-grep run, sg scan, sg test, sg new, new rule, sgconfig.yml, inline-rules, stdin, json, optional chaining, rule catalog, meta variables, pattern objects, nthChild stopBy, range field, metadata url, caseInsensitive glob, severity off, include metadata, rule order, kind pattern, positive rule, kind esquery, debug-query, static analysis, tree-sitter parser, pattern yaml api, search rewrite lint analyze, textual structural, ast cst, named unnamed, kind field, ambiguous pattern, effective selector, meta variable detection, lazy multi, strictness smart, relaxed signature, string fix, fix config, expandEnd, replace substring, toCase separatedBy, rewriter, rewrite joinBy, find patch, barrel import, ruleDirs testConfigs, libraryPath languageSymbol, dynamic injected, custom language, TREE_SITTER_LIBDIR, language injection, styled components, language alias, languageGlobs, expandoChar, napi parse, python api, programmatic API, walrus operator, list comprehension, isinstance tuple."
metadata:
    short-description: Ast-grep project workflows
---

# Ast-Grep

Use this skill for ast-grep project setup, rule authoring, rule debugging, and CLI workflows that go beyond a single structural query.

## Routing

- Prefer `unified_search` with `action="structural"` and `workflow="scan"` for read-only project scans.
- Prefer `unified_search` with `action="structural"` and `workflow="test"` for read-only ast-grep rule tests.
- Prefer `unified_search` with `action="structural"` and `workflow="rewrite"` for dry-run rewrite previews. This runs `ast-grep run --pattern=... --rewrite=... --json=compact --color=never` and returns proposed replacements without applying them. Required fields: `pattern`, `rewrite`. Optional: `lang`, `selector`, `strictness`, `globs`, `context_lines`, `max_results`.
- Prefer structural `debug_query` on the public tool surface before falling back to raw `ast-grep run --debug-query`.
- Use `kind` on the public structural surface to match nodes by tree-sitter node kind (e.g. `function_item`, `call_expression`). `kind` supports ESQuery-style compound selectors like `A > B`, `A + B`, `A ~ B`, `A, B`, and pseudo-selectors like `:has()`, `:not()`, `:is()`, `:nth-child()`. `kind` can be used alone or combined with `pattern`.
- Stay on the public structural surface first when the task is only running project checks, reporting findings, or previewing rewrites.
- Use `unified_exec` only when the public structural surface cannot express the requested ast-grep flow.

## Quick Start

- In VT Code, prefer `vtcode dependencies install ast-grep` before suggesting system package managers.
- External install routes such as Homebrew, Cargo, npm, pip, MacPorts, or Nix are fallback options when the user explicitly wants a system-managed install.
- After installation, validate availability with `ast-grep --help`.
- On Linux, prefer the full `ast-grep` binary name over `sg` because `sg` may already refer to `setgroups`.
- When running CLI patterns with shell metavariables like `$PROP`, use single quotes so the shell does not expand them before ast-grep sees the pattern.
- A good first rewrite example is optional chaining, for example rewriting `$PROP && $PROP()` to `$PROP?.()`.

## Command Overview

- `ast-grep run`: ad-hoc query execution and one-off rewrites.
- `ast-grep scan`: project rule scanning.
- `ast-grep new`: scaffold and rule generation.
- `ast-grep test`: rule-test execution.
- `ast-grep lsp`: editor integration via language server.

## Built-In Languages

- ast-grep ships many built-in languages. Common aliases include `bash`, `c`, `cc` / `cpp`, `cs`, `css`, `ex`, `go` / `golang`, `html`, `java`, `js` / `javascript` / `jsx`, `json`, `kt`, `lua`, `md` / `markdown`, `php`, `py` / `python`, `rb`, `rs` / `rust`, `swift`, `ts` / `typescript`, `tsx`, and `yml`.
- `--lang <alias>` and YAML `language: <alias>` use those built-in aliases. File-system scans infer language from built-in extensions unless the project overrides them.
- In VT Code, public structural `lang` is passed through to ast-grep. VT Code also normalizes and infers a local subset it can pre-parse itself: Rust, Python, JavaScript, TypeScript, TSX, Go, Java, and Markdown.
- That local subset includes common ast-grep aliases and extensions such as `golang`, `jsx`, `cjs`, `mjs`, `cts`, `mts`, `py3`, `pyi`, and `mdx`.
- Use `languageGlobs` when the repository needs a different extension mapping than ast-grep’s built-in defaults.

## How Ast-Grep Works

- ast-grep accepts several query formats: pattern queries, YAML rules, and programmatic API usage.
- The core pipeline is parse first, match second. Tree-Sitter builds the syntax tree, then ast-grep’s Rust matcher finds the target nodes.
- The main usage scenarios are search, rewrite, lint, and analyze.
- ast-grep processes many files in parallel and is built to use multiple CPU cores on larger codebases.
- In VT Code, the public structural surface is the read-only entry point for query, scan, and test. Use the bundled skill when the task is about YAML authoring, rewrite/apply flows, or API-level ast-grep work.

## Project Scaffolding

- A scan-ready ast-grep project needs workspace `sgconfig.yml` plus at least one rule directory, usually `rules/`.
- `rule-tests/` and `utils/` are optional scaffolding that `ast-grep new` can create for rule tests and reusable utility rules.
- If the repository already has `sgconfig.yml` and `rules/`, prefer working with the existing layout instead of recreating scaffolding.
- Use `ast-grep new` when the repository does not have ast-grep scaffolding yet.
- Use `ast-grep new rule` when the scaffold exists and the task is creating a new rule plus optional test case.

## sgconfig.yml

- `sgconfig.yml` is the project-level ast-grep config file, not a rule file. Treat it like the repository root for rule discovery, tests, parser overrides, and embedded-language behavior.
- `ruleDirs` is required and is resolved relative to the directory containing `sgconfig.yml`.
- `testConfigs` is optional and configures ast-grep test discovery. Each entry needs `testDir`; `snapshotDir` is optional and otherwise defaults to `__snapshots__` under that `testDir`.
- `utilDirs` declares directories for global utility rules shared across multiple rule files.
- `languageGlobs` remaps files to parsers and takes precedence over ast-grep’s default extension mapping, which is useful for similar-language reuse like TS -> TSX or C -> Cpp.
- `customLanguages` registers project-local parsers. `libraryPath` can be one relative library path or a target-triple map, `extensions` is required, `expandoChar` is optional, and `languageSymbol` defaults to `tree_sitter_{name}`.
- `languageInjections` is experimental. Each entry needs `hostLanguage`, `rule`, and `injected`.
- Use dynamic `injected` candidates when the rule captures `$LANG` and the embedded language must be chosen from a list such as `css`, `scss`, or `less`.
- Raw ast-grep project discovery walks upward from the current working directory until it finds `sgconfig.yml`, and `--config <file>` overrides that discovery with an explicit root config path.
- `ast-grep scan` requires project config and will error if no `sgconfig.yml` is found. `ast-grep run` can still search without project config, though it also benefits from discovered config for things like `customLanguages` and `languageGlobs`.
- `ast-grep scan --inspect summary` is the quickest way to confirm which project directory and config file ast-grep actually selected during discovery.
- ast-grep also recognizes a home-directory `sgconfig.yml` as a global fallback config. XDG config directories are not part of this behavior.
- Keep `sgconfig.yml` authoring on the skill path. VT Code’s public structural tool can consume an existing config through `config_path`, but it does not expose these top-level schema fields directly.

## Rule Catalog

- Use the ast-grep catalog as inspiration when the user wants existing example rules, not as something to copy blindly.
- Start from examples in the same language family when possible.
- Read catalog markers as hints about rule complexity:
  - simple pattern examples are good starting points
  - `Fix` means the example includes a rewrite path
  - `constraints`, `labels`, `utils`, `transform`, and `rewriters` mean the example depends on more advanced rule features
- When adapting a catalog example, translate it to the current repository’s language, style, and safety constraints instead of preserving the example verbatim.
- Prefer the bundled skill workflow when the user asks to explain, adapt, or combine catalog examples.

## VT Code Bundled Rules

VT Code ships a set of curated ast-grep rules under `rules/` with matching tests under `rule-tests/`. Run them with `vtcode check ast-grep`. The bundled rules are organized by language:

### Python (`rules/python/`)
- `no-print`: flags `print()` calls in production code
- `no-walrus-source`: flags walrus operators that harm readability
- `no-unnecessary-list`: flags `list(...)` wrapping an already-list expression
- `no-identity-check-with-type`: flags `type(x) is T` in favor of `isinstance(x, T)`
- `optional-to-union`: flags `Optional[X]` in favor of `X | None`
- `prefer-dict-get`: flags `if k in d: d[k]` in favor of `d.get(k)`
- `prefer-generator-expression`: flags list comprehensions passed to `sum`/`any`/`all`/`min`/`max`
- `prefer-isinstance-tuple`: flags `isinstance(x, A) or isinstance(x, B)` in favor of `isinstance(x, (A, B))`

### Rust (`rules/rust/`)
- `no-unsafe-fn-without-unsafe`: flags `unsafe fn` bodies that contain no `unsafe` block
- `avoid-duplicate-export`: flags `pub use` when `pub mod` already exposes the module
- `no-iterator-for-each`: flags `.iter().for_each()` in favor of `for` loops
- `no-redundant-closure`: flags `|x| foo(x)` in favor of `foo` directly
- `let-chain-candidate`: flags nested `if` that could be collapsed with `let`-chains
- `no-chars-enumerate`: flags `.chars().enumerate()` when `.char_indices()` is more idiomatic
- `no-alloc-digit-count`: flags digit-count loops that allocate instead of using repeated division
- `prefer-iterator-sum`: flags manual accumulator loops in favor of `.sum()`
- `prefer-retain-over-filter-collect`: flags `.filter().collect()` on a `Vec` in favor of `.retain()`
- `prefer-unwrap-or-default`: flags `.unwrap_or(Default::default())` in favor of `.unwrap_or_default()`

### Kotlin (`rules/kotlin/`)
- `no-var`: flags mutable `var` declarations
- `no-println`: flags `println`/`print` calls
- `no-lateinit`: flags `lateinit var` usage
- `no-unsafe-cast`: flags `as` casts without null-safe `as?`
- `no-unnecessary-let`: flags `let` blocks that add no value
- `prefer-is-empty`: flags `.count() == 0` in favor of `.isEmpty()`
- `prefer-data-class`: flags classes that should be `data class`
- `clean-architecture-imports`: flags imports that violate clean architecture layer boundaries

### Ruby (`rules/ruby/`)
- `no-path-traversal`: flags string concatenation in `File.join` / `Pathname` that may cause traversal
- `prefer-action-over-filter`: flags `before_filter` / `after_filter` in favor of `before_action` / `after_action`
- `prefer-symbol-over-proc`: flags `Proc.new` with a symbol when `(&:method)` is cleaner

### TypeScript (`rules/typescript/`)
- `no-await-in-promise-all`: flags `await` inside `Promise.all()` arrays (defeats parallelism)
- `no-console-except-error`: flags `console.log/debug/warn/info/trace` (allows `console.error` in catch blocks)
- `no-debugger`: flags `debugger` statements
- `no-unnecessary-boolean-literal-compare`: flags `x === true` or `x === false`
- `no-useless-promise-resolve`: flags `return Promise.resolve(...)` in async functions
- `prefer-array-flat-map`: flags `.map(fn).flat()` in favor of `.flatMap(fn)`
- `prefer-nullish-coalescing`: flags `||` in assignments/returns where `??` is more precise
- `use-logical-assignment`: flags `$A = $A || $B` in favor of `$A ||= $B`
- `prefer-optional-chaining`: flags `a && a.b` in favor of `a?.b`
- `no-return-in-forEach`: flags `return` inside `.forEach()` callbacks (does not return from caller)
- `no-array-delete`: flags `delete arr[i]` in favor of `.splice()`

### TSX (`rules/tsx/`)
- `avoid-jsx-short-circuit`: flags `{cond && <Elem />}` in favor of `{cond ? <Elem /> : null}` (prevents rendering `0`)
- `no-nested-links`: flags `<a>` elements nested inside other `<a>` elements (invalid HTML)
- `no-unnecessary-usestate-type`: flags `useState<string>('hello')` when TypeScript can infer the type
- `rename-svg-attribute`: flags hyphenated SVG attributes like `stroke-linecap` in favor of camelCase `strokeLinecap`

### Examples (`rules/examples/`)
- `no-console-log`: starter rule scoped to `__ast_grep_examples__/` for scaffold validation

## Rust Catalog Highlights

- Avoid duplicated exports: a Rust lint-style rule can detect `pub use foo::Bar;` in the same source file that already exposes `pub mod foo;`. Treat this as API-surface cleanup, not a mechanical rewrite. The rule uses `all` to combine a `pub use $A::$B;` pattern with `inside: { kind: source_file }` and a `has` check for `pub mod $A;` with `stopBy: end`:

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

- Beware `chars().enumerate()`: the Rust catalog rewrite from `$A.chars().enumerate()` to `$A.char_indices()` is valid when the code needs byte offsets instead of character indexes. Do not apply blindly if the caller intentionally wants character positions:

```yaml
id: no-chars-enumerate
language: Rust
severity: warning
message: Use `.char_indices()` instead of `.chars().enumerate()` when byte offsets are needed.
rule:
  pattern: "$A.chars().enumerate()"
fix: "$A.char_indices()"
```

- Count `usize` digits without allocation: the catalog rewrite from `$NUM.to_string().chars().count()` to `$NUM.checked_ilog10().unwrap_or(0) + 1` is a good Rust-specific performance cleanup when the target is known to be an integer digit count. Do not over-apply if the expression is part of a more general formatting pipeline:

```yaml
id: no-alloc-digit-count
language: Rust
severity: info
message: Count integer digits without heap allocation.
rule:
  pattern: "$NUM.to_string().chars().count()"
fix: "$NUM.checked_ilog10().unwrap_or(0) + 1"
```

- Unsafe function without unsafe block: the Rust catalog’s `function_item` rule that requires `unsafe` modifiers but rejects bodies containing `unsafe_block` is a good review rule for redundant `unsafe` markers. It is diagnostic-oriented and should usually stay a scan rule, not an automatic rewrite. The rule uses `kind: function_item` with `has` checking `function_modifiers` via `regex: "^unsafe"` and `not` rejecting bodies containing `unsafe_block`:

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

- Rust 2024 let-chain candidate: the catalog’s nested `if`/`if let` detection rule uses `utils` to define reusable matchers for sole-child statements, no-else `if` expressions, and no-else `if let` expressions. The root rule matches an `if` whose block contains only another `if` statement, suggesting the two can be collapsed into a single let-chain. Keep this as a `hint`-severity suggestion because let-chains require Rust 2024 edition:

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

- Rewrite `indoc!` macro: the catalog example that removes `indoc! { r#"..."# }` wrappers is a rewrite-oriented example. Keep it on the CLI skill path because the replacement is formatting-sensitive and should be reviewed interactively before broad apply. The CLI pattern is `ast-grep --pattern ‘indoc! { r#"$$$A"# }’ --rewrite ‘`$$$A`’`.
- Adapt these rules to the repository’s Rust style before using them directly. In VT Code, preserve existing lint policy, public API conventions, and the project’s bias against unnecessary rewrites.

## TypeScript Catalog Highlights

- TypeScript vs TSX matters: keep `.ts` and `.tsx` rules separate unless the repository intentionally parses `.ts` as TSX through `languageGlobs`. Do not assume one pattern works unchanged across both parsers.
- Find import file without extension: good scan rule for ESM codebases that require explicit local file extensions on static or dynamic imports. It is policy-dependent, so only use it where the runtime or bundler actually requires explicit extensions.
- XState v4 to v5 migration: strong example of multi-rule YAML with `utils`, `transform`, and multi-document configs. Keep this sort of migration on the CLI skill path and review the generated diff instead of treating it as a one-line rewrite.
- No `await` inside `Promise.all([...])`: good rewrite rule when the awaited expression is directly inside the array literal. Keep the rewrite narrow so it does not change intentionally sequential logic hidden behind helper calls.
- No console except allowed cases: good scan rule for client-facing TypeScript, but it is repository-policy dependent. Adapt the allowed methods and environments before enabling it broadly.
- Find import usage or identifiers: these examples are useful for repository analysis and dependency cleanup, not just linting. They are often better treated as search/report rules than rewrite rules.
- Switch Chai `should` to `expect`: a useful migration example, but it is test-framework-specific and should be applied only where Chai is actually in use.
- Speed up barrel imports: strong `rewriters` / `transform.rewrite` example for splitting one import into many direct imports. Keep it on the CLI skill path because path conventions, default-vs-named exports, and formatting policy vary by repository.
- Missing Angular `@Component()` decorator: good example of labels plus pattern-object `context` and `selector`. Keep framework-specific rules tied to actual framework usage in the repository.
- Logical assignment operators: a compact rewrite example for `$A = $A || $B` to `$A ||= $B`, but only apply it where the project’s JS target and lint policy allow ES2021 operators.
- Adapt TypeScript catalog rules to the repository’s module system, framework stack, transpilation target, and lint policy before using them directly.

## TSX Catalog Highlights

- TSX vs TypeScript matters for parsing: JSX-bearing patterns should stay on the TSX parser unless the repository intentionally routes `.ts` through TSX with `languageGlobs`.
- Unnecessary `useState<T>` primitives: good cleanup rewrite for `useState<string|number|boolean>($A)` when the initializer already gives TypeScript enough information to infer the state type. **Bundled** as `rules/tsx/no-unnecessary-usestate-type.yml`.
- Avoid `&&` short-circuit in JSX: good React-facing rewrite from `{cond && <View />}` to `{cond ? <View /> : null}` when the left side can evaluate to renderable falsy values like `0`. **Bundled** as `rules/tsx/avoid-jsx-short-circuit.yml`.
- Rewrite MobX component style: useful migration example when `observer(() => ...)` hides React hook linting from tooling. Keep it on the CLI skill path because naming, export shape, and component conventions vary by repository.
- Avoid unnecessary React hooks: good diagnostic rule for `use*` functions that do not actually call hooks. Treat it as a review rule first, because renaming or de-hooking can be API-affecting.
- Reverse React Compiler: clearly rewrite-oriented and intentionally opinionated. Keep it on the CLI skill path and only use it when the user explicitly wants that de-memoization behavior.
- Avoid nested links: good accessibility and correctness scan rule for JSX trees. **Bundled** as `rules/tsx/no-nested-links.yml`.
- Rename SVG attributes: strong TSX rewrite example for hyphenated SVG attribute names such as `stroke-linecap` to `strokeLinecap`. Keep it reviewable because generated markup can be formatting-sensitive. **Bundled** as `rules/tsx/rename-svg-attribute.yml`.
- Adapt TSX catalog rules to the repository’s React version, JSX runtime, lint rules, framework conventions, and browser-support target before using them directly.

## YAML Catalog Highlights

- YAML scan rules are useful for configuration-policy checks where the repository needs to flag specific keys or values rather than rewrite source code.
- The catalog host/port example is a simple message-oriented rule that matches either `host: $HOST` or `port: $PORT` and attaches a diagnostic. Treat it as a starting point for config validation, not a complete policy by itself.
- For YAML rules, be explicit about whether the repository cares about the key name, the value, or both. If both matter together, move from separate `any` patterns to a more structured rule before relying on the result.
- Keep YAML config checks repository-specific. Hard-coded values like `8000` are only useful when they reflect an actual project policy.

## Ruby Catalog Highlights

- Key Ruby tree-sitter node kinds for pattern authoring: `call` for method calls (e.g. `$OBJ.method`), `method_call` for keyword-style calls (e.g. `puts "hello"`), `block` for `{{ }}` blocks, `do_block` for `do...end` blocks, `symbol` for `:name` literals, `assignment` for variable assignments, `method` for method definitions, `class` for class definitions, `if` for conditionals, `unless` for negative conditionals, `case` for case/when, `while` and `until` for loops, `return` for return statements, `yield` for yield calls, `super` for super calls, `self` for self references.
- Ruby has no local tree-sitter parser in VT Code, so preflight pattern validation is skipped; patterns go directly to `sg` for parsing. Use `debug_query` to inspect parse output when matching is surprising.
- Ruby’s `$VAR` meta-variable syntax works directly because `$` is a valid Ruby global variable prefix. No `expandoChar` override is needed.
- Rails `*_filter` to `*_action`: useful migration rewrite for older Rails controllers. The catalog rule uses a `transform` with `replace` to swap `_filter` for `_action` on the captured `$FILTER` meta-variable. The pattern uses `$$$ACTION` to capture all arguments after the filter name. Keep it on the CLI skill path because framework version, controller style, and review expectations vary by repository:

```yaml
id: migration-action-filter
language: Ruby
rule:
  any:
    - pattern: before_filter $$$ACTION
    - pattern: after_filter $$$ACTION
    - pattern: around_filter $$$ACTION
  has:
    pattern: $FILTER
    kind: identifier
fix:
  template: $FILTER_ACTION $$$ACTION
  transform:
    FILTER_ACTION:
      source: $FILTER
      replace:
        regex: _filter$
        by: _action
```

- Prefer symbol over proc: good Ruby cleanup rewrite for cases like `.select { |v| v.even? }` to `.select(&:even?)`. The catalog rule constrains `ITER` to `map|select|each` via `regex`, and matches the block pattern `$LIST.$ITER { |$V| $V.$METHOD }`. The fix uses `$LIST.$ITER(&:$METHOD)` syntax. Only apply where the shorthand remains readable and matches local Ruby style. Extend the `ITER` regex to cover `reject`, `find_all`, `detect`, `any?`, `all?`, `none?`, `count` when appropriate.
- Path traversal detection in Rails: good security-oriented scan rule for `Rails.root.join`, `File.join`, or `send_file` fed by variables. Uses `any` with three patterns and `severity: hint` because this is a detection rule, not proof of exploitability. The surrounding validation path still matters. Advise `File.basename()` or allowlist validation as remediation.
- For bare block fragments like `{ |$V| $V.$METHOD }` or `do |$V| $V.$METHOD end`, wrap in the enclosing method call and use `selector: call` to match the outer call. For symbol-to-proc, match the enclosing method call directly with `$LIST.$ITER(&:$METHOD)`.
- Adapt Ruby catalog rules to the repository’s Rails version, Ruby style guide, and security posture before using them directly.

## Python Catalog Highlights

- Key Python tree-sitter node kinds for pattern authoring: `function_definition` for functions, `call` for function calls, `import_statement` and `import_from_statement` for imports, `assignment` for assignments, `decorated_definition` for decorated functions/classes, `with_statement` for context managers, `try_statement` for try/except, `if_statement` for conditionals, `for_statement` for loops, `return_statement` for returns, `async_function_definition` for async functions, `await` for await expressions, `type` for type annotations, `subscript` for generic types like `Optional[T]`, `list_comprehension` for list comprehensions, `argument_list` for function arguments, `keyword_argument` for keyword arguments, `conditional_expression` for ternary expressions, `assert_statement` for assertions.
- Python has a local tree-sitter parser in VT Code, so preflight pattern validation works for Python patterns. Meta-variable patterns are sanitized and parsed locally before being sent to `sg`.
- Python’s `$VAR` meta-variable syntax works directly because `$` is not a valid Python identifier prefix in expression context. No `expandoChar` override is needed.
- OpenAI SDK migration: useful multi-rule migration example for legacy `openai` Python client code, but keep it on the CLI skill path because imports, client lifetime, response shapes, and surrounding application logic often need repository-specific review. The migration uses three rules separated by `---`: import rewrite (`import openai` to `from openai import Client`), client initialization (`openai.api_key = $KEY` to `client = Client($KEY)`), and completion method (`openai.Completion.create($$$ARGS)` to `client.completions.create($$$ARGS)`).
- Prefer generator expressions: good example of narrowing a rewrite to contexts like `any(...)`, `all(...)`, or `sum(...)` where generator expressions are clearly valid. Do not generalize it to every list comprehension. The constraint-based variant uses `constraints` to restrict `$FUNC` to `any|all|sum` and `$LIST` to `list_comprehension` kind, then strips brackets with a `substring` transform:

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

- Walrus operator in `if` statements: useful paired-rule rewrite example, but only apply it where the repository targets Python 3.8+ and the style guide accepts assignment expressions. This is a multi-rule YAML using `follows` and `precedes` relational operators. The first rule rewrites the `if` to use `:=`, the second deletes the preceding assignment:

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

- Remove async function: strong `rewriters` example for stripping `async` and inner `await`, but treat it as high-risk migration work because it changes call semantics and often requires broader control-flow review. Uses `rewriters` to strip `await` from inside the body before removing the `async` keyword:

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

- Pytest fixture refactors: good example of `utils`-driven context matching for fixture rename or type-hint updates. Uses `utils` to define reusable context matchers like `is-fixture-function` (function following a `@pytest.fixture` decorator) and `is-test-function` (function whose name starts with `test_`). Keep it tied to real pytest usage so similarly named non-test code is not swept in.
- `Optional[T]` to `T | None` and recursive union rewrites: useful typing-modernization examples, but only where the repository targets Python 3.10+ and static typing policy actually prefers PEP 604 unions. The simple variant uses `context` and `selector` to disambiguate `Optional[$T]` as a generic type:

```yaml
id: optional-to-union
language: python
rule:
  pattern:
    context: ‘a: Optional[$T]’
    selector: generic_type
fix: $T | None
```

The recursive variant handles nested `Union` and `Optional` types using multiple `rewriters` that call each other, transforming deeply nested expressions like `Optional[Union[List[Union[str, dict]], str]]` into `List[str | dict] | str | None`.

- SQLAlchemy `mapped_column` to annotated `Mapped[...]`: useful ORM migration example, but keep it on the CLI skill path because ORM version, model style, and nullable semantics need review. Uses `rewriters` to filter out `String` positional args and `nullable=True` keyword args from the argument list, then wraps the result in `Mapped[str | None]`.
- `print` detection: use `kind: call` with `has: { field: function, pattern: print }` to match `print()` calls. Scope with `files` to exclude test directories and scripts where console output is acceptable. For `logging.debug()` or similar, use `regex: ^(debug|info|warning)$` on the function field inside a `logging.` attribute access.
- f-string preference: use `kind: call` with `has: { field: function, pattern: $FN }` and `constraints` restricting `$FN` to `^(str|int|float|repr)$` to find type-conversion calls that could be f-string expressions. This is a suggestion rule, not an enforcement rule, because some conversions are intentional type coercion.
- List comprehension vs `map`/`filter`: pattern `$LIST = list(map($FUNC, $ITER))` can be rewritten to `$LIST = [$FUNC($X) for $X in $ITER]` when `$FUNC` is a simple lambda or single-argument call. Keep it on the CLI skill path because readability depends on the complexity of `$FUNC`.
- `dict.get` with default: pattern `$D[$KEY]` inside a `try_statement` with `except KeyError` can often be rewritten to `$D.get($KEY)` or `$D.get($KEY, $DEFAULT)`. Use `kind: subscript` with `inside` to scope within the try body. Treat as review material because some dict access patterns intentionally propagate `KeyError`.
- Assert vs unittest assertions: pattern `assert $EXPR == $VAL` can be rewritten to `self.assertEqual($EXPR, $VAL)` in unittest contexts, or left as-is in pytest contexts. Use `files` to scope by test framework convention.
- `isinstance` tuple consolidation: pattern `isinstance($X, $A) or isinstance($X, $B)` can be rewritten to `isinstance($X, ($A, $B))`. This is a safe autofix when both `isinstance` calls check the same variable.
- Adapt Python catalog rules to the repository’s Python version floor, framework stack, typing policy, async model, and migration scope before using them directly.

## Kotlin Catalog Highlights

- Clean-architecture import checks: good scan-rule example for enforcing architectural boundaries with `files` plus import-path constraints. Treat it as repository-policy enforcement rather than a universal Kotlin rule.
- The Kotlin catalog example is diagnostic-oriented, not rewrite-oriented. Keep it on the scan path because import-boundary violations usually need design review instead of blind mutation.
- File-scoped package constraints are the point of the example: adapt the `files` glob and package regexes to the repository’s actual module layout before relying on the result.
- Kotlin has no local tree-sitter parser in VT Code, so preflight pattern validation is skipped; patterns go directly to `sg` for parsing. Use `debug_query` to inspect parse output when matching is surprising. This is the same situation as C, C++, Ruby, and other extended languages.
- Unsafe cast detection (`$EXPR as $TYPE`): good warning-level scan rule for catching runtime ClassCastException risks. The safe cast `as?` is a different AST node, so this pattern does not false-positive on safe casts. Treat as review material; some casts are intentionally unsafe after exhaustive `when` or `is` checks.
- `var` vs `val` preference: use `kind: property_declaration` with `has: { field: property_delegate, pattern: var }` to match mutable property declarations. A naive `var $NAME: $TYPE` pattern may over-match in contexts where the parser attaches different node structure. The `kind` plus `has` plus `field` approach is more robust.
- `println` detection: use an `any` composite to cover Kotlin’s top-level `println($$$ARGS)`, Java’s `System.out.println($$$ARGS)`, and `System.err.println($$$ARGS)`. Scope with `files` to exclude test directories where console output is acceptable.
- `isEmpty()` preference: straightforward rewrite rule from `$X.size == 0` or `$X.length == 0` to `$X.isEmpty()`. Also cover `$X.count() == 0` and `$X.size <= 0`. This is a safe autofix because Kotlin’s `isEmpty()` is semantically equivalent for standard collections and strings.
- `lateinit` detection: pattern `lateinit var $NAME: $TYPE` is a direct structural match. Use `severity: info` because `lateinit` is sometimes justified in dependency injection and test setup contexts. Teams should adjust severity to match their policy.
- Unnecessary `let` blocks: pattern `$RECEIVER.let { $PARAM -> $BODY }` catches explicit named-parameter `let` calls. This does not match the implicit `it` form (`$RECEIVER.let { $BODY }`) because the parser structures those differently. Focus on the named-parameter variant as the more egregious anti-pattern.
- Data class candidates: use `kind: class_declaration` with `has: { kind: primary_constructor, has: { kind: class_parameter } }` to find classes with constructor parameters. This is a suggestion rule, not an enforcement rule, because classes with inheritance or behavior should remain regular classes.
- Key Kotlin tree-sitter node kinds for pattern authoring: `class_declaration` for classes, `property_declaration` for val/var properties, `function_declaration` for functions, `primary_constructor` for primary constructors, `class_parameter` for constructor parameters, `import_declaration` for imports, `call_expression` for function calls, `as_expression` for cast expressions, `lambda_expression` for lambdas, `when_expression` for when blocks.
- Kotlin tree-sitter parses `$EXPR as $TYPE` as `as_expression` and `$EXPR as? $TYPE` as a variant with the `?` token attached, so a pattern targeting `as` will not match `as?`. This makes cast-direction rules safe from false positives on safe casts.
- Kotlin’s `?.let { }` safe-call form is parsed differently from `.let { }` dot-call form. Rules targeting one will not match the other. Use `any` with both patterns when both forms should be flagged.
- Adapt Kotlin catalog rules and the rules above to the repository’s package naming, architecture boundaries, Android-vs-server structure, coroutine usage, and lint ownership before using them directly.

## Java Catalog Highlights

- Unused local variable detection: useful educational example for `has` plus ordered `all` plus `precedes`, but prefer the project’s established linter or IDE for real unused-variable enforcement because Java variable scopes are broader than the sample rule covers. The rule uses `all` to guarantee that the meta-variable `$IDENT` is captured by the first `has` clause before the `not`/`precedes` check runs. Without that ordering, the meta-variable would not be available for the later comparison:

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
fix: ‘’
```

Treat matches as review candidates, not conclusive unused-variable proofs. Java variable scopes are broader than this sample covers, and the project’s established linter or IDE is usually a better fit for real unused-variable enforcement.

- Field declarations of type `String`: good structural scan example showing why `field_declaration` plus `has: { field: type }` is more robust than a naive pattern when modifiers and annotations are present. A naive `String $F;` pattern fails because it ignores modifiers and annotations. A `$MOD String $F;` pattern also fails because tree-sitter does not consider `$MOD` a valid modifier and produces an `ERROR` node. The structural rule approach works regardless of how many modifiers or annotations precede the type:

```yaml
id: find-field-with-type
language: java
rule:
  kind: field_declaration
  has:
    field: type
    regex: ^String$
```

Use this `kind` plus `has` plus `field` plus `regex` pattern whenever a naive code pattern fails because Java modifiers, annotations, or access qualifiers change the surface syntax. The `field: type` constraint targets the semantic type child of the declaration, not the raw text, so it is robust against `private static final String`, `@Nullable String`, or other decorated forms.

- The Java catalog examples are primarily search/diagnostic material, not high-confidence autofix rules. Keep them review-oriented unless the repository explicitly wants ast-grep-based cleanup instead of compiler or linter diagnostics.
- Adapt Java catalog rules to the repository’s package conventions, annotation usage, style tooling, and existing static-analysis stack before using them directly.

## HTML Catalog Highlights

- HTML parser reuse for framework templates: useful when Vue, Svelte, Astro, or similar files are mostly HTML, but keep parser caveats in mind because framework-specific control flow or frontmatter may require a custom language instead. Use `languageGlobs` in `sgconfig.yml` to parse `.vue`, `.svelte`, or `.astro` files as HTML when the framework syntax is minimal enough for the HTML parser.
- Key HTML node kinds for pattern authoring: `element` for full HTML elements, `tag_name` for tag names, `attribute_name` for attribute names, `attribute_value` for attribute values, `text` for text content, and `comment` for HTML comments. Use these with `kind` to match specific HTML structures without writing full pattern syntax.
- Matching elements by tag name: use `kind: element` with `has: { field: tag_name, pattern: $TAG }` to match elements by their tag name. For regex-based tag matching (e.g. all heading tags), use `kind: tag_name` with `regex: "^h[1-6]$"` and `inside: { kind: element }`.
- Matching elements by attribute: use `kind: element` with `has: { kind: attribute_name, regex: "^class$" }` to find elements with a specific attribute. To also match the attribute value, add a nested `has` on the attribute node to capture `attribute_value`.
- Scoping with `inside` and `stopBy`: HTML `inside` with `stopBy: { kind: element }` scopes matches to the nearest enclosing element. This is essential for avoiding cross-element matches in deeply nested HTML. The `inside-tag` utility pattern from the catalog demonstrates wrapping `inside` with `kind: element` and `has` to capture the enclosing tag name, then using `constraints` to restrict which tags match.
- Ant Design Vue `visible` to `open`: good framework-specific attribute rewrite using enclosing-tag checks plus constraints. The pattern uses `kind: attribute_name` with `regex: :visible` to match the attribute, `inside` to find the enclosing `element`, `has` to capture the `tag_name`, and `constraints` to restrict to specific components (`a-modal|a-tooltip`). Keep it on the CLI skill path because framework version and component set must be confirmed first.
- i18n key extraction: useful template rewrite example for wrapping static text while skipping mustache expressions. Uses `kind: text` with `pattern: $T` to capture text content, `not: { regex: ‘{{.*}}’ }` to skip mustache interpolation, and `fix: "{{ $(‘$T’) }}"` to wrap the text. Keep it reviewable because real projects usually need key naming, dictionary updates, and whitespace policy beyond the raw rewrite.
- Attribute rewrite patterns: HTML attribute rewrites commonly use `kind: attribute_name` to match the target attribute, `inside` to find the parent element, and `constraints` to narrow by attribute name regex. For renaming attributes (e.g. `visible` to `open`), match the attribute name node and use `fix` to replace it.
- Text content patterns: use `kind: text` to match raw text nodes inside elements. Combine with `inside: { kind: element, has: { field: tag_name, pattern: $TAG } }` to scope text matching to specific elements. Use `not` to exclude text containing interpolation syntax.
- HTML comment patterns: use `kind: comment` to match HTML comments. Combine with `regex` to find comments containing specific text patterns like TODO, FIXME, or deprecated notices.
- HTML `<script>` and `<style>` content is parsed as embedded JavaScript and CSS respectively. Search inside these regions with `lang: javascript` or `lang: css` rules. For custom embedded languages (e.g. TypeScript in `<script lang="ts">`), configure `languageInjections` in `sgconfig.yml`.
- Adapt HTML catalog rules to the repository’s template framework, parser limitations, i18n workflow, and component-library version before using them directly.

## Go Catalog Highlights

- Problematic `defer` with nested function calls: strong Go-specific scan example for catching cases where deferred arguments are evaluated immediately instead of at function exit. In Go, `defer` evaluates arguments when the defer statement is encountered, not when the deferred function runs. This is particularly problematic with assertion libraries in tests:

```yaml
id: problematic-defer-call
language: go
rule:
  pattern:
    context: ‘{ defer $A.$B(t, failpoint.$M($$$)) }’
    selector: defer_statement
```

Treat matches as correctness and test-reliability review items. The fix is wrapping in a closure: `defer func() { require.NoError(t, failpoint.Disable("...")) }()`. Adapt to the repository’s test conventions before enabling broadly.

- Function declarations by name pattern: good example of using `kind` plus `has` plus `regex` when a meta-variable pattern cannot express the naming constraint directly. A plain `Test$_` pattern fails because it is not valid syntax; use a YAML rule instead:

```yaml
id: test-functions
language: go
rule:
  kind: function_declaration
  has:
    field: name
    regex: Test.*
```

Useful for test discovery, migration targeting, or repository audits where meta-variable patterns are too limited.

- Contextual matching for function calls: Go’s tree-sitter grammar parses `fmt.Println($A)` as a type conversion, not a call expression, because Go syntax allows both. Use a contextual pattern with `selector: call_expression` to disambiguate. Note: contextual patterns are pattern objects (`context` + `selector` inside `pattern`), which require the CLI skill path via `unified_exec`. The public structural surface’s `selector` field works with simple string patterns but does not support the `context` field:

```yaml
id: match-function-call
language: go
rule:
  pattern:
    context: ‘func t() { fmt.Println($A) }’
    selector: call_expression
```

Use this pattern whenever a plain call-expression pattern under-matches or parses as a conversion in Go.

- Package-import detection: useful search/scan rule for dependency auditing, compliance checks, or migration prep. Adapt the import regex to the repository’s actual dependency boundaries instead of hard-coding example packages:

```yaml
id: match-package-import
language: go
rule:
  kind: import_spec
  has:
    regex: github.com/golang-jwt/jwt
```

- Problematic JSON tags with `-,`: high-signal security-oriented scan rule for Go struct tags. When a struct field has a JSON tag starting with `-,`, it can be unexpectedly unmarshaled with the `-` key, bypassing the developer’s intent to omit the field. This is a real unmarshaling footgun, not just a style preference:

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

Treat matches as actionable security review items. The fix is using just `-` without a comma: `json:"-"`.

- Adapt Go catalog rules to the repository’s Go version, test conventions, package layout, security posture, and existing static-analysis tooling before using them directly.

## Cpp Catalog Highlights

- Reuse Cpp rules for C only when the repository intentionally parses C sources as Cpp via `languageGlobs`; do not assume mixed C/C++ projects want that parser tradeoff by default.
- C++ has no local tree-sitter parser in VT Code, so preflight pattern validation is skipped; patterns go directly to `sg` for parsing. Use `debug_query` to inspect parse output when matching is surprising.
- Format-string vulnerability rewrite: strong security-oriented example for `fprintf`/`sprintf`-style calls missing an explicit format string. Uses `constraints` with regex on `$PRINTF` and kind on `$VAR` to distinguish vulnerable calls from safe ones. The fix inserts `"%s"` as the format argument:

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

Keep it reviewable because real C/C++ codebases may prefer safer API migrations (e.g. `snprintf`) over mechanical `"%s"` insertion in some contexts.

- Struct inheritance matching: useful example of AST-shaped pattern authoring in C++. A shortened surface pattern like `struct $SOMETHING: $INHERITS` produces an `ERROR` node because tree-sitter-cpp requires the full syntactic form. Use the complete pattern with the body block:

```yaml
id: find-struct-inheritance
language: cpp
rule:
  pattern: struct $NAME : $BASE { $$$BODY; }
```

This matches structs that use inheritance via base class clauses. The full `struct ... : ... { ... }` shape is required for the parser to produce a valid `struct_specifier` node instead of an `ERROR` node.

- Adapt Cpp catalog rules to the repository’s C-vs-C++ parser choice, security posture, libc usage, and coding-standard expectations before using them directly.

## C Catalog Highlights

- Parsing C as Cpp can reduce duplicated rule authoring, but only use that route when the repository intentionally opts into the parser tradeoff with `languageGlobs`; do not blur C and C++ semantics by default.
- Match function calls in C with contextual patterns: tree-sitter-c parses code fragments differently depending on surrounding syntax. A bare `test($A)` becomes `macro_type_specifier`, while `test($A);` becomes `expression_statement -> call_expression`. Use `context` plus `selector: call_expression` to disambiguate. Note: contextual patterns are pattern objects (`context` + `selector` inside `pattern`), which require the CLI skill path via `unified_exec`. The public structural surface's `selector` field works with simple string patterns but does not support the `context` field:

```yaml
id: match-function-call
language: c
rule:
  pattern:
    context: $M($$$);
    selector: call_expression
```

- Rewrite method-style calls to function calls: useful migration example for C codebases that emulate methods with structs or function pointers. Uses `transform` with `replace` to derive a conditional comma from `$$$ARGS`:

```yaml
id: method_receiver
language: c
rule:
  pattern: $R.$METHOD($$$ARGS)
transform:
  MAYBE_COMMA:
    replace:
      source: $$$ARGS
      replace: ‘^.+’
      by: ‘, ‘
fix:
  $METHOD(&$R$MAYBE_COMMA$$$ARGS)
```

Keep it on the CLI skill path because it changes calling conventions and may affect ownership, pointer semantics, or naming policy.

- Yoda-condition rewrite: clearly style-driven and repository-policy-sensitive. Uses `constraints` to restrict `$B` to `number_literal` and `inside` to scope within `if_statement`:

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

Treat it as optional rewrite material only where the project explicitly prefers constant-on-the-left comparisons.

- Adapt C catalog rules to the repository’s parser choice, macro usage, pointer conventions, coding style, and safety policy before using them directly.

## Markdown Catalog Highlights

- Markdown became a first-class language in ast-grep 0.43. Query with `--lang md` or `lang: markdown` in YAML rules.
- Queryable node kinds include `atx_heading` for ATX-style headings, `fenced_code_block` for fenced code blocks, and `list_item` for list items.
- Combine node kinds with compound selectors for broader sweeps, for example `atx_heading, fenced_code_block` matches both headings and code blocks.
- Markdown parsing is powered by `tree-sitter-md` and still has known parsing bugs and edge cases. Use it for inspection, indexing, outline extraction, and lightweight automation rather than critical rewrites.
- VT Code infers `lang=md` from `.md` and `.mdx` file paths and globs, so structural queries over Markdown files do not always require an explicit `lang` argument.
- Adapt Markdown catalog rules to the repository’s documentation conventions, heading hierarchy, and content structure before using them directly.

## JavaScript API Highlights

- Use `@ast-grep/napi` only when rule YAML or VT Code’s public structural tool is not enough. The programmatic API is the right escalation path for computed replacements, ordered-match logic, cross-node inspection, or edit orchestration that would be awkward in pure rule syntax.
- Core objects are `SgRoot` and `SgNode`: `parse(Lang.<X>, source)` creates the tree, `root()` returns the root node, and `find` / `findAll` / traversal / refinement / edit APIs live on `SgNode`.
- `Matcher` inputs can be pattern strings, numeric kind ids, or `NapiConfig` objects. Prefer patterns or `NapiConfig` unless there is a concrete reason to drop to raw kind ids.
- `getMatch` and `getMultipleMatches` expose captured metavariables, but `replace` does not interpolate metavariables for you. Build replacement strings explicitly in JavaScript from matched nodes before calling `commitEdits`.
- Keep VT Code’s boundary clear: prefer the public structural tool or CLI path for ordinary query/scan/test/rewrite flows, and only drop to NAPI when the task is genuinely programmatic.
- `registerDynamicLanguage` and extra language packages exist, but that path is still experimental. Prefer established parsers and repo-native tooling unless dynamic-language support is actually needed.

## Python API Highlights

- Use `ast-grep-py` when the task needs programmatic AST traversal or computed edits but a Python host environment is a better fit than JavaScript or Rust. As with NAPI, prefer it only after rule YAML or VT Code’s structural/CLI path stops being a good fit.
- Core objects are again `SgRoot` and `SgNode`: `SgRoot(source, language)` parses the source, `root()` returns the root node, and search, refinement, traversal, and edit APIs live on `SgNode`.
- `find` and `find_all` support either direct rule keyword arguments or a config object. Prefer keyword-rule searches for simple cases and config objects when constraints or utility rules make the query more expressive.
- `get_match`, `get_multiple_matches`, and `__getitem__` expose captured metavariables. `__getitem__` is useful when you want a stricter access pattern and are willing to let missing captures raise instead of returning `None`.
- `replace` and `commit_edits` generate source edits, but they do not interpolate metavariables for you. Build replacement text explicitly from matched nodes before applying edits.
- Keep VT Code’s boundary clear here too: use Python API only for genuinely programmatic transformations, not as a default substitute for public structural queries or ordinary CLI rewrites.

## NAPI Performance Highlights

- NAPI is not automatically faster than host-language traversal. Performance usually comes from reducing Rust↔JavaScript FFI crossings and letting ast-grep do more work per boundary crossing.
- Prefer `parseAsync` over `parse` when many parse jobs can benefit from Node’s libuv thread pool and the main JS thread is already busy.
- Prefer `findAll` over manual recursive traversal in JavaScript. One bulk Rust-side search is usually cheaper than repeated `kind()`, `children()`, and recursion calls across the FFI boundary.
- Prefer `findInFiles` when scanning many files and you can use its file-path-oriented search model. It avoids unnecessary round-tripping source strings through JavaScript and can parallelize work in Rust threads.
- `findInFiles` has a callback-completion caveat: its returned promise can resolve before all callbacks run. If completion ordering matters, track callback counts explicitly before treating the scan as finished.
- Apply these performance tips only when scale justifies them. For small inputs or one-off transformations, simpler synchronous code is often the better tradeoff.

## Rule Essentials

- Start rule files with `id`, `language`, and root `rule`.
- Treat the root `rule` as a rule object that matches one target AST node per result.
- A rule object still needs a positive anchor. In practice, start with `pattern` or `kind`; `regex` is a filter, not a sufficient root rule by itself.
- Atomic rules are `pattern`, `kind`, `regex`, `nthChild`, and `range`.
- Use atomic fields such as `pattern`, `kind`, `regex`, `nthChild`, and `range` for direct node checks.
- In VT Code's public structural surface, `kind` is available as a first-class field alongside `pattern`. Use `kind` alone to match by node type without a pattern, or combine both to filter pattern matches by node kind.
- Use relational fields such as `inside`, `has`, `follows`, and `precedes` when the match depends on surrounding nodes.
- Use composite fields such as `all`, `any`, `not`, and `matches` to combine sub-rules or reuse utility rules.
- `kind` values support ESQuery-style pseudo-selectors (`:has()`, `:not()`, `:is()`, `:nth-child()`) for matching nodes by descendant structure, exclusion, alternatives, or sibling position without writing separate relational rules.
- Rule object fields are effectively unordered and conjunctive; if matching becomes order-sensitive, rewrite the logic with an explicit `all` sequence instead of assuming YAML key order matters.
- `language` controls how patterns parse. Syntax that is valid in one language can fail in another.

## Rule Cheat Sheet

- Atomic rules check properties of one node. Start here when a single syntax shape is enough.
- `pattern`, `kind`, and `regex` are the common atomic fields. `pattern` can also be an object with `context`, `selector`, and optional `strictness`.
- Pattern objects are for invalid, incomplete, or ambiguous snippets. `context` is required; `selector` picks the real target node inside that context; `strictness` tunes how literally the pattern matches.
- Use pattern objects when the bare snippet would parse as the wrong node kind, such as JavaScript class fields or Go/C call expressions inside ambiguous fragments.
- `kind` is usually a plain node kind name, but ast-grep 0.42+ supports ESQuery-style pseudo-selectors in `kind` strings. Use `:has(selector)` or `:has(> selector)` to match nodes containing descendants (or direct children) matching a selector, `:not(selector)` to exclude nodes, `:is(selector, ...)` for or-logic in compound selectors, and `:nth-child(An+B)` or `:nth-child(An+B of selector)` for positional matching. These pseudo-selectors also work in `selector` values on the CLI and in VT Code's public structural surface.
- ast-grep 0.43+ further expands `kind` with compound selector operators: `A > B` (direct child), `A B` (descendant), `A + B` (immediate sibling), `A ~ B` (general sibling), and `A, B` (either). This syntax works in YAML rule `kind` fields and the CLI `--kind` / `-k` flag. It is ESQuery-style, not full ESQuery: class selectors, attribute selectors, and wildcard selectors are not supported.
- Separate `kind` and `pattern` checks do not change how the pattern is parsed. If parse shape is the problem, switch to one pattern object with `context` and `selector`.
- `regex` matches the whole node text. Reach for `nthChild` when position among named siblings matters and `range` when the match must be limited to a known source span.
- Regex syntax follows Rust `regex`, not PCRE. Do not assume look-around or backreferences are available, and usually pair `regex` with `kind` or `pattern` so the expensive text check only runs on the right node shapes.
- `nthChild` accepts a number, an `An+B` string, or an object with `position`, `reverse`, and `ofRule`. Counting is 1-based and only considers named siblings.
- `range` matches by source position with 0-based `line` and `column`; `start` is inclusive and `end` is exclusive.
- Relational rules describe structure around the target node. Use `inside`, `has`, `follows`, and `precedes` when the match depends on ancestors, descendants, or neighboring nodes.
- Read relational rules as: target node relates to surrounding node. The top-level rule still matches the target; the relational subrule matches the surrounding node that filters it.
- Relational subrules can themselves use `pattern`, `kind`, composites, and captures. Those captures can still be referenced later in `fix`, which is a practical way to extract surrounding syntax while keeping the target node as the match.
- Add relational `field` when the surrounding node matters by semantic role, not just by shape. `field` only applies to `inside` and `has`.
- Add `stopBy` when ancestor or sibling traversal must continue past the nearest boundary instead of stopping early. The default is `neighbor`, `end` searches to the boundary, and a rule object stop is inclusive.
- `inside` means the target is somewhere under a matching ancestor, `has` means the target node contains a matching descendant, `follows` means the target comes after a matching sibling or prior node, and `precedes` means it comes before one.
- Composite rules combine checks for the same target node. Use `all` for explicit conjunction, `any` for alternatives, `not` for exclusions, and `matches` to delegate to a utility rule.
- `all` and `any` still operate on one target node. They combine sub-rules, not multiple matched nodes.
- `all` is the ordered composite. Use it when later checks depend on captures established by earlier `pattern` matches, because YAML rule-object field order is not guaranteed.
- `any` is for alternatives, not for "collect all matching nodes". If one node cannot satisfy every branch at once, `all` is the wrong operator even if the surrounding structure feels plural.
- Nested composites still evaluate one node at a time. A rule like `has: { all: [...] }` means "has one child satisfying every listed rule", not "has one child for each listed rule".
- When you need "a node that has X child and has Y child", write `all: [ { has: ... }, { has: ... } ]` on the outer target instead of putting incompatible checks inside one nested `all`.
- Utility rules keep repeated logic out of the main rule body. Use file-local `utils` for one config file and global utility-rule files when multiple rules in the project need the same building block.
- Local utility rules live under the current file's `utils` map, are only visible in that one config file, inherit the file's language, and cannot define their own separate `constraints`.
- Global utility rules live in separate files discovered through `utilDirs`. They must declare `id` and `language`, and can only use the utility-safe fields: `id`, `language`, `rule`, `constraints`, and local `utils`.
- Local utility names must be unique inside one file. A local utility can shadow a global utility with the same name, so check the nearest file first when `matches` seems to resolve unexpectedly.
- Utility rules can call other utility rules through `matches`, including recursive structural tricks like nested-parentheses matching. Avoid cyclic `matches` dependency graphs, because ast-grep does not allow recursive cycles there.
- Self-reference through relational structure such as `inside` or `has` is different from cyclic `matches` reuse and is allowed when the AST traversal still makes progress.
- Switch from a single `pattern` to a rule object when you need positional constraints, role-sensitive matching, reusable sub-rules, or several structural conditions on one node.
- Rule-object fields are logically equivalent to an `all` across those fields, but not to an ordered `all`. Keep explicit `all` when capture order matters; use rule-object style when the checks are independent and flatter indentation helps readability.

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

- Basic info keys define the rule itself. Use `id` for the unique rule name, `language` for the parser target, `url` for rule documentation, and `metadata` for custom project data that VT Code should preserve with the rule.
- One YAML file can hold multiple rules when you separate documents with `---`.
- Finding keys define what gets matched. `rule` is the core matcher, `constraints` narrows meta-variable captures, and `utils` holds reusable helper rules that you call through `matches`.
- `utils` can be purely local to the current file or can supplement global utility-rule files loaded through `utilDirs`. Keep shared building blocks global only when multiple rule files genuinely need them.
- `constraints` runs after `rule` matched, only targets single meta variables like `$ARG`, and is a poor fit inside `not`.
- Patching keys define reusable fixes. Use `transform` to derive new meta-variables before replacement, `fix` for either a string replacement or a `template` object with `expandStart` / `expandEnd`, and `rewriters` when the transformation is too complex for one inline `fix`.
- Linting keys define what scan results report. Use `severity`, `message`, `note`, and `labels` for diagnostics, then `files` and `ignores` to scope where the rule applies.
- Severity levels are `error`, `warning`, `info`, `hint`, and `off`. `hint` is the default severity in ast-grep project scans.
- `error` findings make raw `ast-grep scan` exit non-zero; VT Code normalizes that CLI behavior into structured findings on the public scan path instead of surfacing a tool error.
- `severity: off` disables the rule during scanning. `note` supports Markdown but cannot interpolate meta variables.
- Source suppression uses `ast-grep-ignore` comments.
  - `ast-grep-ignore` suppresses all rules for the same line or following line
  - `ast-grep-ignore: rule-id` suppresses one rule
  - comma-separated rule ids suppress multiple specific rules
  - next-line suppression only works when there is no preceding AST node on that same comment line
- File-level suppression requires the suppression comment on the first line plus an empty second line.
- `unused-suppression` is a built-in hint-style rule with autofix for stale ignore directives, but it only appears in full `scan` runs when ast-grep is not filtering or disabling rules through CLI narrowing flags.
- `labels` keys must come from meta variables already defined by the rule or `constraints`.
- `files` supports either plain globs or object entries. Use object syntax when you need options like `caseInsensitive` glob matching.
- `ignores` runs before `files`. Both are relative to the `sgconfig.yml` directory, and the glob should not start with `./`.
- Rule-level `ignores` is different from CLI `--no-ignore`: the CLI flag changes global ignore-file behavior, while YAML `ignores` only filters files for that rule.
- JSON output only includes rule `metadata` when the ast-grep run enabled metadata output, for example via `--include-metadata`.
- Parameterized utility rules (experimental, ast-grep 0.42+) let global utility files declare `arguments` so callers pass rule objects into a reusable template via `matches`. Arguments are mandatory, are full rule objects (not strings), and meta-variables captured inside the utility stay private unless explicitly exported by the argument rules. This feature is experimental and its API may change.
- Keep config authoring on the ast-grep skill path. VT Code’s public structural tool runs read-only query/scan/test workflows; it does not expose rule-YAML authoring fields directly.

## Transformation Objects

- `transform` builds new strings from captured meta variables before `fix` runs.
- Each `transform` entry introduces a new variable name without a leading `$`. Inside the transform object, `source` still points at an existing capture or prior transform result using the normal `$VAR` form.
- Later transforms can consume variables created by earlier transforms, so transform order matters when you are stacking multiple string operations.
- Transforms are evaluated in declaration order. A transform that references a variable created by an earlier transform in the same `transform` block will see the already-transformed value.
- Transforms only run after the rule matches. If the rule does not match, no transforms execute and no `fix` is applied.

### replace

- `replace` uses a Rust regex over one meta variable. `source` must be `$VAR` style, `replace` is the regex, `by` is the replacement text, and regex capture groups can be reused in `by`.
- Regex capture groups are only available inside the `replace` field of the `replace` transform and can only be referenced from the `by` field of that same transform. Regular `regex` rules do not expose those capture groups.
- Rust regex syntax applies: no look-around, no backreferences in the traditional PCRE sense, but capture groups `()` work and are referenced as `$1`, `$2`, etc. in `by`.

```yaml
# Strip leading underscore from a variable name
transform:
  CLEAN_NAME:
    replace:
      replace: "^_"
      by: ""
      source: $VAR

# Extract domain from email using capture group
transform:
  DOMAIN:
    replace:
      replace: "^[^@]+@(.+)$"
      by: "$1"
      source: $EMAIL

# String-form (ast-grep 0.38.3+)
transform:
  CLEAN_NAME: replace($VAR, replace="^_", by="")
```

### substring

- `substring` slices a meta variable by character index with inclusive `startChar` and exclusive `endChar`. Negative indexes count from the end, and slicing is based on Unicode characters rather than raw bytes.
- `substring` behaves like Python string slicing, so omit either bound when the slice should stay open-ended.

```yaml
# Remove first and last character (e.g., strip quotes)
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

# Remove first character only
transform:
  NO_PREFIX:
    substring:
      startChar: 1
      source: $NAME

# String-form (ast-grep 0.38.3+)
transform:
  UNQUOTED: substring($STR, startChar=1, endChar=-1)
```

### convert

- `convert` changes identifier-style casing through `toCase`. Common outputs are `lowerCase`, `upperCase`, `capitalize`, `camelCase`, `snakeCase`, `kebabCase`, and `pascalCase`.
- Use `separatedBy` to control how `convert` splits words before rebuilding the target case. Supported separators include dash, dot, space, slash, underscore, and `CaseChange`.
- `CaseChange` splits at transitions such as `astGrep`, `ASTGrep`, or `XMLHttpRequest`, which matters when converting mixed acronym identifiers.
- When `separatedBy` is omitted, all known separators are used. This is usually the right default.

```yaml
# Convert camelCase to snake_case
transform:
  SNAKE_NAME:
    convert:
      toCase: snakeCase
      source: $CAMEL

# Convert only by underscore, preserving camelCase within segments
transform:
  KEBAB_FROM_UNDERSCORE:
    convert:
      toCase: kebabCase
      separatedBy: [underscore]
      source: $UNDERSCORE_NAME

# Convert PascalCase to camelCase (using CaseChange separator)
transform:
  CAMEL:
    convert:
      toCase: camelCase
      separatedBy: [CaseChange]
      source: $PASCAL

# String-form (ast-grep 0.38.3+)
transform:
  SNAKE_NAME: convert($CAMEL, toCase=snakeCase)
```

### Chaining Transforms

- Later transforms can consume variables created by earlier transforms. This is the standard way to build multi-step string pipelines.
- Transform order is the declaration order in the YAML `transform` map.

```yaml
# Pipeline: strip prefix, then convert case
transform:
  RAW_NAME:
    replace:
      replace: "^get"
      by: ""
      source: $METHOD_NAME
  SNAKE:
    convert:
      toCase: snakeCase
      source: $RAW_NAME
# Input: "getUserName" -> RAW_NAME="UserName" -> SNAKE="user_name"
```

### Conditional Separators from Multi-Capture

- Use `replace` transforms for conditional punctuation or whitespace when a multi-capture may be empty. The common pattern is deriving `MAYBE_COMMA` or similar from `$$$ARGS` so the extra separator only appears when matches exist.

```yaml
# Add comma only when there are arguments
rule:
  pattern: "foo($$$ARGS)"
transform:
  MAYBE_COMMA:
    replace:
      replace: ".+"
      by: ", "
      source: $ARGS
fix: "bar($MAYBE_COMMA$newArg)"
# If $$$ARGS matched "a, b" -> MAYBE_COMMA=", " -> "bar(, newArg)"
# If $$$ARGS matched nothing -> MAYBE_COMMA="" -> "bar(newArg)"
```

### String-Form Transforms

- String-form transforms such as `replace(...)`, `substring(...)`, `convert(...)`, and `rewrite(...)` are valid shorthand in ast-grep 0.38.3+.
- Prefer object form when compatibility or debugging clarity matters.
- String-form syntax: `operator($SOURCE, key1=value1, key2=value2)`.
- Array values use `[item1, item2]` syntax inside the string form.

## Rewriters

- `rewriters` is an experimental feature for advanced multi-node rewrites. Prefer ast-grep’s API instead when the YAML starts carrying too much control flow or state.
- Rewriters allow replacing multiple sub-nodes with different fixes in one rule. The normal `fix` replaces one matched node at a time; `rewriters` plus `transform.rewrite` handle the one-to-many case.
- The three-step workflow is:
  1. Define `rewriters` at the YAML rule root. Each rewriter needs `id`, `rule`, and `fix`. Optional fields are `constraints`, `transform`, and `utils`.
  2. Apply the rewriter to a metavariable via `transform` using the `rewrite` operator. `rewriters` lists which rewriter ids to try; `source` points at the metavariable whose sub-nodes are rewritten.
  3. Use the resulting transformed metavariable in the outer `fix`.
- Concrete example converting Python `dict(a=1, b=2)` to `{‘a’: 1, ‘b’: 2}`:
  - Define a rewriter that matches `keyword_argument` nodes and rewrites `$KEY=$VAL` to `’$KEY’: $VAL`.
  - Apply it to `$$$ARGS` captured from `dict($$$ARGS)` via `transform: { LITERAL: { rewrite: { rewriters: [dict-rewrite], source: $$$ARGS } } }`.
  - Use `fix: ‘{ $LITERAL }’` on the outer rule to wrap the rewritten arguments in braces.
- Multiple rewriters can be listed in one `transform.rewrite` call. Each sub-node is transformed by the first matching rewriter in declaration order. If two rewriters could match the same node, only the one that appears earlier in the `rewriters` list is applied. Order matters.
- `joinBy` controls how transformed sub-nodes are stitched together. By default, sub-nodes are replaced in-place preserving original separators. Set `joinBy` to a string like `’ + ‘` or `’\n’` to override the joiner.
- A rewriter can call other rewriters from the same `rewriters` list inside its own `transform` section, enabling multi-pass rewrite pipelines.
- Meta variables captured inside one rewriter do not leak to sibling rewriters or the outer rule. Rewriter-local `transform` variables and `utils` are also scoped to that one rewriter.
- String-form shorthand `rewrite(rewriters, source, joinBy?)` is valid in newer ast-grep versions, but prefer object form when compatibility or debugging clarity matters.
- For simple pattern-to-pattern rewrites, use `workflow="rewrite"` on the public structural surface to preview replacements without applying them. This runs `ast-grep run --pattern=... --rewrite=... --json=compact --color=never` and returns each match with its proposed `replacement` and `replacementOffsets`. The surface remains read-only; no files are modified.
- For advanced rewrite operations using `rewriters`, `transform.rewrite`, `joinBy`, or `FixConfig` with `expandStart`/`expandEnd`, use the CLI skill path via `unified_exec`. VT Code’s public structural surface does not expose multi-rewriter or transform-pipeline behavior.

## Pattern Syntax

- Pattern code must be valid code that tree-sitter can parse.
- Patterns match syntax trees, so a query can match nested expressions instead of only top-level text.
- `$VAR` matches one named AST node.
- `$$$ARGS` matches zero or more AST nodes in places like arguments, parameters, or statements.
- Reusing the same captured name means both occurrences must match the same syntax.
- Prefixing a meta variable with `_` disables capture, so repeated `$_X` occurrences do not need to match the same content.
- `$$VAR` captures unnamed nodes when named-node matching is too narrow.
- If a short snippet is ambiguous, move to an object-style pattern with more `context` plus `selector` instead of guessing.

## Pattern Parsing Deep Dive

- Pattern creation has four stages: preprocess meta-variable text when the language needs a custom `expandoChar`, parse the snippet, choose the effective node, then detect meta variables inside that effective node.
- Invalid pattern code usually fails because a meta variable is standing in for syntax that the parser treats as an operator or keyword. Patterns like `$LEFT $OP $RIGHT` or `{ $KIND foo() {} }` should become rule objects using parseable code plus `kind`, `regex`, `has`, or other rule fields.
- Incomplete or ambiguous snippets can appear to work only because tree-sitter recovered from an error. Treat that as best-effort behavior, not a stable contract across ast-grep upgrades, and prefer valid `context` plus `selector`.
- The default effective node is the leaf node or the innermost node with more than one child. Override it with `selector` when the real match should be a statement instead of the inner expression, especially for `follows` and `precedes`.
- Meta variables are detected only when the whole AST node text matches meta-variable syntax. Mixed text like `obj.on$EVENT`, lowercase names like `$jq`, or string-content fragments do not become meta variables.
- `$$VAR` captures unnamed nodes such as operators when the grammar exposes them only as anonymous tokens. `$$$ARGS` is lazy: it stops before the next node that satisfies the rest of the pattern.
- When pattern behavior is surprising, inspect the parsed tree and effective node first. In VT Code, start with public structural `debug_query`; in the Playground, use the pattern view for the same questions.

## Pattern Core Concepts

- ast-grep is structural search, not plain text search. `pattern` matches syntax-tree shape, while `regex` is the escape hatch when node text itself matters.
- Tree-Sitter gives ast-grep a concrete syntax tree, not a stripped-down abstract syntax tree. That CST detail is why punctuation and modifiers can still matter even when matching stays syntax-aware.
- Named nodes carry a `kind`; unnamed nodes are punctuation or literal tokens. Meta variables match named nodes by default, and `$$VAR` is the opt-in when unnamed nodes matter.
- `kind` belongs to the node itself. `field` belongs to the parent-child relationship, so use relational `has` or `inside` with `field` when role matters more than raw node kind.
- Pseudo-selectors extend `kind` matching with CSS/ESQuery-style combinators. They are string-level refinements on node kind names, not separate rule fields, so they compose naturally with pattern and relational rules.
- A node is significant to ast-grep when it is named or has a `field`. Trivial nodes can still matter for exact matching, so do not assume every important token has its own named node.

## Match Algorithm

- The default strictness is `smart`. Every node you spell out in the pattern is respected, but unnamed nodes in the target code can be skipped.
- Unnamed nodes written in the pattern are not skipped. A shorter pattern like `function $A() {}` can match `async function`, while `async function $A() {}` requires `async` to be present.
- Use strictness to tune what ast-grep may skip during matching.
  - `cst`: skip nothing
  - `smart`: default, skip unnamed nodes in code only
  - `ast`: skip unnamed nodes on both sides
  - `relaxed`: also skip comments
  - `signature`: ignore text and compare mostly named-node kinds
- This explains why quote differences can disappear under `ast`, comments can disappear under `relaxed`, and even different callee text can match under `signature`.
- In VT Code, read-only structural queries already expose `strictness`. Use the bundled skill when the task is choosing between levels, or when the user needs raw CLI `--strictness` or YAML pattern-object `strictness`.

## Custom Languages

- Use custom language support when the parser exists in tree-sitter form but ast-grep does not ship it as a built-in language.
- The basic workflow is:
  - install `tree-sitter` CLI and obtain the grammar
  - compile the parser as a shared library
  - register it in workspace `sgconfig.yml` under `customLanguages`
- Prefer `tree-sitter build --output <lib>` to compile the dynamic library. If the installed tree-sitter is too old for `build`, use `TREE_SITTER_LIBDIR` with `tree-sitter test` as the fallback path.
- Reusing a parser library built by Neovim is valid when it already matches the grammar/version you need.
- Register `libraryPath`, `extensions`, and optional `expandoChar` in `sgconfig.yml`. `expandoChar` matters when `$VAR` is not valid syntax in the target language and must be rewritten to a parser-friendly prefix.
- Use `tree-sitter parse <file>` to inspect parser output when the custom grammar or file association is unclear.
- VT Code’s public structural queries can use a custom language only after the local ast-grep project config is in place. The setup, compilation, and debugging work stays on the bundled ast-grep skill path.

## Language Injection

- ast-grep can search embedded languages inside a host document. Built-in injection already covers HTML with CSS in `<style>` and JavaScript in `<script>`.
- Use `languageInjections` in `sgconfig.yml` when the embedded language is project-specific, such as CSS inside styled-components or GraphQL inside tagged template literals.
- A `languageInjections` entry needs `hostLanguage`, a `rule`, and `injected`. The `rule` should capture the embedded subregion with a meta variable such as `$CONTENT`.
- The `$CONTENT` meta variable in the injection rule designates which portion of the host match should be parsed as the injected language. Without it, ast-grep cannot identify the embedded region.
- Typical patterns are `styled.$TAG\`$CONTENT\`` for CSS-in-JS and `graphql\`$CONTENT\`` for GraphQL template literals.
- ast-grep parses the extracted subregion with the injected language, not the parent document language. That is why CSS patterns can match inside JavaScript once injection is configured.
- Use `languageGlobs` when the whole file should be parsed as a different or superset language. Use `languageInjections` when only a nested region inside the file changes language.
- In VT Code, use `workflow='inspect'` on the public structural surface to see configured `languageInjections`, `customLanguages`, and `languageGlobs` from the project's `sgconfig.yml`.
- In VT Code, read-only structural query / scan / test can consume existing injection config. Designing or debugging `languageInjections` itself stays on the bundled ast-grep skill path.

### Injection Config Examples

```yaml
# CSS-in-JS (styled-components)
languageInjections:
- hostLanguage: js
  rule:
    pattern: styled.$TAG`$CONTENT`
  injected: css

# GraphQL tagged template literals
- hostLanguage: js
  rule:
    pattern: graphql`$CONTENT`
  injected: graphql

# SQL tagged template literals
- hostLanguage: js
  rule:
    pattern: sql`$CONTENT`
  injected: sql
```

### Dynamic Injected Language

- Use dynamic `injected` candidates when the rule captures `$LANG` and the embedded language must be chosen from a list such as `css`, `scss`, or `less`.
- This is useful for framework-specific template directives where the language tag is part of the matched syntax.

### Injection vs. languageGlobs vs. customLanguages

- `languageGlobs` remaps entire files to a different parser. Use it when the file extension does not match ast-grep's built-in mapping (e.g., parsing `.ts` files as TSX).
- `languageInjections` extracts a sub-region of a file and parses it as a different language. Use it for embedded languages like CSS in JS or SQL in template literals.
- `customLanguages` registers a new tree-sitter parser for a language ast-grep does not ship. Use it when the target language has a tree-sitter grammar but is not built in.
- All three are configured in `sgconfig.yml` and consumed automatically by `ast-grep scan` and VT Code's structural workflows.

## FAQ Highlights

- If a pattern fragment fails, the usual fix is to provide more valid `context` and then narrow the real target with `selector`. This is the standard workaround for subnodes like JSON pairs or class fields that are not standalone code.
- If a rule behaves strangely, reduce it to the smallest repro, confirm whether it is matching an expression or a statement, and use `all` to make rule order explicit when later checks depend on earlier meta-variable captures.
- CLI and Playground can disagree because parser versions and text encodings differ. In VT Code, prefer the public structural `debug_query` flow first, then compare the parsed AST or CST before assuming the rule is wrong.
- Meta variables must occupy one whole AST node. `use$HOOK` and similar prefix/suffix patterns will not work; capture the full node and narrow it with `constraints.regex` instead. Use `$$VAR` for unnamed nodes, and remember that `$$$MULTI` is lazy.
- Do not combine separate `kind` and `pattern` rules to force a different parse shape. Use one pattern object with `context` and `selector` so the parser sees the intended node kind.
- ast-grep rules are single-language. Share coverage across related languages by parsing both with the superset via `languageGlobs`, or keep separate rules when the AST differences matter.
- ast-grep does not provide scope, type, control-flow, data-flow, taint, or constant-propagation analysis. If the task needs those, switch tools instead of stretching rule syntax.

## Find & Patch

- ast-grep rewrites are still find first, patch second. `rule` plus optional `constraints` finds the target, `transform` derives replacement strings, and `fix` patches the final text.
- The simple workflow rewrites one matched node at a time. When one node must expand into multiple outputs, use `rewriters` plus `transform.rewrite` instead of forcing everything into one inline `fix`.
- `transform.rewrite` lets you run sub-rules over a matched meta-variable, generate one fix per sub-node, and join the results with `joinBy`.
- `transform.rewrite` is still experimental. It rewrites descendants of the captured source, prevents overlapping rewriter matches, prefers higher-level AST matches first, and for one node only applies the first matching rewriter in the declared order.
- This is the right model for list-style rewrites such as exploding a barrel import into multiple single imports, converting `dict(a=1, b=2)` to `{‘a’: 1, ‘b’: 2}`, or transforming heterogeneous lists where each element type needs a different rewrite rule.
- `transform.rewrite` has three important behavioral properties: (1) it rewrites descendants of the captured source metavariable, not the source itself; (2) overlapping rewriter matches are prevented so each sub-node is rewritten at most once; (3) higher-level AST matches are preferred before nested ones, and for one node only the first matching rewriter in declaration order is applied.
- Use `joinBy` when the rewritten sub-nodes must be stitched with a different separator than the original source text. For example, `joinBy: "\n"` converts comma-separated imports into newline-separated direct imports.
- For simple pattern-to-pattern rewrites, use `workflow="rewrite"` on the public structural surface to preview replacements without applying them. Each result includes the original `text`, proposed `replacement`, `replacementOffsets`, and `metaVariables`. The surface remains read-only; no files are modified.
- For FixConfig rewrites with range expansion (expandStart/expandEnd), use `workflow="rewrite"` with `fix_config` on the public structural surface. The tool generates a temporary YAML rule and runs `sg scan` internally.
- For advanced `transform.rewrite`, `rewriters`, `joinBy`, and multi-pass transform operations, use the CLI skill path via `unified_exec`.

## Rewrite Essentials

- Use `ast-grep run --pattern ... --rewrite ...` for one-off rewrites.
- Use YAML `fix` in rule files for reusable rewrites that should live with the rule.
- Use `--interactive` to review rewrite hunks before applying them.
- Use `--update-all` or `-U` only when the user clearly wants non-interactive apply behavior.
- Meta variables captured in `pattern` can be reused in `fix`.
- String `fix` is raw replacement text, not a parsed Tree-Sitter pattern. Meta variables can appear anywhere in the replacement string.
- `fix` indentation is preserved relative to the matched source location, so multiline rewrites must be authored with deliberate indentation.
- Non-matched meta variables become empty strings in rewritten output.
- If appended uppercase text would be parsed as part of a meta variable name, use transforms instead of writing `$VARName` directly.
- Use `transform.rewrite` when a matched list must be rewritten element-by-element before the outer `fix` runs.
- Use `joinBy` to control how rewritten list items are stitched together, for example newline-joined imports in a barrel-import rewrite.
- Use `FixConfig` when replacing only the matched node is not enough, especially for deleting list items or key-value pairs that also need a surrounding comma removed.
- In `FixConfig`, `template` is the replacement text and `expandStart` / `expandEnd` widen the rewritten range to consume commas, brackets, or other surrounding trivia outside the target node.
- On the public structural surface, `workflow="rewrite"` supports FixConfig via the `fix_config` parameter. This is the preferred path for rewrites that need range expansion. The tool generates a temporary YAML rule and runs `sg scan` internally.
- When `fix_config` is used, each result includes `replacement` (the template), `file`, `line_number`, `range`, and `message` from the matched rule. The `fix_config` object is echoed back so callers can confirm the expansion config that was applied.

### FixConfig Examples

Delete a key-value pair and its trailing comma from a YAML-like structure:

```yaml
rule:
  kind: pair
  has:
    field: key
    regex: Remove
fix:
  template: ''
  expandEnd:
    regex: ','
```

In the public structural tool, this becomes:

```json
{
  "action": "structural",
  "workflow": "rewrite",
  "lang": "javascript",
  "pattern": "$KEY: $VAL",
  "fix_config": {
    "template": "",
    "expand_end": {
      "regex": ","
    }
  }
}
```

Delete an array element and its surrounding comma, expanding both start and end:

```yaml
fix:
  template: ''
  expandStart:
    regex: ','
    stopBy: line
  expandEnd:
    regex: ','
```

Replace a function call argument while consuming surrounding whitespace:

```yaml
rule:
  pattern: foo($ARG)
fix:
  template: 'bar($ARG)'
  expandStart:
    kind: '('
  expandEnd:
    kind: ')'
```

In the public structural tool:

```json
{
  "action": "structural",
  "workflow": "rewrite",
  "lang": "javascript",
  "pattern": "foo($ARG)",
  "fix_config": {
    "template": "bar($ARG)",
    "expand_start": { "kind": "(" },
    "expand_end": { "kind": ")" }
  }
}
```

- Keep advanced `transform` and `rewriters` in the skill-driven CLI workflow.

## Run Command Basics

- `ast-grep -p 'foo()'` and `ast-grep run -p 'foo()'` are equivalent. `run` is the default subcommand.
- `ast-grep run` defaults to searching `.` when no path is provided and can search multiple paths in one invocation.
- `--globs` includes or excludes paths and overrides ignore-file behavior. Prefix a glob with `!` to exclude, and let later globs win when multiple patterns match.
- `--no-ignore` changes which ignore sources ast-grep respects. The supported categories are `hidden`, `dot`, `exclude`, `global`, `parent`, and `vcs`.
- `--follow` makes ast-grep traverse symlinks. Expect loop or broken-link errors to surface directly from the CLI when the filesystem is invalid.

## Scan Command Basics

- `ast-grep scan` defaults to searching `.` when no path is provided and can search multiple paths in one invocation.
- `--config <file>` points scan at a project `sgconfig.yml` root. It is the default scan mode in VT Code’s public `workflow="scan"` surface.
- `--rule <file>` runs one YAML rule file without project setup and conflicts with `--config`.
- `--inline-rules '...'` runs one or more inline YAML rules without creating a file on disk. Separate multiple rules with YAML `---`. It conflicts with `--rule`.
- `--filter <regex>` narrows project-config scan to matching rule ids and conflicts with `--rule`.
- `--include-metadata` only affects JSON output and is already enabled on VT Code’s public scan path so normalized findings can carry rule metadata.

## Test Command Basics

- `ast-grep test` validates rule tests from the ast-grep project config.
- Rule test files are YAML with `id`, `valid`, and `invalid`. `valid` cases should produce no issue; `invalid` cases should produce at least one issue.
- Ast-grep’s test output distinguishes four outcomes:
  - `reported`: invalid code correctly reports
  - `validated`: valid code correctly stays quiet
  - `noisy`: valid code reported unexpectedly
  - `missing`: invalid code was not reported
- `--config <file>` points test execution at a specific ast-grep root config.
- `--test-dir <dir>` narrows where test YAML files are discovered.
- `--snapshot-dir <dir>` changes the snapshot directory name from the default `__snapshots__`.
- `--filter <glob>` narrows which rule test cases run.
- `--skip-snapshot-tests` checks test validity without snapshot-output assertions. VT Code exposes this one on the public `workflow="test"` path.
- `--include-off` includes `severity: off` rules during test runs.
- `--update-all` generates or refreshes snapshot baselines, usually under `__snapshots__/`.
- `--interactive` is for selective snapshot updates after rule or test changes.
- Snapshot tests cover output details such as spans, labels, or message rendering in addition to simple valid/invalid matching, so `--skip-snapshot-tests` is useful while a rule is still evolving.

## Other Commands

- `ast-grep new [project|rule|test|util]` scaffolds a project or individual items. Common flags are `--lang`, `--yes`, `--base-dir`, and an optional item `NAME`.
- `ast-grep lsp` starts the language server and accepts an optional `--config <file>`.
- `ast-grep completions [shell]` generates shell completion scripts for `bash`, `elvish`, `fish`, `powershell`, or `zsh`.
- `ast-grep help` and `ast-grep --help` are the authoritative command-discovery entry points when the exact subcommand or flags are in doubt.

## CLI Modes

- `--interactive` is for reviewing rewrite hunks one-by-one; ast-grep’s interactive controls are `y`, `n`, `e`, and `q`.
- `--json=pretty|stream|compact` is for raw ast-grep JSON output when the user needs native ast-grep payloads or shell pipelines. `pretty` is the default if a style is not specified. Prefer VT Code’s normalized structural results when those are sufficient.
- Raw ast-grep JSON match objects include fields such as `text`, `range`, `file`, `lines`, optional `replacement`, optional `replacementOffsets`, and optional `metaVariables`. Scan-mode rule matches add fields like `ruleId`, `severity`, `message`, and optional `note`.
- ast-grep JSON positions are zero-based for line, column, and byte offsets. Keep that convention in mind when translating payloads into editor-facing or user-facing locations.
- `--json=stream` emits one JSON object per line and is the better fit for large pipelines; `pretty` and `compact` emit one JSON array and are easier to inspect but less streaming-friendly.
- `--json=<STYLE>` must use the equals-sign form. `--json stream` is parsed as plain `--json` plus an extra positional argument, not as `--json=stream`.
- `--stdin` is for piping code into ast-grep. It conflicts with `--interactive`.
- `ast-grep run --stdin` requires an explicit `--lang` because stdin has no file extension for language inference.
- `ast-grep scan --stdin` only works with one single rule via `--rule` / `-r`.
- `--stdin` only activates when the flag is present and ast-grep is not running in a TTY.
- `--heading=auto|always|never` only changes the human-readable text layout. It does not matter when VT Code is already consuming structured JSON.
- `--color=auto|always|ansi|never` only controls terminal coloring. VT Code’s public structural query forces plain output with `--color=never`.
- `--format=github|sarif` is for CI/reporting pipelines, not VT Code’s normalized public scan result shape.
- `--report-style=rich|medium|short` only changes ast-grep’s human-readable diagnostics.
- `--error`, `--warning`, `--info`, `--hint`, and `--off` override rule severities for one scan run. These flags belong on the CLI skill path, not VT Code’s public structural surface.
- `--inspect entity` is the direct CLI way to inspect each rule’s final enabled severity, including overrides and project-config effects.
- `unused-suppression` can also have its severity overridden on the CLI, but that is still CLI-only behavior outside VT Code’s public structural surface.
- `--inspect=summary|entity` emits file and rule discovery diagnostics to stderr without changing the actual match results.
- `--threads <NUM>` controls approximate parallelism. `0` keeps ast-grep’s default heuristics.
- `-C/--context` shows symmetric surrounding lines. `-A/--after` and `-B/--before` are asymmetric alternatives and conflict with `--context`.
- `ast-grep run` exits `0` when at least one match is found and `1` when no matches are found. VT Code normalizes that no-match case to an empty `matches` array on the public structural query path.
- `ast-grep scan` exits `1` when at least one error-severity rule matches and `0` when no rules match. VT Code normalizes that error-finding case to structured `findings` instead of surfacing a tool error.

## API Escalation

- Do not force complex transformations into rule syntax when the task needs arbitrary AST inspection or computed replacements.
- Escalate to ast-grep’s library API when the task needs conditional replacement logic, counting or ordering matched nodes, per-node patch generation, or replacement text computed from matched content and surrounding nodes.
- Node.js NAPI is the main experimental API surface today. The common entry points are `parse`, `kind`, and `pattern`, and the main objects are `SgRoot` and `SgNode`.
- In NAPI, `parse(Lang.<X>, source)` returns `SgRoot`, `root()` returns `SgNode`, and traversal/search APIs like `find`, `findAll`, `field`, `parent`, `children`, `matches`, `inside`, `has`, `replace`, and `commitEdits` live on `SgNode`.
- `NapiConfig` is the programmatic equivalent of rule YAML for `find` / `findAll`, and `FindConfig` is the config shape for file-based searching.
- Python bindings expose the same general model with `SgRoot(src, language)` plus `SgNode` methods for rule checks, traversal, searching, and edit generation.
- JS language-specific objects like `js.parse(...)` are deprecated; prefer the unified NAPI functions with `Lang.JavaScript`.
- Rust `ast_grep_core` is the lowest-level and most efficient option, but also the heaviest lift.
- Applying ast-grep `fix` through the JS/Python APIs is still experimental, so prefer generating explicit patches in code when reliability matters.
- If the target language has no suitable JS/Python parser path for the desired automation, prefer a Rust implementation or another repo-native AST approach instead of overcomplicating ast-grep rules.

## Use `unified_exec` For

- `ast-grep --help`
- `ast-grep new`
- `ast-grep new rule`
- `ast-grep scan -r <rule.yml> <path>`
- `ast-grep scan --rule <file> <path>`
- `ast-grep scan --inline-rules '...' <path>`
- `ast-grep run --pattern <pattern> --rewrite <rewrite>`
- `ast-grep run --json`
- `ast-grep run --stdin --lang <lang>`
- `ast-grep run --no-ignore hidden --follow`
- `ast-grep run --inspect summary --threads 4`
- `ast-grep run --context 2`
- `ast-grep scan --format sarif --report-style short`
- `ast-grep scan --error=rule-id`
- `ast-grep scan --stdin --rule <rule.yml>`
- `ast-grep test --config sgconfig.yml --filter 'rust/*'`
- `ast-grep test --test-dir rule-tests --snapshot-dir __snapshots__`
- `ast-grep test --include-off --update-all`
- `ast-grep lsp`
- `ast-grep completions`
- `ast-grep new project --base-dir . --yes`
- `ast-grep new rule my-rule --lang rust`
- `ast-grep help`
- ast-grep GitHub Action setup
- ast-grep programmatic API experiments and library examples
- System package-manager install commands when the user explicitly wants them
- `sg new`
- Rewrite or apply flows
- Interactive ast-grep flags
- Raw ast-grep color control such as `--color never`
- `transform`, `rewrite`, `joinBy`, or `rewriters`
- Non-trivial `sgconfig.yml` authoring or debugging
- Rule authoring tasks that need direct ast-grep CLI iteration beyond public scan/test

## Read More

- Read [references/project-workflows.md](references/project-workflows.md) when you need the boundary between public scan/test support and skill-driven CLI work, or when you need a quick reminder of ast-grep pattern and rule essentials.
