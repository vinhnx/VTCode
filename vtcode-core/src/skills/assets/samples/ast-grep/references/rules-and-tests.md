# Rules And Tests

Use this path when the work should survive past one command.

## Escalate from pattern to rule when

- the query needs multiple constraints or relational logic
- the user wants a reusable check or codemod
- you need repeatable verification
- one-off `sg run` patterns keep changing across examples

## Start inline, then save

Prototype with `--inline-rules` before writing files:

```bash
sg scan --inline-rules '
id: suspicious-console
language: <language>
rule:
  pattern: console.log($$$ARGS)
message: Review console logging
severity: warning
' src
```

When the rule stabilizes, move it into a YAML file and run:

```bash
sg scan --rule path/to/rule.yml src
```

You can test multiple temporary rules by separating them with `---` inside `--inline-rules`.

## Rule-authoring defaults

- Keep the first rule minimal. Add one constraint at a time.
- Preserve a small set of positive and negative examples beside the rule while iterating.
- Do not add rewrite fields unless the user explicitly wants changes.
- If matching is off, go back to `sg run --debug-query` instead of piling on YAML.
- Remember that `constraints` run after the main `rule`, and only on single metavariables like `$ARG`, not `$$$ARGS`.
- Use `kind` when the node type is the important fact.
- Use `field` with `has` or `inside` when the node role relative to its parent is the important fact.
- Use `$$VAR` when unnamed tokens must participate in the rule.
- Use local `utils` for repeated helper logic inside one rule file.
- Move shared helpers into global utility rules via `utilDirs` when multiple rule files need the same helper.
- If a trivial token like `get` or an operator changes the meaning, spell it out in the pattern or relational rule instead of assuming named nodes capture it.
- If relational rules depend on captured metavariables, force the evaluation order with `all` instead of depending on object key order.
- Do not try to hide TS/JS or C/C++ differences inside one rule unless one language can safely be parsed as the superset via `languageGlobs`.

## Verify with `sg test`

Use `sg test` before trusting the rule on a real codebase:

```bash
sg test -t path/to/tests
sg test -t path/to/tests --filter suspicious-console
sg test -t path/to/tests --skip-snapshot-tests
sg test -t path/to/tests -U
```

Guidance:

- Start with the smallest test directory that covers the rule.
- Use `--filter` to isolate a single rule while iterating.
- Use `--skip-snapshot-tests` when validating parseability first.
- Use `-U` only when snapshot changes are intentional.
- After tests pass, rerun `sg scan --rule ...` on the real target path.
- If no scaffold exists yet, create it with `sg new project|rule|test|util` instead of inventing directory layout by hand.
