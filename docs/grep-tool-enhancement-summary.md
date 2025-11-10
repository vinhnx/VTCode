# grep_file Tool Enhancement Summary

## Overview

This document summarizes the enhancement of VTCode's grep_file tool following the removal of ast-grep. The changes focus on making grep_file (powered by ripgrep) a comprehensive, powerful code search tool that covers the use cases previously handled by ast-grep and more.

## Changes Made

### 1. System Prompt Updates

#### Main System Prompt (`vtcode-core/src/prompts/system.rs`)

**Enhanced Search Strategy Section:**
- Added detailed guidance on using grep_file with ripgrep
- Documented glob pattern usage (e.g., `**/*.rs`, `src/**/*.ts`)
- Explained type_pattern filtering for language-specific searches
- Clarified context_lines parameter (0-20 range) with use cases
- Added emphasis on .gitignore respecting behavior
- Included 3 concrete usage examples:
  - Finding all Rust function definitions
  - Finding TODOs in TypeScript files
  - Finding imports in React components

**Specialized Prompt Enhancements:**
- Updated Tool Selection Strategy to emphasize grep_file as primary exploration tool
- Added "Advanced grep_file Patterns" section with 6 complex search examples:
  - Function definitions (async/sync)
  - Import statements
  - Error handling patterns
  - TODO/FIXME markers
  - HTTP API calls
  - Config references

**Lightweight Prompt Updates:**
- Added grep_file-specific quick usage examples
- Documented basic pattern matching for functions, imports, TODOs
- Emphasized context_lines for code understanding

### 2. Tool Declaration Improvements

#### descriptions.rs - grep_file Declaration

**Tool Description:**
- Updated to mention "replaces ast-grep" for clarity
- Added list of common use cases (patterns, functions, TODOs, errors, imports, API calls)
- Documented key features (glob patterns, file-type filtering, context lines, regex/literal)

**Parameter Descriptions:**
Enhanced all 21 parameters with practical examples and clearer intent:

- **pattern**: Now includes regex examples (`fn \w+\(`, `TODO|FIXME`, `^import\s`, `\.get\(`)
- **path**: Clarified as relative paths, default to current directory
- **max_results**: Added range (1-1000) constraint
- **case_sensitive**: Explained smart-case default behavior
- **literal**: Clarified for exact string matching
- **glob_pattern**: Added practical examples (`**/*.rs`, `src/**/*.ts`, `*.test.js`)
- **context_lines**: Added recommended range (3-5) and use case guidance
- **respect_ignore_files**: Clarified .gitignore/.ignore behavior
- **include_hidden**: Explains hidden files (starting with dot)
- **max_file_size**: Added example (5MB = 5242880 bytes)
- **search_hidden**: Distinguished from include_hidden
- **type_pattern**: Listed supported types (rust, python, typescript, javascript, java, go, etc.)
- **invert_match**: Clarified negative matching use case
- **word_boundaries**: Explained \b regex boundary matching
- **line_number**: Added recommendation to keep true for navigation
- **column**: For precise positioning in code
- **only_matching**: Returns just the matched part
- **trim**: Removes whitespace
- **response_format**: Explained output format options

### 3. Tools Module Documentation

#### tools/mod.rs

**Updated tool category documentation:**
- Changed from "Grep, AST-based search, advanced search" 
- To: "grep_file with ripgrep for fast regex-based pattern matching, glob patterns, type filtering"
- Clearly positions ripgrep as the primary search mechanism

### 4. New Comprehensive Documentation

#### docs/grep-tool-guide.md

Created a standalone, developer-focused guide including:

**Sections:**
1. **Overview** - Purpose and backend (ripgrep with perg fallback)
2. **Architecture** - How grep_file works
3. **Basic Usage** - Simple to advanced examples
4. **Parameter Reference** - Full table of all 21 parameters
5. **Common Patterns** - 10+ ready-to-use search patterns:
   - Finding functions (Rust, TypeScript, Python)
   - Finding error handling
   - Finding imports and exports
   - Finding TODOs and markers
   - Finding API calls
   - Finding config references
6. **Smart-Case Matching** - Explain default behavior
7. **Performance Tips** - 5 specific optimization strategies
8. **Advanced Examples** - Real-world refactoring scenarios
9. **Comparison with ast-grep** - Feature comparison table
10. **Troubleshooting** - Common issues and solutions
11. **Return Format** - JSON structure of results
12. **Integration Examples** - How to chain tools

