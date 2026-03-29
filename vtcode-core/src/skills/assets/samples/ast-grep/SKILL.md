---
name: ast-grep
description: Use when the task involves ast-grep project workflows such as `ast-grep run`, `sg scan`, `sg test`, `sg new`, `sg new rule`, `ast-grep lsp`, project scaffolding with `sgconfig.yml`, `rules/`, `rule-tests/`, or `utils/`, `customLanguages`, `languageGlobs`, `languageInjections`, `transform`, `rewriters`, `scan --rule`, `scan --inline-rules`, `--rewrite`, YAML `fix`, `expandStart`, `expandEnd`, `template`, `--interactive`, `--update-all`, `--stdin`, `--json`, `scan -r`, shell `completions`, GitHub Action setup, ast-grep JavaScript/Python/Rust programmatic API usage, `ast_grep_core`, `matches` utility rules, or ast-grep rule authoring, rewriting, and debugging.
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

## Rule Essentials

- Start rule files with `id`, `language`, and root `rule`.
- Treat the root `rule` as a rule object that matches one target AST node per result.
- Use atomic fields such as `pattern`, `kind`, and `regex` for direct node checks.
- Use relational fields such as `inside`, `has`, `follows`, and `precedes` when the match depends on surrounding nodes.
- Use composite fields such as `all`, `any`, `not`, and `matches` to combine sub-rules or reuse utility rules.
- Rule object fields are effectively unordered and conjunctive; if matching becomes order-sensitive, rewrite the logic with an explicit `all` sequence instead of assuming YAML key order matters.
- `language` controls how patterns parse. Syntax that is valid in one language can fail in another.

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
- `sg new`
- Rewrite or apply flows
- Interactive ast-grep flags
- Raw ast-grep color control such as `--color never`
- `transform` or `rewriters`
- Non-trivial `sgconfig.yml` authoring or debugging
- Rule authoring tasks that need direct ast-grep CLI iteration beyond public scan/test

## Read More

- Read [references/project-workflows.md](references/project-workflows.md) when you need the boundary between public scan/test support and skill-driven CLI work, or when you need a quick reminder of ast-grep rule object essentials.
