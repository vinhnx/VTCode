# Prompting Defaults

Use this before writing a pattern or rule.

## Turn the request into constraints

Write down:

- language
- search root and globs
- must-match examples
- must-not-match examples
- syntax node to anchor on
- whether the important distinction is text, structure, node kind, field role, or an unnamed token
- whether the language is built-in or requires a custom parser / `expandoChar`
- whether the code lives in a normal file, a non-standard extension, or an injected subregion
- whether punctuation, comments, or exact text shape matter
- whether the result is a one-shot search or a reusable rule

## Defaults for ast-grep work

- Restate the task as concrete syntax constraints, not natural-language intent.
- Start from examples and counterexamples.
- Prefer the smallest pattern that can prove the target structure.
- When the query is fragile, reduce freedom: specify `--lang`, `--selector`, `--strictness`, and exact examples.
- If the target is not valid standalone code, plan a pattern object with `context` plus `selector` instead of forcing a fragment into `pattern`.
- If the language is unsupported, plan the `sgconfig.yml` custom-language step explicitly instead of inventing a nearby built-in language.
- If the parser exists but the extension is unusual, plan `languageGlobs` instead of a custom parser.
- If the target syntax is embedded in another language, plan `languageInjections` instead of treating the host file as the injected language.
- If the difference depends on a role like key/value, note that you will probably need `field`.
- If the difference depends on punctuation or another unnamed token, note that you may need `$$VAR` or explicit syntax.
- If `$VAR` is invalid syntax for the language, note that the pattern must use the configured `expandoChar`.
- If the task wants a reusable project workflow, note whether you need `sg new`, `ruleDirs`, `testConfigs`, or `utilDirs`.
- If the task wants code changes, decide whether plain `fix` is enough or whether nested rewriting needs `transform` or `rewriters`.
- If matches fail, inspect the query with `--debug-query` before adding more rule logic.
- Iterate in a loop: pattern or rule, test on examples, rerun on target files.

## Minimal template

```text
Goal:
Language:
Search root:
Must match:
Must not match:
Candidate pattern:
Built-in or custom language:
Normal file, unusual extension, or injection:
Kind or field distinction:
Named or unnamed token involved:
Optional selector:
Optional strictness:
Need reusable rule? yes/no
Need rewrite / transform / rewriters? yes/no
```
