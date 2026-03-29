---
name: ast-grep
description: "Use for ast-grep setup and authoring: install, `ast-grep run`, `sg scan`, `sg test`, `sg new`, `sg new rule`, `ast-grep lsp`, scaffolding with `sgconfig.yml`, `rules/`, `rule-tests/`, `utils/`, `scan --rule`, `scan --inline-rules`, `--stdin`, `--json`, optional chaining, rule catalog, meta variables, object-style patterns, cheat sheets, `nthChild stopBy`, `range field`, config hints `metadata url` and `caseInsensitive glob`, FAQ `rule order`, `kind pattern`, `debug-query`, `static analysis`, tree-sitter parser, pattern yaml api, search rewrite lint analyze, core concepts `textual structural`, `ast cst`, `named unnamed`, `kind field`, `significant trivial`, deep dive `ambiguous pattern`, `effective selector`, `meta variable detection`, `lazy multi`, strictness `strictness smart ast relaxed signature cst`, `constraints`, `expandEnd`, `transform`, `rewriters`, `rewrite joinBy`, `find patch`, `barrel import`, `languageGlobs`, `expandoChar`, programmatic API."
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

## Rule Catalog

- Use the ast-grep catalog as inspiration when the user wants existing example rules, not as something to copy blindly.
- Start from examples in the same language family when possible.
- Read catalog markers as hints about rule complexity:
  - simple pattern examples are good starting points
  - `Fix` means the example includes a rewrite path
  - `constraints`, `labels`, `utils`, `transform`, and `rewriters` mean the example depends on more advanced rule features
- When adapting a catalog example, translate it to the current repository’s language, style, and safety constraints instead of preserving the example verbatim.
- Prefer the bundled skill workflow when the user asks to explain, adapt, or combine catalog examples.

## Rule Essentials

- Start rule files with `id`, `language`, and root `rule`.
- Treat the root `rule` as a rule object that matches one target AST node per result.
- Use atomic fields such as `pattern`, `kind`, and `regex` for direct node checks.
- Use relational fields such as `inside`, `has`, `follows`, and `precedes` when the match depends on surrounding nodes.
- Use composite fields such as `all`, `any`, `not`, and `matches` to combine sub-rules or reuse utility rules.
- Rule object fields are effectively unordered and conjunctive; if matching becomes order-sensitive, rewrite the logic with an explicit `all` sequence instead of assuming YAML key order matters.
- `language` controls how patterns parse. Syntax that is valid in one language can fail in another.

## Rule Cheat Sheet

- Atomic rules check properties of one node. Start here when a single syntax shape is enough.
- `pattern`, `kind`, and `regex` are the common atomic fields. Reach for `nthChild` when position among named siblings matters and `range` when the match must be limited to a known source span.
- Relational rules describe structure around the target node. Use `inside`, `has`, `follows`, and `precedes` when the match depends on ancestors, descendants, or neighboring nodes.
- Add relational `field` when the surrounding node matters by semantic role, not just by shape. Add `stopBy` when ancestor or sibling traversal must continue past the nearest boundary instead of stopping early.
- Composite rules combine checks for the same target node. Use `all` for explicit conjunction, `any` for alternatives, `not` for exclusions, and `matches` to delegate to a utility rule.
- Utility rules keep repeated logic out of the main rule body. Use file-local `utils` for one config file and global utility-rule files when multiple rules in the project need the same building block.
- Switch from a single `pattern` to a rule object when you need positional constraints, role-sensitive matching, reusable sub-rules, or several structural conditions on one node.

## Config Cheat Sheet

