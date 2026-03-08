# Project And Config

Use this path when ast-grep work is becoming a reusable project instead of a one-off command.

## When to use `sg new`

Prefer scaffolding over hand-made folders when starting from scratch:

```bash
sg new project
sg new rule my-rule --lang ts -y
sg new test my-rule -y
sg new util shared-helper --lang ts -y
```

`sg new project` creates the usual project skeleton:

- `sgconfig.yml`
- rule directories
- test directories
- utility-rule directories

Use `sg new rule`, `sg new test`, or `sg new util` when the project already exists and you want the right file placement without guesswork.

## `sgconfig.yml` defaults that matter most

`sgconfig.yml` configures project discovery. It is not a rule file.

Key sections:

- `ruleDirs`: where ast-grep should find reusable YAML rules
- `testConfigs`: where `sg test` should find test cases and snapshots
- `utilDirs`: where shared global utility rules live
- `languageGlobs`: map unusual file extensions to an existing parser
- `customLanguages`: register a new parser via tree-sitter dynamic library
- `languageInjections`: parse embedded languages inside host documents

Minimal project shape:

```yaml
ruleDirs:
  - rules
testConfigs:
  - testDir: rule-tests
utilDirs:
  - utils
```

## Pick the right config tool

- Use `languageGlobs` when the parser already exists and only the extension or file name is non-standard.
- Use `customLanguages` when the language itself is not built into ast-grep.
- Use `languageInjections` when code is embedded inside another language, such as CSS in JS template strings or GraphQL in tagged templates.

Examples:

```yaml
languageGlobs:
  html: ['*.vue', '*.svelte', '*.astro']
  tsx: ['*.ts']
```

```yaml
languageInjections:
  - hostLanguage: js
    rule:
      pattern: styled.$TAG`$CONTENT`
    injected: css
```

## Injection rule requirements

For custom injections:

- the `rule` must locate the host-language node that contains the embedded source
- the rule should capture `$CONTENT` as the injected subregion
- `injected` can be a fixed language string or a candidate list that uses `$LANG`

Built-in HTML JS/CSS injection already works in ast-grep. Add project config only when the default behavior is not enough.

## Verification

```bash
sg scan --config sgconfig.yml --rule rules/my-rule.yml .
sg test -c sgconfig.yml
```

If config-driven matching still looks wrong, inspect the rule with `--debug-query` and the host parser shape before adding more YAML.
