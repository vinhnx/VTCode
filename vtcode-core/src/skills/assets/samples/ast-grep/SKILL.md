---
name: ast-grep
description: Use when the task involves ast-grep quick start, rule catalog examples, and project workflows such as installing ast-grep, `ast-grep run`, `sg scan`, `sg test`, `sg new`, `sg new rule`, `ast-grep lsp`, optional chaining refactors, project scaffolding with `sgconfig.yml`, `rules/`, `rule-tests/`, or `utils/`, pattern syntax, meta variables, multi meta variables like `$$$ARGS`, non-capturing `$_`, unnamed-node `$$VAR`, object-style patterns, rule cheat sheets, advanced hints like `nthChild stopBy` and `range field`, `constraints`, `labels`, local/global utility rules, `utils`, `transform`, `rewriters`, `customLanguages`, `languageGlobs`, `languageInjections`, `scan --rule`, `scan --inline-rules`, `--rewrite`, YAML `fix`, `expandStart`, `expandEnd`, `template`, `--interactive`, `--update-all`, `--stdin`, `--json`, `scan -r`, shell `completions`, GitHub Action setup, ast-grep JS/Python/Rust programmatic API usage, `ast_grep_core`, `matches` utility rules, or ast-grep rule authoring, rewriting, and debugging.
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

## Pattern Syntax

- Pattern code must be valid code that tree-sitter can parse.
- Patterns match syntax trees, so a query can match nested expressions instead of only top-level text.
- `$VAR` matches one named AST node.
- `$$$ARGS` matches zero or more AST nodes in places like arguments, parameters, or statements.
- Reusing the same captured name means both occurrences must match the same syntax.
- Prefixing a meta variable with `_` disables capture, so repeated `$_X` occurrences do not need to match the same content.
- `$$VAR` captures unnamed nodes when named-node matching is too narrow.
- If a short snippet is ambiguous, move to an object-style pattern with more `context` plus `selector` instead of guessing.

## Rewrite Essentials

- Use `ast-grep run --pattern ... --rewrite ...` for one-off rewrites.
- Use YAML `fix` in rule files for reusable rewrites that should live with the rule.
- Use `--interactive` to review rewrite hunks before applying them.
- Use `--update-all` or `-U` only when the user clearly wants non-interactive apply behavior.
- Meta variables captured in `pattern` can be reused in `fix`.
- `fix` indentation is preserved relative to the matched source location, so multiline rewrites must be authored with deliberate indentation.
- Non-matched meta variables become empty strings in rewritten output.
- If appended uppercase text would be parsed as part of a meta variable name, use transforms instead of writing `$VARName` directly.
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
- `transform` or `rewriters`
- Non-trivial `sgconfig.yml` authoring or debugging
- Rule authoring tasks that need direct ast-grep CLI iteration beyond public scan/test

## Read More

- Read [references/project-workflows.md](references/project-workflows.md) when you need the boundary between public scan/test support and skill-driven CLI work, or when you need a quick reminder of ast-grep pattern and rule essentials.