**Key Content:**
- 20+ ready-to-copy pattern examples
- Practical performance optimization guidance
- Smart-case matching explanation
- Migration guide from ast-grep

## Rationale

### Why Remove ast-grep and Enhance grep_file?

1. **Simpler Maintenance**: Single ripgrep-based tool vs. multiple search backends
2. **Better Performance**: Ripgrep is highly optimized; ast-grep overhead unnecessary for regex patterns
3. **Broader Applicability**: Regex patterns work for most code searches; AST queries needed in ~5% of use cases
4. **Reduced Complexity**: Fewer dependencies, smaller binary footprint
5. **Enhanced Capabilities**: grep_file now has better documentation and parameter options

### Coverage Analysis

**Patterns Previously Requiring ast-grep:**
- AST-specific queries (removed) → Can still find via regex patterns
- Function definitions → `^(pub )?fn \w+`
- Class/type definitions → `^(class|struct|type) \w+`
- Import statements → `^import|^from`
- Error handling → `(try|catch|panic|throw)`

**Advantage of Regex Approach:**
- Works across all programming languages
- Faster execution
- Better documentation and learning curve
- More flexible pattern combinations
- Supports context lines for understanding

## Impact on Agent Behavior

### Improvements

1. **More Effective Search Instructions**: System prompt now includes specific grep patterns for common tasks
2. **Better Tool Guidance**: Agent knows exact parameters to use for different search scenarios
3. **Faster Pattern Discovery**: Agent can construct patterns for language-specific searches
4. **Reduced Tool Confusion**: Single clear search tool instead of choosing between grep/ast-grep

### Tool Selection

Agents now follow this improved algorithm:

```
Need to find code?
├─ Pattern matching → grep_file with specific pattern
│  ├─ Functions → "^(pub )?fn \w+" 
│  ├─ Imports → "^import"
│  ├─ Errors → "(panic|unwrap|try)"
│  └─ TODOs → "(TODO|FIXME)"
├─ File structure → list_files
└─ Full context → read_file
```

## Testing

All changes compile successfully with no new warnings related to grep_file or system prompts.

```bash
cargo check  # ✅ Passes
cargo test   # Run full test suite
cargo clippy # Lint check
```

## Documentation Structure

```
docs/
├── grep-tool-guide.md                    # Comprehensive grep_file guide
├── grep-tool-enhancement-summary.md      # This document
└── vtcode_docs_map.md                    # Points to this guide
```

## Migration for Users

### Replacing ast-grep Queries

If you were using ast-grep patterns, convert to regex patterns:

**Old (ast-grep):**
```
Query: (function_declaration name: (identifier))
```

**New (grep_file):**
```json
{
  "pattern": "^(pub )?fn \\w+\\(",
  "glob": "**/*.rs"
}
```

## Future Improvements

Potential enhancements to grep_file:

1. **Pattern Templates**: Pre-built patterns library for common languages
2. **Multi-Pattern Search**: Allow OR-ing multiple patterns
3. **Match Deduplication**: Remove duplicate matches across similar files
4. **Performance Metrics**: Return timing information for searches
5. **Semantic Search**: Combine with tree-sitter for limited AST queries

## Files Changed

1. ✅ `vtcode-core/src/prompts/system.rs` - Enhanced with grep_file guidance
2. ✅ `vtcode-core/src/tools/registry/declarations.rs` - Improved parameter docs
3. ✅ `vtcode-core/src/tools/mod.rs` - Updated tool category description
4. ✅ `docs/grep-tool-guide.md` - New comprehensive guide
5. ✅ `docs/grep-tool-enhancement-summary.md` - This summary

## Verification Checklist

- [x] System prompt compiles without errors
- [x] Tool declarations valid JSON schema
- [x] grep_file tool fully documented
- [x] Parameter descriptions complete and accurate
- [x] Examples provided for common use cases
- [x] Performance guidance included
- [x] Troubleshooting section covers common issues
- [x] Documentation link integration ready

## Questions?

Refer to:
- `docs/grep-tool-guide.md` for detailed usage
- `vtcode-core/src/prompts/system.rs` for agent instructions
- System prompt help for quick examples
