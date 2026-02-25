# grep_file Tool Guide

## Overview

The `grep_file` tool is VT Code's primary code search mechanism, powered by **ripgrep** for fast, efficient pattern matching across codebases. It replaces the previous AST-grep tool and provides comprehensive regex-based and literal string searching with support for file filtering, context lines, and language-specific searches.

## Architecture

-   **Backend**: ripgrep (rg) with fallback to perg for environments where ripgrep is unavailable
-   **Search Type**: Regex-based (default) or literal string matching
-   **File Filtering**: Glob patterns, file type matching, size limits
-   **Performance**: Respects `.gitignore` and `.ignore` files by default for faster searches
-   **Context**: Optional surrounding lines for understanding matched code

## Basic Usage

### Simple Pattern Search

```json
{
    "pattern": "TODO",
    "path": "src"
}
```

### Function Definition Search

```json
{
    "pattern": "^(pub )?fn \\w+\\(",
    "glob": "**/*.rs",
    "context_lines": 3
}
```

### Import Statement Search

```json
{
    "pattern": "^import\\s.*from",
    "glob": "**/*.ts",
    "case_sensitive": false
}
```

## Parameter Reference

### Core Parameters

| Parameter     | Type    | Default      | Description                                   |
| ------------- | ------- | ------------ | --------------------------------------------- |
| `pattern`     | string  | _(required)_ | Regex pattern or literal string to search for |
| `path`        | string  | "."          | Directory to search (relative path)           |
| `max_results` | integer | 100          | Maximum results to return (1-1000)            |

### Pattern Matching

| Parameter         | Type    | Default | Description                                            |
| ----------------- | ------- | ------- | ------------------------------------------------------ |
| `literal`         | boolean | false   | Treat pattern as literal string (disable regex)        |
| `case_sensitive`  | boolean | false   | Force case-sensitive matching. Default uses smart-case |
| `word_boundaries` | boolean | false   | Match only at word boundaries (`\b` in regex)          |
| `invert_match`    | boolean | false   | Return lines that DON'T match the pattern              |
| `only_matching`   | boolean | false   | Show only matched parts, not full lines                |

### File Filtering

| Parameter              | Type    | Default | Description                                                                |
| ---------------------- | ------- | ------- | -------------------------------------------------------------------------- |
| `glob_pattern`         | string  | null    | Glob pattern to filter files (e.g., `**/*.rs`, `src/**/*.ts`)              |
| `type_pattern`         | string  | null    | Filter by file type (rust, python, typescript, javascript, java, go, etc.) |
| `max_file_size`        | integer | null    | Skip files larger than this (in bytes)                                     |
| `respect_ignore_files` | boolean | true    | Respect `.gitignore` and `.ignore` files                                   |
| `search_hidden`        | boolean | false   | Search inside hidden directories (starting with `.`)                       |
| `include_hidden`       | boolean | false   | Include hidden files in results                                            |
| `search_binary`        | boolean | false   | Search binary files (usually false)                                        |

### Output Formatting

| Parameter         | Type    | Default   | Description                                     |
| ----------------- | ------- | --------- | ----------------------------------------------- |
| `context_lines`   | integer | 0         | Lines of context before/after matches (0-20)    |
| `line_number`     | boolean | true      | Include line numbers in output                  |
| `column`          | boolean | false     | Include column numbers for exact match position |
| `trim`            | boolean | false     | Trim leading/trailing whitespace                |
| `response_format` | string  | "concise" | Output format (concise or detailed)             |

## Common Patterns

### Finding Functions

**Rust functions:**

```json
{
    "pattern": "^(pub )?async fn \\w+|^(pub )?fn \\w+",
    "glob": "**/*.rs"
}
```

**TypeScript/JavaScript functions:**

```json
{
    "pattern": "^(export )?function \\w+|^const \\w+ = (async )?\\(",
    "glob": "**/*.ts"
}
```

**Python functions:**

```json
{
    "pattern": "^def \\w+\\(",
    "type_pattern": "python"
}
```

### Finding Error Handling

**Rust panics and unwraps:**

```json
{
    "pattern": "panic!|unwrap\\(|expect\\(",
    "type_pattern": "rust",
    "context_lines": 2
}
```

**Try-catch blocks:**

```json
{
    "pattern": "try\\s*{|catch\\s*\\(|throw ",
    "glob": "**/*.ts"
}
```

### Finding Imports and Exports

**TypeScript imports:**

```json
{
    "pattern": "^import\\s+.*from\\s+['\"]",
    "glob": "**/*.ts"
}
```

**Python imports:**

```json
{
    "pattern": "^import |^from .* import ",
    "type_pattern": "python"
}
```

### Finding TODOs and FIXMEs

**All comment markers:**

```json
{
    "pattern": "(TODO|FIXME|HACK|BUG|XXX)[:\\s]",
    "context_lines": 1
}
```

**Language-specific TODOs:**

```json
{
    "pattern": "// TODO|# TODO",
    "type_pattern": "rust",
    "context_lines": 1
}
```

### Finding API Calls

**HTTP verbs:**

```json
{
    "pattern": "\\.(get|post|put|delete|patch)\\(",
    "glob": "src/**/*.ts",
    "context_lines": 2
}
```

