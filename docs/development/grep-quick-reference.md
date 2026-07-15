# rg Text Search Quick Reference Card

> **Note:** legacy text-search dispatcher names are internal implementation
> details. Use `exec_command.cmd` with `rg` for flexible shell text search.
> Advanced `code_search` provides bounded literal search across definitions,
> syntactic usages, text, and paths.

Shell examples follow the active shell prompt profile. Linux, macOS, and WSL
use the Unix-like profile by default; native Windows uses PowerShell. VT Code
does not rewrite GNU flags for macOS BSD tools and does not translate Unix
commands to PowerShell. Use WSL when you want Unix-like workflows on Windows.

## Essential Commands

```sh
rg "TODO" src                 # pattern and path
rg -n -C 3 "TODO" src         # line numbers and nearby lines
rg --glob "**/*.rs" "TODO"    # file filter
rg -t rust "TODO"             # language filter
rg -F "literal text" src      # literal string
rg -i "todo" src              # case-insensitive search
rg -l "TODO" src              # filenames only
```

For the live AI-facing tool call, pass the command through `exec_command.cmd`:

```json
{"cmd":"rg -n \"TODO\" src"}
```

## Common Search Patterns

| Task | Regex | File filter | Notes |
| --- | --- | --- | --- |
| Find functions | `^(pub )?fn \w+\(` | `--glob "**/*.rs"` | Rust function definitions |
| Find imports | `^import.*from` | `--glob "**/*.ts"` | TypeScript and JavaScript imports |
| Find classes | `^class \w+` | `--glob "**/*.java"` | Java class definitions |
| Find TODOs | `TODO\|FIXME` | `--glob "**/*.rs"` | Common markers |
| Find errors | `panic!\|unwrap\|throw` | `--glob "**/*.rs"` | Error patterns |
| Find API calls | `\.get\(\|\.post\(` | `--glob "**/*.ts"` | HTTP verbs |
| Find exports | `^export ` | `--glob "**/*.ts"` | Module exports |
| Find config | `config\.` | `--glob "**/*.py"` | Config references |
| Find async | `async fn` | `--glob "**/*.rs"` | Async functions |
| Find unused | `^pub fn` | `--glob "**/*.rs"` | Public functions for refactoring |

## Smart Patterns by Language

### Rust

```sh
rg -n -t rust "^fn \\w+" .
rg -n -t rust "impl \\w+" .
rg -n -C 1 "#\\[test\\]" .
rg -n -t rust "Result<|Option<" .
```

### TypeScript

```sh
rg -n --glob "**/*.ts" "^export (function|const|class)" .
rg -n --glob "**/*.ts" "interface \\w+" .
rg -n --glob "**/*.tsx" "async \\(" .
rg -n --glob "**/*.tsx" "useState|useEffect|useContext" .
```

### Python

```sh
rg -n -t python "^def \\w+" .
rg -n -t python "^class \\w+" .
rg -n -t python "import |from .* import" .
rg -n -C 2 "@property|@staticmethod" .
```

## Performance Tips

| Optimisation | Benefit | Example |
| --- | --- | --- |
| Use `--glob` | 10-100x faster | `--glob "**/*.rs"` |
| Use `-t` | 5-10x faster | `-t rust` |
| Set `--max-filesize` | Skip large files | `--max-filesize 5M` |
| Keep ignore files enabled | Skip `node_modules` and build output | default |
| Reduce nearby lines | Smaller output | omit `-C`, `-A`, and `-B` |
| Use `-F` | Faster for exact strings | `rg -F "literal text"` |

## Output Example

```text
src/main.rs:42:// TODO: refactor this function
```

Use `rg -l` when you only need filenames:

```text
src/main.rs
src/lib.rs
```

## Advanced `code_search`

The advanced profile exposes `code_search` with required `query` and optional
`path`, `file_types`, `result_types`, and `max_results`. The four result types
are `definition`, `usage`, `text`, and `path`. Definitions are recognised
declarations. Usages are exact syntactic identifiers, not resolved references.
Text covers prose, configuration, comments, strings, and unclassified literal
matches. Path results match filenames or paths.

Literal smart-case applies: wholly lower-case queries are case-insensitive;
queries containing an upper-case character are case-sensitive. If a response
is truncated, narrow a filter in another call. No exact repository-wide total
is implied.

```json
{"query":"Widget","path":"src","file_types":["rust"],"result_types":["definition","usage"],"max_results":20}
```

Use `exec_command` or the specialised ast-grep skill for arbitrary structural
patterns.

## Real-World Examples

### Refactor: Replace Old Import

```sh
rg -l --glob "src/**/*.ts" "^import.*OldLib" .
```

Result: all files needing import updates.

### Audit: Find Hardcoded Secrets

```sh
rg -n -i -C 1 --glob "src/**/*.{ts,js,py}" "password|api.key|token" .
```

### Refactor: Identify Deprecated API

```sh
rg -n --glob "src/**/*.{ts,tsx}" "\\.oldMethod\\(" .
```

### Analysis: Find Error Handling Patterns

```sh
rg -n -C 2 -t typescript "try\\s*\\{|catch\\s*\\(|throw " .
```

## Regex Cheat Sheet

| Regex | Matches |
| --- | --- |
| `.` | Any character |
| `\w` | Word character `[a-zA-Z0-9_]` |
| `\d` | Digit `[0-9]` |
| `\s` | Whitespace |
| `^` | Line start |
| `$` | Line end |
| `\|` | OR in shell examples that need escaping |
| `(...)` | Group |
| `*` | 0 or more |
| `+` | 1 or more |
| `?` | 0 or 1 |
| `[...]` | Character class |

Escape backslashes for JSON shell strings, for example `\\w`, `\\d`, and
`\\s`.

## Decision Tree

```text
What do you want to find?

 Functions and classes?
   Rust: ^(pub )?fn \w+|^pub struct
   Python: ^def |^class
   TypeScript: ^export (function|class)

 Imports and exports?
   TypeScript and JavaScript: ^import|^export
   Python: ^import |^from

 Error handling?
   Rust: panic!|unwrap|Result
   JavaScript and TypeScript: try|catch|throw
   Python: try|except|raise

 TODOs and comments?
   (TODO|FIXME|HACK|XXX)

 API and database?
   HTTP: \.get\(|\.post\(|\.put\(
   SQL: SELECT|INSERT|UPDATE

 Config and constants?
   config\.|process.env|os.getenv
```

## Common Mistakes

| Wrong | Right | Why |
| --- | --- | --- |
| `rg "fn test"` | `rg "^fn test"` | Anchor patterns to line start |
| `rg --glob "*.rs"` | `rg --glob "**/*.rs"` | Use `**` for recursive filters |
| `rg "my.variable"` | `rg "my\\.variable"` | Escape special chars |
| `rg "needle" .` on a huge tree | `rg --glob "src/**/*.ts" "needle" .` | Narrow scope |
| `rg -C 50 "needle"` | `rg -C 3 "needle"` | Keep nearby output small |
| Large codebase searches | `rg -t rust "needle"` | Use a type filter where possible |

## See Also

- Full guide: `docs/development/grep-tool-guide.md`
- Advanced code search: see the grep-tool-guide "Advanced `code_search`"
  section.
- System prompt: agent instructions for grep usage
- ripgrep docs: https://github.com/BurntSushi/ripgrep
