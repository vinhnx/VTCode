# Fallbacks

## Prefer plain-text grep when syntax does not matter

Use `rg` or grep for:

- exact strings
- filenames or paths
- logs, stack traces, or command output
- quick inventories where false positives are acceptable

Examples:

```bash
rg -n 'console\\.log' src
rg --files | rg 'test'
rg -n 'TODO|FIXME' .
```

Do not force ast-grep into text-only tasks.

## If `sg` is missing

Check first:

```bash
command -v ast-grep
command -v sg
```

If `sg` is unavailable:

- in VT Code, install the search bundle with `vtcode dependencies install search-tools` or just ast-grep with `vtcode dependencies install ast-grep`
- say structural search or AST inspection cannot be done accurately in the current environment
- if the task can tolerate text-only approximation, use `rg` and label it as a fallback
- if syntax accuracy is required, stop and ask for ast-grep availability instead of guessing
- if the install succeeded but the shell still cannot find it, suggest `export PATH="$HOME/.vtcode/bin:$PATH"`

Linux note:

- prefer `ast-grep` over `sg`; VT Code does not rely on `sg` there because it can collide with the system `setgroups` command

## If `sg` is present but the language is unsupported

- check whether the language is built into ast-grep first
- if not, register a custom parser in workspace-local `sgconfig.yml` instead of retrying the same failing command
- if the language treats `$` as invalid syntax, use the configured `expandoChar` for metavariables
- if no parser can be built or reused, say structural search is unavailable for that language in the current environment

## Good fallback language

`ast-grep` is not available here, so I cannot verify syntax-aware matches. I can either approximate with plain-text grep or install the optional search bundle with `vtcode dependencies install search-tools`.
