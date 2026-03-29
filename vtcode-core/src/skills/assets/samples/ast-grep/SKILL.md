---
name: ast-grep
description: "Use for ast-grep: ast-grep run, sg scan, sg test, sg new, new rule, sgconfig.yml, inline-rules, stdin, json, optional chaining, rule catalog, meta variables, pattern objects, nthChild stopBy, range field, metadata url, caseInsensitive glob, severity off, include metadata, rule order, kind pattern, positive rule, kind esquery, debug-query, static analysis, tree-sitter parser, pattern yaml api, search rewrite lint analyze, textual structural, ast cst, named unnamed, kind field, ambiguous pattern, effective selector, meta variable detection, lazy multi, strictness smart, relaxed signature, string fix, fix config, expandEnd, replace substring, toCase separatedBy, rewriter, rewrite joinBy, find patch, barrel import, ruleDirs testConfigs, libraryPath languageSymbol, dynamic injected, custom language, TREE_SITTER_LIBDIR, language injection, styled components, language alias, languageGlobs, expandoChar, napi parse, python api, programmatic API."
metadata:
    short-description: Ast-grep project workflows
---

# Ast-Grep

Use this skill for ast-grep project setup, rule authoring, rule debugging, and CLI workflows that go beyond a single structural query.

## Routing

- Prefer `unified_search` with `action="structural"` and `workflow="scan"` for read-only project scans.
- Prefer `unified_search` with `action="structural"` and `workflow="test"` for read-only ast-grep rule tests.
- Prefer structural `debug_query` on the public tool surface before falling back to raw `ast-grep run --debug-query`.
- Stay on the public structural surface first when the task is only running project checks and reporting findings.
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

- ast-grep ships many built-in languages. Common aliases include `bash`, `c`, `cc` / `cpp`, `cs`, `css`, `ex`, `go` / `golang`, `html`, `java`, `js` / `javascript` / `jsx`, `json`, `kt`, `lua`, `php`, `py` / `python`, `rb`, `rs` / `rust`, `swift`, `ts` / `typescript`, `tsx`, and `yml`.
- `--lang <alias>` and YAML `language: <alias>` use those built-in aliases. File-system scans infer language from built-in extensions unless the project overrides them.
- In VT Code, public structural `lang` is passed through to ast-grep. VT Code also normalizes and infers a local subset it can pre-parse itself: Rust, Python, JavaScript, TypeScript, TSX, Go, and Java.
- That local subset includes common ast-grep aliases and extensions such as `golang`, `jsx`, `cjs`, `mjs`, `cts`, `mts`, `py3`, and `pyi`.
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

## Rust Catalog Highlights

- Avoid duplicated exports: a Rust lint-style rule can detect `pub use foo::Bar;` in the same source file that already exposes `pub mod foo;`. Treat this as API-surface cleanup, not a mechanical rewrite.
- Beware `chars().enumerate()`: the Rust catalog rewrite from `$A.chars().enumerate()` to `$A.char_indices()` is valid when the code needs byte offsets instead of character indexes.
- Count `usize` digits without allocation: the catalog rewrite from `$NUM.to_string().chars().count()` to `$NUM.checked_ilog10().unwrap_or(0) + 1` is a good Rust-specific performance cleanup when the target is known to be an integer digit count.
- Unsafe function without unsafe block: the Rust catalog’s `function_item` rule that requires `unsafe` modifiers but rejects bodies containing `unsafe_block` is a good review rule for redundant `unsafe` markers. It is diagnostic-oriented and should usually stay a scan rule, not an automatic rewrite.
- Rewrite `indoc!` macro: the catalog example that removes `indoc! { r#"..."# }` wrappers is a rewrite-oriented example. Keep it on the CLI skill path because the replacement is formatting-sensitive and should be reviewed interactively before broad apply.
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
- Unnecessary `useState<T>` primitives: good cleanup rewrite for `useState<string|number|boolean>($A)` when the initializer already gives TypeScript enough information to infer the state type.
- Avoid `&&` short-circuit in JSX: good React-facing rewrite from `{cond && <View />}` to `{cond ? <View /> : null}` when the left side can evaluate to renderable falsy values like `0`.
- Rewrite MobX component style: useful migration example when `observer(() => ...)` hides React hook linting from tooling. Keep it on the CLI skill path because naming, export shape, and component conventions vary by repository.
- Avoid unnecessary React hooks: good diagnostic rule for `use*` functions that do not actually call hooks. Treat it as a review rule first, because renaming or de-hooking can be API-affecting.
- Reverse React Compiler: clearly rewrite-oriented and intentionally opinionated. Keep it on the CLI skill path and only use it when the user explicitly wants that de-memoization behavior.
- Avoid nested links: good accessibility and correctness scan rule for JSX trees.
- Rename SVG attributes: strong TSX rewrite example for hyphenated SVG attribute names such as `stroke-linecap` to `strokeLinecap`. Keep it reviewable because generated markup can be formatting-sensitive.
- Adapt TSX catalog rules to the repository’s React version, JSX runtime, lint rules, framework conventions, and browser-support target before using them directly.