- Basic info keys define the rule itself. Use `id` for the unique rule name, `language` for the parser target, `url` for rule documentation, and `metadata` for custom project data that VT Code should preserve with the rule.
- Finding keys define what gets matched. `rule` is the core matcher, `constraints` narrows meta-variable captures, and `utils` holds reusable helper rules that you call through `matches`.
- Patching keys define reusable fixes. Use `transform` to derive new meta-variables before replacement, `fix` for either a string replacement or a `template` object with `expandStart` / `expandEnd`, and `rewriters` when the transformation is too complex for one inline `fix`.
- Linting keys define what scan results report. Use `severity`, `message`, `note`, and `labels` for diagnostics, then `files` and `ignores` to scope where the rule applies.
- `files` supports either plain globs or object entries. Use object syntax when you need options like `caseInsensitive` glob matching.
- Keep config authoring on the ast-grep skill path. VT Code’s public structural tool runs read-only query/scan/test workflows; it does not expose rule-YAML authoring fields directly.

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
- This is the right model for list-style rewrites such as exploding a barrel import into multiple single imports.
- Keep this declarative workflow on the ast-grep skill path. VT Code’s public structural surface stays read-only and does not expose rewrite/apply behavior.

## Rewrite Essentials

- Use `ast-grep run --pattern ... --rewrite ...` for one-off rewrites.
- Use YAML `fix` in rule files for reusable rewrites that should live with the rule.
- Use `--interactive` to review rewrite hunks before applying them.
- Use `--update-all` or `-U` only when the user clearly wants non-interactive apply behavior.
- Meta variables captured in `pattern` can be reused in `fix`.
- `fix` indentation is preserved relative to the matched source location, so multiline rewrites must be authored with deliberate indentation.
- Non-matched meta variables become empty strings in rewritten output.
- If appended uppercase text would be parsed as part of a meta variable name, use transforms instead of writing `$VARName` directly.
- Use `transform.rewrite` when a matched list must be rewritten element-by-element before the outer `fix` runs.
- Use `joinBy` to control how rewritten list items are stitched together, for example newline-joined imports in a barrel-import rewrite.
- Use `fix.template` plus `expandStart` / `expandEnd` when the rewrite must consume surrounding commas, brackets, or trivia outside the target node.
- Keep advanced `transform` and `rewriters` in the skill-driven CLI workflow.

## CLI Modes

- `--interactive` is for reviewing rewrite hunks one-by-one; ast-grep’s interactive controls are `y`, `n`, `e`, and `q`.
- `--json` is for raw ast-grep JSON output when the user needs native ast-grep payloads or shell pipelines. Prefer VT Code’s normalized structural results when those are sufficient.
- `--stdin` is for piping code into ast-grep. It conflicts with `--interactive`.
- `ast-grep run --stdin` requires an explicit `--lang` because stdin has no file extension for language inference.
- `ast-grep scan --stdin` only works with one single rule via `--rule` / `-r`.
- `--stdin` only activates when the flag is present and ast-grep is not running in a TTY.

## API Escalation

- Do not force complex transformations into rule syntax when the task needs arbitrary AST inspection or computed replacements.
- Escalate to ast-grep’s library API when the task needs conditional replacement logic, counting or ordering matched nodes, per-node patch generation, or replacement text computed from matched content and surrounding nodes.
- JavaScript is the most mature ast-grep binding.
- Python bindings exist and are useful for syntax-tree scripting.
- Rust `ast_grep_core` is the lowest-level and most efficient option, but also the heaviest lift.
- Applying ast-grep `fix` through the JS/Python APIs is still experimental, so prefer generating explicit patches in code when reliability matters.
- If the target language has no suitable JS/Python parser path for the desired automation, prefer a Rust implementation or another repo-native AST approach instead of overcomplicating ast-grep rules.

## Use `unified_exec` For

- `ast-grep --help`
- `ast-grep new`
- `ast-grep new rule`
- `ast-grep scan -r <rule.yml>`
- `ast-grep scan --rule <file>`
- `ast-grep scan --inline-rules '...'`
- `ast-grep run --pattern <pattern> --rewrite <rewrite>`
- `ast-grep run --json`
- `ast-grep run --stdin --lang <lang>`
- `ast-grep scan --stdin --rule <rule.yml>`
- `ast-grep lsp`
- `ast-grep completions`
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
