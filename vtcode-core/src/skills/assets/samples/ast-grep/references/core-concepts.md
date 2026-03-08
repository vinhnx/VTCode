# Core Concepts

Use this reference when a pattern seems close but ast-grep behavior still feels surprising.

## Structural, not textual

ast-grep matches parsed code structure, not raw text. The pattern itself must be valid code for the selected language.

Use regex or grep when the task is really about:

- plain text fragments
- file names
- logs or stack traces
- substring-only naming checks without syntax context

Combine structural and textual matching by using YAML constraints with regex when syntax plus text both matter.

## CST, not simplified AST

ast-grep works on Tree-sitter CST.

That means:

- operators can matter
- punctuation can matter
- modifiers can matter
- smart matching can skip trivial nodes when safe, but not all syntax details disappear

If a token like `+`, `get`, `static`, or `async` changes the meaning, make it explicit in the pattern or rule.

## Named vs unnamed nodes

`$VAR` matches named nodes by default.

Examples:

- `return $A` matches `return 123`
- `return $A` does not match `return;`
- `return $$A` can match both because `$$A` includes unnamed nodes

Use `$$VAR` when punctuation or another unnamed node must participate in the match.

## Kind vs field

- `kind` describes the node itself
- `field` describes the node's role relative to its parent

When multiple children have the same `kind`, `field` is often what makes the rule precise.

Example shape:

```yaml
rule:
  kind: pair
  has:
    field: key
    kind: string
```

Use `has` or `inside` with `field` when the question is "which role does this node play?"

## Significant vs trivial nodes

Named nodes and fielded nodes are significant, but some important syntax is still carried by trivial nodes.

Example:

- `class A { get method() {} }`
- `class A { method() {} }`

If the distinction matters, spell out the modifier instead of relying on broad method patterns.

## Practical rule of thumb

When a match is wrong, ask:

1. Is this really structural or actually textual?
2. Is the pattern valid code?
3. Does the missing distinction live in an unnamed token?
4. Do I need `field` because the same kind appears in multiple roles?
5. Do I need a rule instead of a raw pattern?