## YAML Catalog Highlights

- YAML scan rules are useful for configuration-policy checks where the repository needs to flag specific keys or values rather than rewrite source code.
- The catalog host/port example is a simple message-oriented rule that matches either `host: $HOST` or `port: $PORT` and attaches a diagnostic. Treat it as a starting point for config validation, not a complete policy by itself.
- For YAML rules, be explicit about whether the repository cares about the key name, the value, or both. If both matter together, move from separate `any` patterns to a more structured rule before relying on the result.
- Keep YAML config checks repository-specific. Hard-coded values like `8000` are only useful when they reflect an actual project policy.

## Ruby Catalog Highlights

- Rails `*_filter` to `*_action`: useful migration rewrite for older Rails controllers. Keep it on the CLI skill path because framework version, controller style, and review expectations vary by repository.
- Prefer symbol over proc: good Ruby cleanup rewrite for cases like `.select { |v| v.even? }` to `.select(&:even?)`, but only where the shorthand remains readable and matches local Ruby style.
- Path traversal detection in Rails: good security-oriented scan rule for `Rails.root.join`, `File.join`, or `send_file` fed by variables. Treat it as a review hint, not a proof of exploitability, because the surrounding validation path still matters.
- Adapt Ruby catalog rules to the repository’s Rails version, Ruby style guide, and security posture before using them directly.

## Python Catalog Highlights

- OpenAI SDK migration: useful multi-rule migration example for legacy `openai` Python client code, but keep it on the CLI skill path because imports, client lifetime, response shapes, and surrounding application logic often need repository-specific review.
- Prefer generator expressions: good example of narrowing a rewrite to contexts like `any(...)`, `all(...)`, or `sum(...)` where generator expressions are clearly valid. Do not generalize it to every list comprehension.
- Walrus operator in `if` statements: useful paired-rule rewrite example, but only apply it where the repository targets Python 3.8+ and the style guide accepts assignment expressions.
- Remove async function: strong `rewriters` example for stripping `async` and inner `await`, but treat it as high-risk migration work because it changes call semantics and often requires broader control-flow review.
- Pytest fixture refactors: good example of `utils`-driven context matching for fixture rename or type-hint updates. Keep it tied to real pytest usage so similarly named non-test code is not swept in.
- `Optional[T]` to `T | None` and recursive union rewrites: useful typing-modernization examples, but only where the repository targets Python 3.10+ and static typing policy actually prefers PEP 604 unions.
- SQLAlchemy `mapped_column` to annotated `Mapped[...]`: useful ORM migration example, but keep it on the CLI skill path because ORM version, model style, and nullable semantics need review.
- Adapt Python catalog rules to the repository’s Python version floor, framework stack, typing policy, async model, and migration scope before using them directly.

## Kotlin Catalog Highlights

- Clean-architecture import checks: good scan-rule example for enforcing architectural boundaries with `files` plus import-path constraints. Treat it as repository-policy enforcement rather than a universal Kotlin rule.
- The Kotlin catalog example is diagnostic-oriented, not rewrite-oriented. Keep it on the scan path because import-boundary violations usually need design review instead of blind mutation.
- File-scoped package constraints are the point of the example: adapt the `files` glob and package regexes to the repository’s actual module layout before relying on the result.
- Adapt Kotlin catalog rules to the repository’s package naming, architecture boundaries, Android-vs-server structure, and lint ownership before using them directly.

## Java Catalog Highlights

- Unused local variable detection: useful educational example for `has` plus ordered `all` plus `precedes`, but prefer the project’s established linter or IDE for real unused-variable enforcement because Java variable scopes are broader than the sample rule covers.
- Field declarations of type `String`: good structural scan example showing why `field_declaration` plus `has: { field: type }` is more robust than a naive pattern when modifiers and annotations are present.
- The Java catalog examples are primarily search/diagnostic material, not high-confidence autofix rules. Keep them review-oriented unless the repository explicitly wants ast-grep-based cleanup instead of compiler or linter diagnostics.
- Adapt Java catalog rules to the repository’s package conventions, annotation usage, style tooling, and existing static-analysis stack before using them directly.

