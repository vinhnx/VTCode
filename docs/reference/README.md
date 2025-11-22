# ANSI Escape Sequences Documentation Index

## Overview

This directory contains comprehensive documentation on ANSI escape sequences and their usage in VTCode.

## Documents

### 1. [ansi-escape-sequences.md](./ansi-escape-sequences.md)
**Complete ANSI Reference**

Comprehensive reference covering:
- All ANSI escape sequence types (CSI, OSC, DCS)
- Cursor control sequences
- Erase functions
- Color codes (8/16/256/RGB)
- Screen modes and private modes
- Keyboard strings
- Full specification with examples

**Use this when**: You need to understand what a specific ANSI sequence does or find the right sequence for a terminal operation.

### 2. [ansi-in-vtcode.md](./ansi-in-vtcode.md)
**VTCode Implementation Guide**

Detailed guide on how VTCode uses ANSI sequences:
- Module overview (ansi_parser, anstyle_utils, ansi renderer)
- PTY output processing flow
- TUI rendering with Ratatui
- Color and style mapping
- Best practices and patterns
- Testing strategies

**Use this when**: You're working on VTCode features that involve ANSI handling, PTY output, or TUI rendering.

### 3. [ansi-quick-reference.md](./ansi-quick-reference.md)
**Developer Quick Reference**

Quick lookup for common sequences:
- Most-used ANSI codes
- VTCode-specific patterns
- Regex patterns for matching
- Common mistakes and solutions
- Debugging tips
- Performance optimization

**Use this when**: You need a quick reminder of common ANSI codes or patterns while coding.

## Quick Navigation

### By Task

**Stripping ANSI codes**:
- Implementation: `vtcode-core/src/utils/ansi_parser.rs`
- Guide: [ansi-in-vtcode.md#ansi-parser](./ansi-in-vtcode.md#1-ansi-parser-vtcode-coresrcutilsansi_parserrs)
- Quick ref: [ansi-quick-reference.md#stripping-ansi](./ansi-quick-reference.md#stripping-ansi)

**Converting ANSI to Ratatui styles**:
- Implementation: `vtcode-core/src/utils/anstyle_utils.rs`
- Guide: [ansi-in-vtcode.md#ansi-style-utilities](./ansi-in-vtcode.md#2-ansi-style-utilities-vtcode-coresrcutilsanstyle_utilsrs)
- Quick ref: [ansi-quick-reference.md#vtcode-usage-examples](./ansi-quick-reference.md#vtcode-usage-examples)

**Understanding ANSI sequences**:
- Full reference: [ansi-escape-sequences.md](./ansi-escape-sequences.md)
- Common patterns: [ansi-quick-reference.md#common-patterns](./ansi-quick-reference.md#common-patterns-in-pty-output)

**PTY output processing**:
- Flow diagram: [ansi-in-vtcode.md#pty-output-processing](./ansi-in-vtcode.md#pty-output-processing)
- Implementation: `vtcode-core/src/tools/pty.rs`

**TUI rendering**:
- Color mapping: [ansi-in-vtcode.md#tui-rendering](./ansi-in-vtcode.md#tui-rendering)
- Effects mapping: [ansi-in-vtcode.md#effects-mapping](./ansi-in-vtcode.md#effects-mapping)

### By Use Case

**I need to...**

- **Remove ANSI codes from text**
  → [ansi-quick-reference.md#stripping-ansi](./ansi-quick-reference.md#stripping-ansi)

- **Understand what `\x1b[31m` means**
  → [ansi-escape-sequences.md#8-16-colors](./ansi-escape-sequences.md#8-16-colors)

- **Convert ANSI styles for TUI**
  → [ansi-in-vtcode.md#ansi-style-utilities](./ansi-in-vtcode.md#2-ansi-style-utilities-vtcode-coresrcutilsanstyle_utilsrs)

- **Debug ANSI-related issues**
  → [ansi-quick-reference.md#debugging-ansi-issues](./ansi-quick-reference.md#debugging-ansi-issues)

- **Process PTY output correctly**
  → [ansi-in-vtcode.md#pty-output-processing](./ansi-in-vtcode.md#pty-output-processing)

- **Add color to terminal output**
  → [ansi-escape-sequences.md#colors-graphics-mode](./ansi-escape-sequences.md#colors--graphics-mode)

- **Control cursor position**
  → [ansi-escape-sequences.md#cursor-controls](./ansi-escape-sequences.md#cursor-controls)

- **Clear screen or lines**
  → [ansi-escape-sequences.md#erase-functions](./ansi-escape-sequences.md#erase-functions)

- **Use alternative screen buffer**
  → [ansi-escape-sequences.md#common-private-modes](./ansi-escape-sequences.md#common-private-modes)

## Code Examples

### Strip ANSI from PTY output
```rust
use vtcode_core::utils::ansi_parser::strip_ansi;

let pty_output = "\x1b[32mCompiling\x1b[0m vtcode";
let clean = strip_ansi(pty_output);
// clean == "Compiling vtcode"
```

### Convert ANSI to Ratatui style
```rust
use vtcode_core::utils::anstyle_utils::ansi_style_to_ratatui_style;

let ansi_style = anstyle::Style::new()
    .fg_color(Some(AnsiColor::Red))
    .bold();
let ratatui_style = ansi_style_to_ratatui_style(ansi_style);
```

### Render colored text
```rust
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

let mut renderer = AnsiRenderer::stdout();
renderer.render("Success!", MessageStyle::Success);
// Outputs green text
```

## Testing

All ANSI-related code has comprehensive tests:

- **Parser tests**: `vtcode-core/src/utils/ansi_parser.rs`
- **Style conversion tests**: `vtcode-core/src/utils/anstyle_utils.rs`
- **Integration tests**: `vtcode-core/src/tools/registry/executors.rs`

Run tests:
```bash
cargo nextest run --package vtcode-core ansi
```

## External Resources

- [Wikipedia: ANSI escape code](https://en.wikipedia.org/wiki/ANSI_escape_code)
- [XTerm Control Sequences](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html)
- [VT100 Terminal Manuals](https://vt100.net/)
- [Build your own CLI with ANSI](http://www.lihaoyi.com/post/BuildyourownCommandLinewithANSIescapecodes.html)

## Contributing

When adding new ANSI-related features:

1. **Check existing utilities** - Don't reinvent the wheel
2. **Add tests** - All ANSI handling should be tested
3. **Update docs** - Add examples to relevant guides
4. **Follow patterns** - Use `strip_ansi()` for cleaning, `ansi_style_to_ratatui_style()` for conversion

## Summary

| Document | Purpose | When to Use |
|----------|---------|-------------|
| [ansi-escape-sequences.md](./ansi-escape-sequences.md) | Complete ANSI spec | Reference lookup |
| [ansi-in-vtcode.md](./ansi-in-vtcode.md) | VTCode implementation | Feature development |
| [ansi-quick-reference.md](./ansi-quick-reference.md) | Quick lookup | Daily coding |

---

**Last Updated**: 2025-11-22  
**Maintainer**: VTCode Team
