# FAQ Gotchas

Use this reference when a pattern or rule seems reasonable but ast-grep still behaves unexpectedly.

## Pattern must be valid code

Patterns are parsed by tree-sitter. If the snippet is not valid code for that language, ast-grep cannot interpret it the way you intend.

Bad partial snippet:

```yaml
pattern: '"key": "$VAL"'
```

Better:

```yaml
rule:
  pattern:
    context: '{"key": "$VAL"}'
    selector: pair
```

Use `context` plus `selector` when the real target is a sub-expression, field, pair, or other fragment that is not valid standalone code.

## CLI and Playground can disagree

Common causes:

- different parser versions
- different text encoding during parser recovery
- incomplete or ambiguous pattern code

Use `--debug-query` in the CLI and compare the parsed node shape before changing the rule.

## Metavariables only match AST nodes

- Metavariable names must start with `$` and use uppercase letters, digits, or `_`.
- A metavariable normally matches one named AST node.
- Use `$$UNNAMED` when you need unnamed nodes.
- Do not mix a metavariable with prefix or suffix text inside one node. `use$HOOK` and `io_uring_$FUNC` are invalid.

## Multiple metavariables are lazy

`$$$ARGS` style captures stop before the next node that can satisfy the remainder of the pattern. Keep this in mind before assuming greedy behavior.

## Prefix or suffix matching needs constraints

For naming-convention searches, switch to YAML rules with regex constraints.

```yaml
rule:
  pattern: $HOOK($$$ARGS)
constraints:
  HOOK:
    regex: '^use'
```

## Separate languages unless one is a true superset

ast-grep does not support one rule that cleanly spans multiple languages with different ASTs.

Use one of these:

- parse everything as the superset language with `languageGlobs`
- write separate rules per language

## Rule objects are unordered

If a later relational rule depends on a metavariable captured earlier, do not rely on dictionary order. Wrap the sequence in `all` so the evaluation order is explicit.

## `kind` plus `pattern` is often the wrong shape

If the parser reads your pattern as the wrong AST node, adding a separate `kind` rule will not fix it. Use a pattern object with `context` and `selector` so the parser sees an unambiguous full snippet.

## Not a static-analysis engine

ast-grep does not provide:

- scope analysis
- type information
- control-flow analysis
- data-flow analysis
- taint analysis

Use it for syntax-aware matching, not semantic guarantees.