## HTML Catalog Highlights

- HTML parser reuse for framework templates: useful when Vue, Svelte, Astro, or similar files are mostly HTML, but keep parser caveats in mind because framework-specific control flow or frontmatter may require a custom language instead.
- Ant Design Vue `visible` to `open`: good framework-specific attribute rewrite using enclosing-tag checks plus constraints. Keep it on the CLI skill path because framework version and component set must be confirmed first.
- i18n key extraction: useful template rewrite example for wrapping static text while skipping mustache expressions, but keep it reviewable because real projects usually need key naming, dictionary updates, and whitespace policy beyond the raw rewrite.
- Adapt HTML catalog rules to the repository’s template framework, parser limitations, i18n workflow, and component-library version before using them directly.

## Go Catalog Highlights

- Problematic `defer` with nested function calls: strong Go-specific scan example for catching cases where deferred arguments are evaluated immediately. Treat it as a correctness and test-reliability review rule, and prefer a manual rewrite to a closure-wrapped `defer func() { ... }()` when the repository agrees with that style.
- Function declarations by name pattern: good example of using `kind` plus `has` plus `regex` when a meta-variable pattern cannot express the naming constraint directly, such as `Test.*`.
- Contextual matching for function calls: good illustration of Go parser ambiguity between function calls and type conversions. Use contextual patterns with `selector: call_expression` when naive call patterns do not behave as expected.
- Package-import detection: useful search/scan rule for dependency auditing, compliance checks, or migration prep. Adapt the import regex to the repository’s actual dependency boundaries instead of hard-coding example packages.
- Problematic JSON tags with `-,`: high-signal security-oriented scan rule for Go struct tags. Treat matches as actionable review items because the example represents a real unmarshaling footgun, not just a style preference.
- Adapt Go catalog rules to the repository’s Go version, test conventions, package layout, security posture, and existing static-analysis tooling before using them directly.

## Cpp Catalog Highlights

- Reuse Cpp rules for C only when the repository intentionally parses C sources as Cpp via `languageGlobs`; do not assume mixed C/C++ projects want that parser tradeoff by default.
- Format-string vulnerability rewrite: strong security-oriented example for `fprintf`/`sprintf`-style calls missing an explicit format string. Keep it reviewable because real C/C++ codebases may prefer safer API migrations over mechanical `"%s"` insertion in some contexts.
- Struct inheritance matching: useful example of AST-shaped pattern authoring in C++, especially when a shorter surface pattern produces an `ERROR` node. Use the full structural form or fall back to a YAML rule when the syntax is too incomplete to parse reliably.
- Adapt Cpp catalog rules to the repository’s C-vs-C++ parser choice, security posture, libc usage, and coding-standard expectations before using them directly.

## C Catalog Highlights

- Parsing C as Cpp can reduce duplicated rule authoring, but only use that route when the repository intentionally opts into the parser tradeoff with `languageGlobs`; do not blur C and C++ semantics by default.
- Match function calls in C with contextual patterns: good example of tree-sitter-c ambiguity around fragments like `test($A)`. Use `context` plus `selector: call_expression` when a plain call pattern under-parses or turns into `macro_type_specifier`.
- Rewrite method-style calls to function calls: useful migration example for C codebases that emulate methods with structs or function pointers, but keep it on the CLI skill path because it changes calling conventions and may affect ownership, pointer semantics, or naming policy.
- Yoda-condition rewrite: clearly style-driven and repository-policy-sensitive. Treat it as optional rewrite material only where the project explicitly prefers constant-on-the-left comparisons.
- Adapt C catalog rules to the repository’s parser choice, macro usage, pointer conventions, coding style, and safety policy before using them directly.

## Rule Essentials

- Start rule files with `id`, `language`, and root `rule`.
- Treat the root `rule` as a rule object that matches one target AST node per result.
- A rule object still needs a positive anchor. In practice, start with `pattern` or `kind`; `regex` is a filter, not a sufficient root rule by itself.
- Use atomic fields such as `pattern`, `kind`, and `regex` for direct node checks.
- Use relational fields such as `inside`, `has`, `follows`, and `precedes` when the match depends on surrounding nodes.
- Use composite fields such as `all`, `any`, `not`, and `matches` to combine sub-rules or reuse utility rules.
- Rule object fields are effectively unordered and conjunctive; if matching becomes order-sensitive, rewrite the logic with an explicit `all` sequence instead of assuming YAML key order matters.
- `language` controls how patterns parse. Syntax that is valid in one language can fail in another.

