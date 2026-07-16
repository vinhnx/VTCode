# Ast-Grep Project Workflows

Use `exec_command` for specialised ast-grep work, including patterns, project
workflows, rule debugging, configuration inspection, and rewrite previews.

On Linux, prefer the `ast-grep` executable name because `sg` can refer to the
system `setgroups` command. Quote patterns containing metavariables such as
`$NAME` with single quotes so the shell does not expand them.

## Quick Checks

Confirm the installed CLI and discover version-specific flags before relying on
an example:

```sh
ast-grep --version
ast-grep --help
ast-grep run --help
ast-grep scan --help
ast-grep test --help
```

VT Code can install its managed dependency with:

```sh
vtcode dependencies install ast-grep
```

## Ad-Hoc Structural Search

Use `run` for a single language and pattern. Add explicit paths to keep the
search bounded.

```sh
ast-grep run --lang rust --pattern 'fn $NAME($$$ARGS) { $$$BODY }' src
ast-grep run --lang typescript --pattern 'console.log($$$ARGS)' packages
ast-grep run --lang python --pattern '$OBJ.$METHOD($$$ARGS)' app tests
```

For machine-readable results, request ast-grep's native JSON and disable terminal
colour:

```sh
ast-grep run --lang rust --pattern '$VALUE.unwrap()' --json=stream --color=never src
```

Ast-grep JSON positions are zero-based. `--json=stream` is preferable for large
pipelines because it emits one object per line.

## Pattern Debugging

Inspect how ast-grep parses a pattern when a query produces surprising matches
or no matches:

```sh
ast-grep run --lang rust --pattern '$EXPR.await' --debug-query=ast
ast-grep run --lang javascript --pattern 'foo($ARG)' --debug-query=cst
ast-grep run --lang typescript --pattern '$OBJ.$METHOD($$$ARGS)' --debug-query=pattern
```

If a fragment is not valid by itself, provide a valid `context` in a YAML rule
and select the intended node:

```yaml
id: object-property
language: TypeScript
rule:
  pattern:
    context: "const value = { $KEY: $VALUE }"
    selector: pair
```

Reduce difficult rules to the smallest source example. Check the parsed AST or
CST, the effective matched node, metavariable spelling, and the language before
adding relational constraints.

## Project Configuration and Rules

A typical project uses this layout:

```text
sgconfig.yml
rules/
rule-tests/
utils/
```

`sgconfig.yml` owns rule directories, test discovery, utility rules, language
globs, custom parsers, and language injections. Run a configured scan with:

```sh
ast-grep scan --config sgconfig.yml --json=stream --include-metadata --color=never .
```

Run one rule without project scaffolding:

```sh
ast-grep scan --rule rules/no-unwrap.yml --json=stream --color=never src
```

You can also use `--inline-rules` for a short experiment. Prefer a checked-in
rule file once the rule is worth testing or maintaining.

A small diagnostic rule looks like this:

```yaml
id: no-unwrap
language: Rust
severity: warning
message: Handle the error instead of unwrapping it.
rule:
  pattern: $VALUE.unwrap()
```

Inspect project and rule discovery when configuration appears to be ignored:

```sh
ast-grep scan --config sgconfig.yml --inspect=summary .
ast-grep scan --config sgconfig.yml --inspect=entity .
```

Use `--filter` to narrow configured rules. Use `--rule` for one standalone rule;
it conflicts with `--config`.

## Rule Tests

Run the configured test suite or a focused subset through `exec_command`:

```sh
ast-grep test --config sgconfig.yml
ast-grep test --config sgconfig.yml --filter 'rust/*'
ast-grep test --test-dir rule-tests --snapshot-dir __snapshots__
ast-grep test --config sgconfig.yml --skip-snapshot-tests
```

Use `--update-all` only when the user has authorised snapshot changes. Inspect
the resulting diff before accepting refreshed snapshots.

## Rewrite Preview and Application

An ast-grep rewrite can change files, so preview it first. Native JSON includes
the proposed replacement without applying it:

```sh
ast-grep run --lang javascript \
  --pattern '$PROP && $PROP()' \
  --rewrite '$PROP?.()' \
  --json=compact --color=never src
```

After reviewing the preview, use `--interactive` for an authorised selective
application:

```sh
ast-grep run --lang javascript \
  --pattern '$PROP && $PROP()' \
  --rewrite '$PROP?.()' \
  --interactive src
```

For reusable fixes, put `fix` beside the YAML rule and preview with `scan`:

```yaml
id: use-char-indices
language: Rust
severity: warning
message: Use byte offsets directly.
rule:
  pattern: $VALUE.chars().enumerate()
fix: $VALUE.char_indices()
```

```sh
ast-grep scan --rule rules/use-char-indices.yml --json=compact --color=never src
```

Use YAML `FixConfig` when a replacement must expand around commas or brackets.
Use `transform`, `rewriters`, and `joinBy` for computed multi-node output. Keep
those operations in explicit rule files so their scope and tests remain visible.

## Custom Languages and Injections

Register custom parsers, file mappings, and embedded-language rules in
`sgconfig.yml`. Inspect discovery with `ast-grep scan --inspect=summary`. If the
grammar itself is unclear, compare ast-grep's debug output with
`tree-sitter parse <file>`.

`languageGlobs` remaps whole files. `languageInjections` parses a matched region
with another language. `customLanguages` registers a tree-sitter parser that
ast-grep does not bundle.

## Choosing the Interface

- Use `exec_command` with the ast-grep CLI for patterns, YAML scans, rule tests,
  parse debugging, configuration inspection, and rewrite previews.
- Use ast-grep's Node.js, Python, or Rust library API only when a rule cannot
  express the required traversal or computed replacement.

Keep searches scoped to relevant paths. Treat no-match and finding exit codes
according to the selected ast-grep subcommand, and inspect ast-grep's native
output before deciding whether a command failed.
