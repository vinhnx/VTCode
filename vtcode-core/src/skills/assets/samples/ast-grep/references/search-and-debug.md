# One-shot Search And Debug

Use this path for read-only structural search or to understand why a pattern fails.

## Start with `sg run`

```bash
sg run -p 'console.log($$$ARGS)' --lang ts src
sg run -p 'try { $$$BODY } catch ($ERR) { $$$CATCH }' --lang ts -C 2 src
sg run -p 'foo($A)' --lang ts --selector call_expression --strictness ast src
sg run -p 'logger.info($MSG)' --lang js --globs '**/*.test.js' --globs '!dist/**' src
```

Patterns must be valid code for the chosen language. If the target fragment is not valid standalone code, stop using raw `sg run -p` patterns and move to a YAML pattern object with `context` plus `selector`.
Remember that ast-grep searches CST structure, not raw text. If the difference depends on an operator, punctuation, or modifier token, make that syntax explicit instead of assuming it will be ignored.
Remember that ast-grep already handles some injected-language cases such as JS/CSS inside HTML. If the embedded language is project-specific, move to `sgconfig.yml` `languageInjections` instead of forcing the whole file language.

## Inspect the query when matches are wrong

`--debug-query` shows how ast-grep parsed the pattern. Use it before rewriting the rule.

```bash
sg run -p 'foo($A)' --lang ts --debug-query=pattern
sg run -p 'foo($A)' --lang ts --debug-query=ast
sg run -p 'foo($A)' --lang ts --debug-query=cst
sg run -p 'foo($A)' --lang ts --debug-query=sexp
```

Use `pattern` first. Move to `ast` or `cst` when node kinds or punctuation are unclear.
If CLI results differ from the Playground, compare parser output first. Differences are often caused by parser version, encoding during error recovery, or incomplete pattern context.

## Tuning guide

- Add `--lang` first.
- If `--lang` is unsupported or the extension is not recognized, switch to [custom-languages.md](custom-languages.md) instead of retrying the same query.
- If the file uses a non-standard extension but an existing parser should work, switch to [project-and-config.md](project-and-config.md) and use `languageGlobs`.
- Add `--selector` when the real match should be a subnode.
- Add `--strictness` only when default matching is too loose or too strict.
- Add `-C`, `-A`, or `-B` when reviewing results needs more context.
- Add repeated `--globs` to narrow scope before expanding the pattern.
- If a metavariable misses punctuation or another unnamed token, reconsider the pattern and move to `$$VAR` or a YAML rule path.
- If the issue is "key vs value" or another parent-child role, switch from raw patterns to YAML rules with `field`.
- Metavariables match AST nodes, not substrings. For prefixes like `useSomething`, move to YAML constraints and regex.

## Stop conditions

Switch to [rules-and-tests.md](rules-and-tests.md) when the pattern needs reusable logic, multiple constraints, or durable verification.
Switch to [project-and-config.md](project-and-config.md) when the work needs `sgconfig.yml`, `sg new`, `languageGlobs`, or `languageInjections`.
Switch to [core-concepts.md](core-concepts.md) when the confusion is about CST behavior, named vs unnamed nodes, or `kind` vs `field`.
Switch to [custom-languages.md](custom-languages.md) when the language is unsupported, extensions are not mapped, or `$VAR` is not valid syntax for the target parser.
Switch to [faq-gotchas.md](faq-gotchas.md) when the failure looks like parsing ambiguity, metavariable syntax, `kind` plus `pattern`, or multi-language rule reuse.
Switch to [fallbacks.md](fallbacks.md) when the task is actually plain-text search.
