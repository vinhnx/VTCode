# Custom Languages

Use this path when the target language is not built into ast-grep or when the file extension is not being recognized correctly.

## What VT Code does

VT Code does not bundle extra ast-grep parsers.

`unified_search` with `action="structural"` runs local `sg` from the workspace root, so workspace-local `sgconfig.yml` is the place to make custom languages work.

## Recommended setup

1. Install `tree-sitter` CLI and get the grammar for the target language.
2. Compile the grammar as a dynamic library.
3. Register that parser under `customLanguages` in `sgconfig.yml`.

Minimal shape:

```yaml
customLanguages:
  my_lang:
    libraryPath: path/to/parser.so
    extensions: [mylang]
    expandoChar: _
```

Use `libraryPath` for the compiled parser library and `extensions` for file detection.
Use `expandoChar` when `$VAR` is not valid syntax in the target language. In that case, patterns should use the configured replacement character instead of `$`.
If the parser already exists and only the extension is unusual, use `languageGlobs` instead of `customLanguages`.
If the code is embedded inside another host language, use `languageInjections` instead of pretending the host file is entirely the injected language.

## Practical guidance

- Reuse an existing compiled parser when available, for example one already built by the editor environment.
- Keep the custom language name stable and use that alias in `--lang` or YAML `language:`.
- After registration, rerun `sg` from the workspace root so `sgconfig.yml` is in scope.
- If matching still looks wrong, inspect parser output with `tree-sitter parse <file>` and compare it to `sg run --debug-query=ast`.

## Verification

```bash
sg run -p 'target($A)' --lang my_lang .
sg run -p 'target($A)' --lang my_lang --debug-query=ast .
tree-sitter parse path/to/file.my_lang
```

## Stop conditions

- If the language is unsupported and no parser can be built or reused, say structural search cannot be verified accurately in the current environment.
- Do not pretend a nearby built-in language is "close enough" unless the user explicitly accepts an approximation.