**Database queries:**

```json
{
    "pattern": "SELECT|INSERT|UPDATE|DELETE",
    "glob": "**/*.sql",
    "case_sensitive": false
}
```

### Finding Config References

**Environment variables:**

```json
{
    "pattern": "process\\.env\\.|os\\.getenv\\(|getenv\\(",
    "glob": "**/*.js"
}
```

**Config objects:**

```json
{
    "pattern": "config\\.",
    "case_sensitive": false,
    "context_lines": 1
}
```

## Smart-Case Matching

By default, grep_file uses **smart-case matching**:

-   **Lowercase pattern** → Case-insensitive search

    -   `pattern: "todo"` matches "TODO", "Todo", "todo"

-   **Uppercase characters in pattern** → Case-sensitive search
    -   `pattern: "TODO"` matches "TODO" only
    -   `pattern: "myVar"` matches "myVar" only

Force case sensitivity with `case_sensitive: true`:

```json
{
    "pattern": "ERROR",
    "case_sensitive": true // Forces case-sensitive match
}
```

## Performance Tips

1. **Use specific globs** instead of searching all files:

    ```json
    {
        "pattern": "fn deploy",
        "glob": "src/**/*.rs" // Much faster than searching entire directory
    }
    ```

2. **Use type_pattern** for language filtering:

    ```json
    {
        "pattern": "class MyClass",
        "type_pattern": "python" // Faster than glob: "**/*.py"
    }
    ```

3. **Respect ignore files** by default (leave `respect_ignore_files: true`)

    - Skips node_modules, .git, build artifacts automatically
    - Set `false` only when you need to search ignored directories

4. **Limit context lines** in large searches:

    ```json
    {
        "pattern": ".*",
        "context_lines": 0 // No context for massive matches
    }
    ```

5. **Use literal matching** when searching exact strings:
    ```json
    {
        "pattern": "const.ERROR_MSG",
        "literal": true // Faster than regex for exact strings
    }
    ```

## Advanced Examples

### Refactoring Scenario: Update all imports

```json
{
    "pattern": "^import.*from.*old-module",
    "glob": "src/**/*.ts",
    "context_lines": 0,
    "files_with_matches": true
}
```

Returns: List of all files importing the old module.

### Finding unused exports

```json
{
    "pattern": "^export.*const|^export.*function",
    "glob": "src/**/*.ts",
    "max_results": 500
}
```

### Auditing security concerns

```json
{
    "pattern": "eval\\(|exec\\(|innerHTML|dangerouslySetInnerHTML",
    "glob": "**/*.ts",
    "context_lines": 2
}
```

### Finding configuration issues

```json
{
    "pattern": "hardcoded.*password|api.*key.*=|token.*=",
    "case_sensitive": false,
    "context_lines": 1
}
```

## Comparison with ast-grep

| Feature              | grep_file (ripgrep)           | ast-grep             |
| -------------------- | ----------------------------- | -------------------- |
| **Speed**            | Very fast                     | Fast                 |
| **Pattern Type**     | Regex + literal               | AST queries          |
| **File Filtering**   | Glob, type, size              | Limited              |
| **Language Support** | All languages                 | Limited              |
| **Installation**     | Usually pre-installed         | Requires binary      |
| **Learning Curve**   | Regex knowledge               | Domain language      |
| **Use Cases**        | General code search, patterns | AST-specific queries |

**Migration:** Replace AST grep queries with regex patterns targeting:

-   Function signatures: `^(pub )?fn name`
-   Class definitions: `^class Name`
-   Imports: `^import|^from`
-   Comments: `#|//|/*`

## Troubleshooting

### No results found

1. Check pattern syntax (regex special chars must be escaped)
2. Verify path exists
3. Check if files are in `.gitignore` (add `respect_ignore_files: false`)
4. Use `context_lines: 1` to debug with surrounding lines

### Too many results

1. Add `glob_pattern` or `type_pattern` to narrow scope
2. Increase `max_results` limit (up to 1000)
3. Use `files_with_matches: true` to get just filenames

### Slow searches

1. Add `glob_pattern` to narrow scope (e.g., `**/*.rs`)
2. Use `type_pattern` instead of glob when possible
3. Set `max_file_size` to skip large files
4. Use `respect_ignore_files: true` (default) to skip node_modules, etc.

## Return Format

```json
{
    "success": true,
    "query": "TODO",
    "matches": [
        {
            "type": "match",
            "data": {
                "path": { "text": "src/main.rs" },
                "line_number": 42,
                "lines": { "text": "// TODO: refactor this function\n" }
            }
        }
    ]
}
```

## Integration with Other Tools

### Using grep_file with read_file

```
1. grep_file to locate patterns
2. read_file to examine full context
3. edit_file to make changes
```

### Using grep_file with execute_code

```python
# Find all matches
results = grep_file(pattern="TODO", path="src")
# Process results locally
todos = [m['data']['lines']['text'] for m in results['matches']]
```

## See Also

-   [AGENTS.md](../AGENTS.md) for system prompt integration
-   [Tool Registry](vtcode_docs_map.md) for tool execution