## Rule Cheat Sheet

- Atomic rules check properties of one node. Start here when a single syntax shape is enough.
- `pattern`, `kind`, and `regex` are the common atomic fields. `pattern` can also be an object with `context`, `selector`, and optional `strictness`.
- `kind` is usually a plain node kind name, but ast-grep also supports a limited ESQuery-style syntax for some `kind` selectors.
- `regex` matches the whole node text. Reach for `nthChild` when position among named siblings matters and `range` when the match must be limited to a known source span.
- `nthChild` accepts a number, an `An+B` string, or an object with `position`, `reverse`, and `ofRule`. Counting is 1-based and only considers named siblings.
- Relational rules describe structure around the target node. Use `inside`, `has`, `follows`, and `precedes` when the match depends on ancestors, descendants, or neighboring nodes.
- Add relational `field` when the surrounding node matters by semantic role, not just by shape. `field` only applies to `inside` and `has`.
- Add `stopBy` when ancestor or sibling traversal must continue past the nearest boundary instead of stopping early. The default is `neighbor`, `end` searches to the boundary, and a rule object stop is inclusive.
- Composite rules combine checks for the same target node. Use `all` for explicit conjunction, `any` for alternatives, `not` for exclusions, and `matches` to delegate to a utility rule.
- `all` and `any` still operate on one target node. They combine sub-rules, not multiple matched nodes.
- Utility rules keep repeated logic out of the main rule body. Use file-local `utils` for one config file and global utility-rule files when multiple rules in the project need the same building block.
- Switch from a single `pattern` to a rule object when you need positional constraints, role-sensitive matching, reusable sub-rules, or several structural conditions on one node.

## Config Cheat Sheet

- Basic info keys define the rule itself. Use `id` for the unique rule name, `language` for the parser target, `url` for rule documentation, and `metadata` for custom project data that VT Code should preserve with the rule.
- One YAML file can hold multiple rules when you separate documents with `---`.
- Finding keys define what gets matched. `rule` is the core matcher, `constraints` narrows meta-variable captures, and `utils` holds reusable helper rules that you call through `matches`.
- `constraints` runs after `rule` matched, only targets single meta variables like `$ARG`, and is a poor fit inside `not`.
- Patching keys define reusable fixes. Use `transform` to derive new meta-variables before replacement, `fix` for either a string replacement or a `template` object with `expandStart` / `expandEnd`, and `rewriters` when the transformation is too complex for one inline `fix`.
- Linting keys define what scan results report. Use `severity`, `message`, `note`, and `labels` for diagnostics, then `files` and `ignores` to scope where the rule applies.
- `severity: off` disables the rule during scanning. `note` supports Markdown but cannot interpolate meta variables.
- `labels` keys must come from meta variables already defined by the rule or `constraints`.
- `files` supports either plain globs or object entries. Use object syntax when you need options like `caseInsensitive` glob matching.
- `ignores` runs before `files`. Both are relative to the `sgconfig.yml` directory, and the glob should not start with `./`.
- Rule-level `ignores` is different from CLI `--no-ignore`: the CLI flag changes global ignore-file behavior, while YAML `ignores` only filters files for that rule.
- JSON output only includes rule `metadata` when the ast-grep run enabled metadata output, for example via `--include-metadata`.
- Keep config authoring on the ast-grep skill path. VT Code’s public structural tool runs read-only query/scan/test workflows; it does not expose rule-YAML authoring fields directly.

## Transformation Objects

- `transform` builds new strings from captured meta variables before `fix` runs.
- `replace` uses a Rust regex over one meta variable. `source` must be `$VAR` style, `replace` is the regex, `by` is the replacement text, and regex capture groups can be reused in `by`.
- `substring` slices a meta variable by character index with inclusive `startChar` and exclusive `endChar`. Negative indexes count from the end, and slicing is based on Unicode characters rather than raw bytes.
- `substring` behaves like Python string slicing, so omit either bound when the slice should stay open-ended.
- `convert` changes identifier-style casing through `toCase`. Common outputs are `lowerCase`, `upperCase`, `capitalize`, `camelCase`, `snakeCase`, `kebabCase`, and `pascalCase`.
- Use `separatedBy` to control how `convert` splits words before rebuilding the target case. Supported separators include dash, dot, space, slash, underscore, and `CaseChange`.
- `CaseChange` splits at transitions such as `astGrep`, `ASTGrep`, or `XMLHttpRequest`, which matters when converting mixed acronym identifiers.
- String-form transforms such as `replace(...)`, `substring(...)`, `convert(...)`, and `rewrite(...)` are valid shorthand in newer ast-grep versions, but keep examples explicit when debugging.

