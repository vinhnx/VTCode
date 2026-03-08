# Rewrite And Transform

Use this path only when the user explicitly wants code changes or a codemod workflow.

## VT Code boundary

`unified_search` with `action="structural"` stays read-only.

For rewrite work, use ast-grep through shell commands plus normal VT Code edit/review flow. Do not silently jump from search to rewrite.

## Start with the safest rewrite loop

1. Prove the match first with `sg run` or `sg scan --inline-rules`
2. Add `fix` only after the match is stable
3. Review the rewritten output before broad apply
4. Use `sg test` for reusable rules
5. Apply interactively before `-U` unless the task clearly wants bulk codemod execution

One-shot rewrite example:

```bash
sg run -p 'console.log($$$ARGS)' -r 'logger.info($$$ARGS)' --lang ts src
```

Reusable rule example:

```yaml
id: no-console
language: ts
rule:
  pattern: console.log($$$ARGS)
fix: logger.info($$$ARGS)
```

```bash
sg scan --rule rules/no-console.yml src
```

## `constraints`, `transform`, and `fix`

- `constraints` filter already-matched single metavariables after the main `rule`
- `transform` creates new variables from existing metavariables
- `fix` uses matched or transformed variables to produce replacement text

Important constraint rule:

- constraints only apply to single metavariables like `$ARG`
- constraints do not directly target `$$$ARGS`

## `transform` defaults

Prefer the smallest transformation that solves the text-generation problem:

- `replace` for regex-based substitution
- `substring` for trimming wrapper characters
- `convert` for case changes
- `rewrite` for AST-aware subrewrites through rewriters

Object style:

```yaml
transform:
  NEW_NAME:
    replace:
      source: $OLD_NAME
      replace: debug(?<TAIL>.*)
      by: release$TAIL
```

String style on ast-grep 0.38.3+:

```yaml
transform:
  NEW_NAME: replace($OLD_NAME, replace='debug(?<TAIL>.*)', by='release$TAIL')
```

## Use `rewriters` for nested or list rewrites

Reach for `rewriters` when simple `fix` cannot reshape nested nodes or lists cleanly.

Common signs:

- you need to rewrite each identifier inside `$$$ITEMS`
- you need different rewrite logic for a captured subtree
- you need `joinBy` to rebuild a list of rewritten nodes

Minimal pattern:

```yaml
rewriters:
  - id: rewrite-ident
    rule:
      kind: identifier
    fix: import $MATCH from './barrel/$MATCH'
transform:
  IMPORTS:
    rewrite:
      rewriters: [rewrite-ident]
      source: $$$ITEMS
      joinBy: "\n"
fix: $IMPORTS
```

## Safety defaults

- Prefer interactive application over `-U` unless bulk apply is explicitly wanted
- Keep rewrites deterministic and testable
- If the transformation logic becomes hard to reason about, simplify the rule or fall back to normal code edits
