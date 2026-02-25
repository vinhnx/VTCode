# grep_file Quick Reference Card

## Essential Parameters

```json
{
  "pattern": "TODO",           // Required: regex or literal string
  "path": "src",               // Directory to search (default: ".")
  "max_results": 50,           // Results limit (default: 100, max: 1000)
  "glob_pattern": "**/*.rs",   // Filter files: **/*.rs, src/**/*.ts, etc.
  "type_pattern": "rust",      // Language: rust, python, typescript, java, go
  "context_lines": 3,          // Lines around match (0-20, default: 0)
  "literal": false,            // false=regex (default), true=literal string
  "case_sensitive": false      // false=smart-case (default), true=case-sensitive
}
```

## Common Search Patterns

| Task | Pattern | Glob | Notes |
|------|---------|------|-------|
| Find functions | `^(pub )?fn \w+\(` | `**/*.rs` | Rust function definitions |
| Find imports | `^import.*from` | `**/*.ts` | TypeScript/JS imports |
| Find classes | `^class \w+` | `**/*.java` | Java class definitions |
| Find TODOs | `TODO\|FIXME` | `**/*.rs` | Common markers |
| Find errors | `panic!\|unwrap\|throw` | `**/*.rs` | Error patterns |
| Find API calls | `\.get\(\|\.post\(` | `**/*.ts` | HTTP verbs |
| Find exports | `^export ` | `**/*.ts` | Module exports |
| Find config | `config\.` | `**/*.py` | Config references |
| Find async | `async fn` | `**/*.rs` | Async functions |
| Find unused | `^pub fn` | `**/*.rs` | Public functions (for refactoring) |

## Smart Patterns by Language

### Rust
```json
{"pattern": "^fn \\w+", "type_pattern": "rust"}
{"pattern": "impl \\w+", "type_pattern": "rust"}
{"pattern": "#\\[test\\]", "context_lines": 1}
{"pattern": "Result<|Option<", "type_pattern": "rust"}
```

### TypeScript
```json
{"pattern": "^export (function|const|class)", "glob": "**/*.ts"}
{"pattern": "interface \\w+", "glob": "**/*.ts"}
{"pattern": "async \\(", "glob": "**/*.tsx"}
{"pattern": "useState|useEffect|useContext", "glob": "**/*.tsx"}
```

### Python
```json
{"pattern": "^def \\w+", "type_pattern": "python"}
{"pattern": "^class \\w+", "type_pattern": "python"}
{"pattern": "import |from .* import", "type_pattern": "python"}
{"pattern": "@property|@staticmethod", "context_lines": 2}
```

## Performance Tips

| Optimization | Benefit | Example |
|--------------|---------|---------|
| Use `glob_pattern` | 10-100x faster | `glob: "**/*.rs"` |
| Use `type_pattern` | 5-10x faster | `type_pattern: "rust"` |
| Set `max_file_size` | Skip large files | `max_file_size: 5242880` |
| Keep `respect_ignore_files: true` | Skip node_modules, build | (default) |
| Reduce `context_lines` | Smaller output | `context_lines: 0` |
| Use `literal: true` | Faster for exact strings | `literal: true` |

## Output Example

```json
{
  "success": true,
  "query": "TODO",
  "matches": [
    {
      "type": "match",
      "data": {
        "path": {"text": "src/main.rs"},
        "line_number": 42,
        "lines": {"text": "// TODO: refactor this function\n"}
      }
    }
  ]
}
```

## Real-World Examples

### Refactor: Replace old import
```json
{
  "pattern": "^import.*OldLib",
  "glob": "src/**/*.ts",
  "files_with_matches": true
}
```
Result: All files needing import updates

### Audit: Find hardcoded secrets
```json
{
  "pattern": "password|api.key|token",
  "case_sensitive": false,
  "glob": "src/**/*.{ts,js,py}",
  "context_lines": 1
}
```

### Refactor: Identify deprecated API
```json
{
  "pattern": "\\.oldMethod\\(",
  "glob": "src/**/*.{ts,tsx}",
  "max_results": 500
}
```

### Analysis: Find error handling patterns
```json
{
  "pattern": "try\\s*{|catch\\s*\\(|throw ",
  "type_pattern": "typescript",
  "context_lines": 2
}
```

## Regex Cheat Sheet

| Pattern | Matches |
|---------|---------|
| `.` | Any character |
| `\w` | Word character [a-zA-Z0-9_] |
| `\d` | Digit [0-9] |
| `\s` | Whitespace |
| `^` | Line start |
| `$` | Line end |
| `\|` | OR (escaped in JSON) |
| `(...)` | Group |
| `*` | 0 or more |
| `+` | 1 or more |
| `?` | 0 or 1 |
| `[...]` | Character class |

**In JSON, escape backslashes:** `\\w`, `\\d`, `\\s`

## Decision Tree

```
What do you want to find?

 Functions/Classes?
   Rust: ^(pub )?fn \w+|^pub struct
   Python: ^def |^class 
   TypeScript: ^export (function|class)

 Imports/Exports?
   TypeScript/JS: ^import|^export
   Python: ^import |^from

 Error handling?
   Rust: panic!|unwrap|Result
   JS/TS: try|catch|throw
   Python: try|except|raise

 TODOs/Comments?
   (TODO|FIXME|HACK|XXX)

 API/Database?
   HTTP: \.get\(|\.post\(|\.put\(
   SQL: SELECT|INSERT|UPDATE

 Config/Constants?
    config\.|process.env|os.getenv
```

## Common Mistakes

|   Wrong |   Right | Why |
|---------|---------|-----|
| `pattern: "fn test"` | `pattern: "^fn test"` | Anchor patterns to line start |
| `glob_pattern: "*.rs"` | `glob_pattern: "**/*.rs"` | Use `**` for recursive |
| `pattern: "my.variable"` | `pattern: "my\.variable"` | Escape special chars |
| No `glob` + huge search | `glob: "src/**/*.ts"` | Always narrow scope |
| `context_lines: 50` | `context_lines: 3` | Reasonable context (0-20) |
| Large codebase searches | `type_pattern: "rust"` | Use type_pattern not glob |

## See Also

- Full guide: `docs/development/grep-tool-guide.md`
- System prompt: Agent instructions for grep usage
- ripgrep docs: https://github.com/BurntSushi/ripgrep