## Rewriters

- `rewriters` is an experimental feature for advanced multi-node rewrites. Prefer ast-grep’s API instead when the YAML starts carrying too much control flow or state.
- A rewriter is a smaller rule object with only `id`, `rule`, `constraints`, `transform`, `utils`, and `fix`. `id`, `rule`, and `fix` are required.
- Rewriters are only usable through the `rewrite` transformation. They are not standalone scan/report rules.
- Meta variables captured inside one rewriter do not leak to sibling rewriters or the outer rule.
- Rewriter-local `transform` variables and `utils` are also scoped to that one rewriter.
- A rewriter transform can call other rewriters from the same `rewriters` list when the rewrite pipeline needs multiple internal passes.

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
- Typical patterns are `styled.$TAG\`$CONTENT\`` for CSS-in-JS and `graphql\`$CONTENT\`` for GraphQL template literals.
- ast-grep parses the extracted subregion with the injected language, not the parent document language. That is why CSS patterns can match inside JavaScript once injection is configured.
- Use `languageGlobs` when the whole file should be parsed as a different or superset language. Use `languageInjections` when only a nested region inside the file changes language.
- In VT Code, read-only structural query / scan / test can consume existing injection config. Designing or debugging `languageInjections` itself stays on the bundled ast-grep skill path.

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
- This is the right model for list-style rewrites such as exploding a barrel import into multiple single imports, and it is the canonical example for rewriter usage.
- Keep this declarative workflow on the ast-grep skill path. VT Code’s public structural surface stays read-only and does not expose rewrite/apply behavior.

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
- `--config <file>` points test execution at a specific ast-grep root config.
- `--test-dir <dir>` narrows where test YAML files are discovered.
- `--snapshot-dir <dir>` changes the snapshot directory name from the default `__snapshots__`.
- `--filter <glob>` narrows which rule test cases run.
- `--skip-snapshot-tests` checks test validity without snapshot-output assertions. VT Code exposes this one on the public `workflow="test"` path.
- `--include-off` includes `severity: off` rules during test runs.
- `--update-all` and `--interactive` are snapshot-maintenance flows and stay on the CLI skill path.

## Other Commands

- `ast-grep new [project|rule|test|util]` scaffolds a project or individual items. Common flags are `--lang`, `--yes`, `--base-dir`, and an optional item `NAME`.
- `ast-grep lsp` starts the language server and accepts an optional `--config <file>`.
- `ast-grep completions [shell]` generates shell completion scripts for `bash`, `elvish`, `fish`, `powershell`, or `zsh`.
- `ast-grep help` and `ast-grep --help` are the authoritative command-discovery entry points when the exact subcommand or flags are in doubt.

## CLI Modes

- `--interactive` is for reviewing rewrite hunks one-by-one; ast-grep’s interactive controls are `y`, `n`, `e`, and `q`.
- `--json=pretty|stream|compact` is for raw ast-grep JSON output when the user needs native ast-grep payloads or shell pipelines. `pretty` is the default if a style is not specified. Prefer VT Code’s normalized structural results when those are sufficient.
- `--stdin` is for piping code into ast-grep. It conflicts with `--interactive`.
- `ast-grep run --stdin` requires an explicit `--lang` because stdin has no file extension for language inference.
- `ast-grep scan --stdin` only works with one single rule via `--rule` / `-r`.
- `--stdin` only activates when the flag is present and ast-grep is not running in a TTY.
- `--heading=auto|always|never` only changes the human-readable text layout. It does not matter when VT Code is already consuming structured JSON.
- `--color=auto|always|ansi|never` only controls terminal coloring. VT Code’s public structural query forces plain output with `--color=never`.
- `--format=github|sarif` is for CI/reporting pipelines, not VT Code’s normalized public scan result shape.
- `--report-style=rich|medium|short` only changes ast-grep’s human-readable diagnostics.
- `--error`, `--warning`, `--info`, `--hint`, and `--off` override rule severities for one scan run. These flags belong on the CLI skill path, not VT Code’s public structural surface.
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
