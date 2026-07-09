# Text Search Guide

> **Note:** legacy text-search dispatcher names are internal implementation
> details. The AI-facing default path for text search is `exec_command.cmd`
> with `rg`.

Shell examples follow the active shell prompt profile. Linux, macOS, and WSL
use the Unix-like profile by default; native Windows uses PowerShell. VT Code
does not rewrite GNU flags for macOS BSD tools and does not translate Unix
commands to PowerShell. Use WSL when you want Unix-like workflows on Windows.

## Overview

Use **ripgrep** (`rg`) through `exec_command.cmd` for fast text and regex
search across codebases. Use shell `grep` only when `rg` is unavailable or when
you need a host-specific grep feature. Use `code_search` for semantic code
search, such as ast-grep structural queries and Tree-sitter outlines.

## Architecture

-   **Backend**: `rg` on PATH, or `grep` as a fallback
-   **Search type**: regex by default, literal string matching with `rg -F`
-   **File filtering**: `rg --glob`, `rg -t`, path arguments, and size limits
-   **Performance**: `rg` respects `.gitignore` and `.ignore` files by default
-   **Output control**: line numbers, column numbers, filename-only output, and
    nearby lines are shell flags

## Basic Usage

Call `exec_command` with a shell command:

```json
{
    "cmd": "rg -n \"TODO\" src"
}
```

### Simple Text Search

```sh
rg -n "TODO" src
```

### Function Definition Search

```sh
rg -n -C 3 --glob "**/*.rs" "^(pub )?fn \\w+\\(" .
```

### Import Statement Search

```sh
rg -n -i --glob "**/*.ts" "^import\\s.*from" .
```

## Flag Reference

### Core Flags

| Need | `rg` command form |
| --- | --- |
| Search under a path | `rg "TODO" src` |
| Show line numbers | `rg -n "TODO" src` |
| Limit result volume | `rg -n "TODO" src | head -c 4000` |
| Return only filenames | `rg -l "TODO" src` |

### Pattern Matching

| Need | `rg` flag |
| --- | --- |
| Literal string search | `-F` |
| Case-insensitive search | `-i` |
| Case-sensitive search | `-s` |
| Smart-case search | `-S` |
| Whole-word search | `-w` |
| Invert a match | `-v` |
| Show only matched text | `-o` |

### File Filtering

| Need | `rg` flag |
| --- | --- |
| Glob file filter | `--glob "**/*.rs"` |
| Language type filter | `-t rust`, `-t python`, `-t ts` |
| Skip large files | `--max-filesize 5M` |
| Search hidden files | `--hidden` |
| Include ignored files | `--no-ignore` |
| Search binary files | `-a` |

### Output Formatting

| Need | `rg` flag |
| --- | --- |
| Nearby lines | `-C 3` |
| Lines before matches | `-B 2` |
| Lines after matches | `-A 2` |
| Column numbers | `--column` |
| Trim leading whitespace | `--trim` |
| JSON output for scripts | `--json` |

## Common Patterns

### Finding Functions

**Rust functions:**

```sh
rg -n --glob "**/*.rs" "^(pub )?async fn \\w+|^(pub )?fn \\w+" .
```

**TypeScript and JavaScript functions:**

```sh
rg -n --glob "**/*.ts" "^(export )?function \\w+|^const \\w+ = (async )?\\(" .
```

**Python functions:**

```sh
rg -n -t python "^def \\w+\\(" .
```

### Finding Error Handling

**Rust panics and unwraps:**

```sh
rg -n -C 2 -t rust "panic!|unwrap\\(|expect\\(" .
```

**Try-catch blocks:**

```sh
rg -n --glob "**/*.ts" "try\\s*\\{|catch\\s*\\(|throw " .
```

### Finding Imports and Exports

**TypeScript imports:**

```sh
rg -n --glob "**/*.ts" "^import\\s+.*from\\s+['\\\"]" .
```

**Python imports:**

```sh
rg -n -t python "^import |^from .* import " .
```

### Finding TODOs and FIXMEs

**All comment markers:**

```sh
rg -n -C 1 "(TODO|FIXME|HACK|BUG|XXX)[:\\s]" .
```

**Language-specific TODOs:**

```sh
rg -n -C 1 -t rust "// TODO|# TODO" .
```

### Finding API Calls

**HTTP verbs:**

```sh
rg -n -C 2 --glob "src/**/*.ts" "\\.(get|post|put|delete|patch)\\(" .
```

**Database queries:**

```sh
rg -n -i --glob "**/*.sql" "SELECT|INSERT|UPDATE|DELETE" .
```

### Finding Config References

**Environment variables:**

```sh
rg -n --glob "**/*.js" "process\\.env\\.|os\\.getenv\\(|getenv\\(" .
```

**Config objects:**

```sh
rg -n -i -C 1 "config\\." .
```

## Smart-Case Matching

Use `rg -S` for smart-case matching:

-   `rg -S "todo"` matches `TODO`, `Todo`, and `todo`
-   `rg -S "TODO"` matches `TODO` only

Use `rg -s` when you always need case-sensitive matching:

```sh
rg -n -s "ERROR" src
```

## Performance Tips

1. **Use specific globs** instead of searching all files:

    ```sh
    rg -n --glob "src/**/*.rs" "fn deploy" .
    ```

2. **Use type filters** for language filtering:

    ```sh
    rg -n -t python "class MyClass" .
    ```

