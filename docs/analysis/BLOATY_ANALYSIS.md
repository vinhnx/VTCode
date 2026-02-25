# Bloaty Analysis Report for vtcode

## Overview

[Bloaty](https://github.com/google/bloaty) is a size profiler for binaries that helps understand what's making the binary large. This report analyzes the vtcode project binary size composition.

## Binary Size Summary

| Profile | Size | Notes |
|---------|------|-------|
| `debug` | 84 MiB | Unoptimized with full debug info |
| `release-fast` | 32 MiB | Optimized, stripped (62% smaller) |

**Size Reduction:** Release-fast profile reduces binary size by **~62%** (52 MiB saved)

---

## Release-fast Binary Analysis (32 MiB)

### By Segments

```
FILE SIZE        VM SIZE    
 --------------  -------------- 
  94.4%  30.4Mi  94.3%  30.4Mi    __TEXT
   3.1%  1024Ki   3.1%  1024Ki    __DATA_CONST
   2.3%   744Ki   2.3%   752Ki    __LINKEDIT
   0.2%  64.0Ki   0.3%  96.0Ki    __DATA
   0.0%     104   0.0%       0    [Mach-O Headers]
 100.0%  32.2Mi 100.0%  32.2Mi    TOTAL
```

**Key Insight:** 94% of the binary is code (`__TEXT` segment), which is typical for Rust binaries.

### By Sections

```
FILE SIZE        VM SIZE    
 --------------  -------------- 
  70.1%  22.5Mi  70.0%  22.5Mi    [__TEXT,__text]       ← Actual code
  22.9%  7.35Mi  22.8%  7.35Mi    [__TEXT,__const]      ← Constants/rodata
   3.1%  1020Ki   3.1%  1020Ki    [__DATA_CONST,__const]
   2.3%   744Ki   2.3%   751Ki    [__LINKEDIT]
   1.0%   329Ki   1.0%   329Ki    [__TEXT,__cstring]    ← String literals
```

**Key Insights:**
- **70% code** (`__text`) - This is the actual executable code
- **23% constants** (`__const`) - Static data, vtables, etc.
- **1% strings** (`__cstring`) - String literals in the code

---

## Debug Binary Analysis (84 MiB)

### Top Size Contributors

| Component | Size | % of Total | Description |
|-----------|------|------------|-------------|
| `[Others]` | 62.5 MiB | 74.1% | 186,753 individual symbols |
| `__eh_frame` | 7.82 MiB | 9.3% | Exception handling info |
| `__LINKEDIT` | 6.10 MiB | 7.2% | Link edit information |
| `_ts_parse_table` | 2.39 MiB | 2.8% | **Tree-sitter parse tables** |
| `_ts_small_parse_table` | 1.63 MiB | 1.9% | **Tree-sitter small parse tables** |
| `__unwind_info` | 1.56 MiB | 1.8% | Unwind information |
| `_ts_parse_actions` | 386 KiB | 0.4% | **Tree-sitter parse actions** |
| `_ts_lex` | 225 KiB | 0.3% | **Tree-sitter lexer** |
| `_ts_lex_modes` | 130 KiB | 0.2% | **Tree-sitter lexer modes** |

### Tree-sitter Impact

**Total Tree-sitter size: ~4.8 MiB (5.7% of debug binary)**

```
_ts_parse_table         2.39 MiB
_ts_small_parse_table   1.63 MiB
_ts_parse_actions       386 KiB
_ts_lex                 225 KiB
_ts_lex_modes           130 KiB
_ts_small_parse_table_map 82 KiB
```

This comes from the `tree-sitter-swift` feature and other tree-sitter language parsers.

### Largest Individual Symbols

| Symbol | Size | Description |
|--------|------|-------------|
| `vtcode_core::tools::registry::declarations::base_function_declarations` | 192 KiB | Tool registry declarations |
| `vtcode::agent::runloop::unified::turn::guards::validate_tool_args_security::EMPTY_REQUIRED` | 82.8 KiB | Security validation constants |
| `vtcode_core::tools::registry::execution_facade::{{closure}}` | 76.4 KiB | Tool execution closure |
| `vtcode_core::exec::cancellation::ACTIVE_TOOL_TOKEN` | 61.8 KiB | Cancellation token |

---

## Recommendations

### 1. **Tree-sitter Optimization** (Potential savings: ~3-4 MiB)
- Consider loading tree-sitter grammars dynamically instead of embedding all parse tables
- Only include language parsers that are actually needed
- Use `tree-sitter` feature flags more granularly

### 2. **String Optimization** (Potential savings: ~300-500 KiB)
- Review string literals in code (`__cstring` section is 329 KiB in release)
- Consider using `&'static str` constants instead of `String` where possible
- Use string interning for repeated strings

### 3. **Code Size Optimization** (Potential savings: 1-2 MiB)
- Enable more aggressive LTO: `lto = "fat"` (currently `lto = "thin"` in release-fast)
- Use `opt-level = "z"` or `opt-level = "s"` for size optimization in non-critical paths
- Review large constant data structures (like `base_function_declarations` at 192 KiB)

### 4. **Debug Info Management**
- The `bloaty` profile has been added to Cargo.toml for analysis builds
- Consider using `split-debuginfo` for production to reduce binary size while keeping debug info separate

### 5. **Dependency Audit**
- Run `cargo bloat` (different tool) to identify which dependencies contribute most to binary size
- Consider replacing heavy dependencies with lighter alternatives where possible

---

## Bloaty Profile Added

A new Cargo profile has been added for bloaty analysis:

```toml
[profile.bloaty]
inherits = "release"
debug = true
strip = false
```

Build with: `cargo build --profile bloaty`

This creates an optimized binary with debug symbols for detailed analysis.

---

## How to Use Bloaty

```bash
# Install
brew install bloaty

# Basic analysis
bloaty -d segments ./target/release-fast/vtcode
bloaty -d sections ./target/release-fast/vtcode
bloaty -d symbols ./target/release-fast/vtcode

# With debug info (for compileunits)
cargo build --profile bloaty
bloaty -d compileunits ./target/bloaty/vtcode
bloaty -d inlines ./target/bloaty/vtcode

# Filter specific patterns
bloaty -d symbols ./target/debug/vtcode --source-filter="tree_sitter"
bloaty -d symbols ./target/debug/vtcode --source-filter="vtcode_core"

# Show top N symbols
bloaty -d symbols -n 50 ./target/release-fast/vtcode

# CSV output for further analysis
bloaty -d sections --csv ./target/release-fast/vtcode > sections.csv
```

---

## Conclusion

The vtcode binary is **32 MiB** in release-fast mode, which is reasonable for a Rust application with:
- Multiple LLM provider integrations
- Tree-sitter code parsing
- Terminal UI (ratatui)
- Async runtime (tokio)

**Main size contributors:**
1. **Tree-sitter parsers** (~5 MiB in debug, likely ~2-3 MiB in release)
2. **Code complexity** (70% of release binary is actual code)
3. **Static constants** (23% of release binary)

**Potential optimizations** could reduce the binary by 10-20% without significant functionality loss, primarily through:
- Dynamic loading of tree-sitter grammars
- More aggressive LTO
- Code size optimization for non-critical paths