3. **Respect ignore files** by default:

    - Skips `node_modules`, `.git`, and build artefacts automatically
    - Use `--no-ignore` only when you need ignored directories

4. **Limit nearby lines** in large searches:

    ```sh
    rg -n "needle" src
    ```

5. **Use literal matching** when searching exact strings:

    ```sh
    rg -n -F "const.ERROR_MSG" src
    ```

## Advanced Examples

### Refactoring Scenario: Update all imports

```sh
rg -l --glob "src/**/*.ts" "^import.*from.*old-module" .
```

Returns all files importing the old module.

### Finding Unused Exports

```sh
rg -n --glob "src/**/*.ts" "^export.*const|^export.*function" .
```

### Auditing Security Concerns

```sh
rg -n -C 2 --glob "**/*.ts" "eval\\(|exec\\(|innerHTML|dangerouslySetInnerHTML" .
```

### Finding Configuration Issues

```sh
rg -n -i -C 1 "hardcoded.*password|api.*key.*=|token.*=" .
```

## Comparison with ast-grep

| Feature | `rg` | ast-grep |
| --- | --- | --- |
| **Speed** | Very fast | Fast |
| **Pattern type** | Regex and literal text | AST queries |
| **File filtering** | Glob, type, size | Language-aware source files |
| **Language support** | All text files | Supported programming languages |
| **Installation** | Usually pre-installed | Requires binary |
| **Learning curve** | Regex knowledge | AST query knowledge |
| **Use cases** | General code search, prose, config | Syntax-aware code queries |

## Semantic Outline

`code_search` exposes `action=outline`, which wraps the Tree-sitter outline
runtime to produce a cheap, token-efficient symbol map of a file or directory
without requiring a structural query. Use it for the "what is in this file or
directory?" question before reading full source.

**When to use which search action:**

| Action | Question it answers |
| --- | --- |
| `rg` through `exec_command.cmd` | "Which lines match this text or regex?" |
| `code_search` outline | "What symbols are defined in this file or directory?" |
| `code_search` structural | "Which nodes match this AST query?" |

```json
{"action":"outline","path":"vtcode-core/src/tools/registry/builder.rs","lang":"rust","view":"digest"}
{"action":"outline","path":"vtcode-core/src/tools/registry","lang":"rust","type":"function","view":"names","items":"exports"}
```

Sub-fields: `path` (default `.`), `lang`, `type` (string or array of symbol
types), `match` (name regex), `items` (`auto` | `structure` | `exports` |
`imports` | `all`; default `auto`), `pub_members` (bool), `follow` (bool), and
`view` (`digest` | `names` | `full`; default `digest`).

-   `digest` (default): symbols grouped by kind per file, with flat member
    names. About 100-300 bytes for a typical file.
-   `names`: grouped names only, no members.
-   `full`: per-symbol records with the raw zero-based `range`, a derived
    1-based inclusive `lineRange` (`{start, end}`), signatures, `astKind`, and
    nested members. Members also carry `astKind`, `range`, and `lineRange`.

Directory results also include a top-level `summary` with `total_symbols`,
`by_kind` (per-kind symbol counts summing to `total_symbols`), and
`all_symbols` (flat symbol list capped at 200 entries). When the cap is hit,
`summary.truncated` is `true` and `summary.visible_symbols` reports the
visible count. Narrow with `type` or `match`, or outline a specific file.

Outline and structural search shell out to the same resolved `ast-grep` binary.
On a missing binary, both actions auto-install ast-grep on first use by
downloading the matching platform release from GitHub into `~/.vtcode/bin`
with checksum verification and a 24-hour failure cooldown. Set
`VTCODE_AST_GREP_NO_INSTALL=1` to opt out of auto-install; the error then
surfaces immediately with the manual install command
(`vtcode dependencies install ast-grep`). Unlike structural search, outline has
no text equivalent.

## Troubleshooting

### No Results Found

1. Check regex syntax and escape special characters.
2. Verify the path exists.
3. Check whether files are ignored by `.gitignore`; use `--no-ignore` only
   when that is intentional.
4. Add `-C 1` to see nearby lines.

### Too Many Results

1. Add a path, `--glob`, or `-t` filter to narrow scope.
2. Pipe through `head -c 4000` while exploring.
3. Use `rg -l` when filenames are enough.

### Slow Searches

1. Add `--glob` to narrow scope, for example `--glob "**/*.rs"`.
2. Use `-t rust` or another type filter when possible.
3. Set `--max-filesize` to skip large files.
4. Keep ignore files enabled unless you need generated or vendored content.

## Return Format

Text search returns normal shell output from `exec_command`. Use concise shell
formats for AI-facing work:

```sh
rg -n "TODO" src
rg -l "TODO" src
rg --json "TODO" src | head -c 4000
```

## Integration with Other Tools

### Inspecting Matches

1. Use `rg` through `exec_command.cmd` to locate text.
2. Use `sed`, `cat`, or another shell command through `exec_command.cmd` to
   inspect full context.
3. Use `apply_patch` to make changes.

### Scripted Search

```python
# Find all matches.
results = subprocess.run(["rg", "TODO", "src"], check=False, capture_output=True, text=True)
todos = [line for line in results.stdout.splitlines() if "TODO" in line]
```

## See Also

-   [AGENTS.md](../../AGENTS.md) for system prompt integration
-   [Tool Registry](../modules/vtcode_docs_map.md) for tool execution
