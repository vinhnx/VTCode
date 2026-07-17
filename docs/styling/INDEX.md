# VT Code Styling System - Complete Documentation Index

## Quick Navigation

### For Users

- **[Quick Start](STYLING_QUICK_START.md)** - How to use styling in your code

### For Understanding the System

- **[Architecture](ARCHITECTURE.md)** - System design and components
- **[Research](anstyle-crates-research.md)** - Technical deep-dive on crates

## Implementation Status

### Phase 1: Foundation COMPLETE

**Timeline**: November 2025
**Status**: Production-ready

Achievements:

- Upgraded `anstyle-git` and `anstyle-ls` crates
- Modernized `InlineTextStyle` with Effects support
- Added background color support
- Created `ThemeConfigParser` module
- Updated all call sites (20+ locations)
- Comprehensive test coverage (14+ tests)

**Key Files**:

- `crates/codegen/vtcode-core/src/ui/tui/types.rs` - InlineTextStyle struct
- `crates/codegen/vtcode-core/src/ui/tui/style.rs` - Style conversion functions
- `crates/codegen/vtcode-core/src/ui/tui/theme_parser.rs` - Config parsing
- `crates/codegen/vtcode-core/src/utils/style_helpers.rs` - Style factories
- `crates/codegen/vtcode-core/src/utils/diff_styles.rs` - Diff color palettes

## Document Guide

| Document                                 | Purpose                 | Audience               |
| ---------------------------------------- | ----------------------- | ---------------------- |
| [README.md](README.md)                   | Overview and navigation | Everyone               |
| [ARCHITECTURE.md](ARCHITECTURE.md)       | System design           | Architects/maintainers |
| [STYLING_QUICK_START.md](STYLING_QUICK_START.md) | Usage guide    | Developers             |
| [anstyle-crates-research.md](anstyle-crates-research.md) | Technical research | Advanced devs |
| [quick-reference.md](quick-reference.md) | Cheat sheets            | Quick lookup           |
| [styling_integration.md](styling_integration.md) | Integration patterns | Integration developers |

## Quick Reference

### Using Styled Output

```rust
// CLI output with colors
use vtcode_core::utils::colors::style;
println!("{}", style("Success").green());
println!("{}", style("Error").red().bold());

// TUI widgets
use vtcode_core::utils::style_helpers::{ColorPalette, render_styled};
use ratatui::text::Span;

let palette = ColorPalette::default();
let span = Span::styled("Text", palette.success);
```

### Converting Styles

```rust
use anstyle::Style;
use vtcode_core::utils::ratatui_styles::anstyle_to_ratatui;

let anstyle = Style::new().bold().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Green)));
let ratatui_style = anstyle_to_ratatui(anstyle);
```

### Parsing Configuration

```rust
use vtcode_core::ui::tui::ThemeConfigParser;

// Parse Git color syntax
let git_style = ThemeConfigParser::parse_git_style("bold red #ff0000")?;

// Parse LS_COLORS syntax
let ls_style = ThemeConfigParser::parse_ls_colors("01;34")?;

// Parse flexibly (tries Git first, then LS)
let style = ThemeConfigParser::parse_flexible("green")?;
```

## Key Crates

| Crate           | Version | Purpose                             |
| --------------- | ------- | ----------------------------------- |
| `anstyle`       | 1.0     | ANSI style abstraction              |
| `anstyle-git`   | 1.1     | Git color config parsing            |
| `anstyle-ls`    | 1.0     | LS_COLORS parsing                   |
| `anstyle-parse` | 0.2     | ANSI escape sequence parsing        |
| `anstyle-query` | 1.0     | Terminal color capability detection |
| `ratatui`       | 0.30    | TUI rendering                       |
| `catppuccin`    | 2.5     | Theme color palettes                |

All crates are in `vtcode-core/Cargo.toml`

## Architecture Overview

```
User Config (TOML)
    ↓
System Config (Git/LS_COLORS)
    ↓
ThemeConfigParser
    ↓
Style Objects (anstyle::Style)
     CLI Output (render)
     TUI Output (ratatui conversion)
```

## Testing

Run styling tests:

```bash
cargo test vtcode_ui           # UI/styling tests
cargo test style_helpers       # Style helper tests
cargo test theme_parser        # Config parser tests
cargo test --lib              # All library tests
```

Run quality checks:

```bash
cargo clippy --lib            # Lint check
cargo fmt --check             # Format check
cargo check                   # Compile check
```

## Contributing

When adding new styling features:

1. **Follow the pattern**: Use `ColorPalette`, `style_from_color_name()`, `render_styled()`
2. **No hardcoded colors**: All colors go through helpers
3. **Add tests**: Unit tests for new functions
4. **Update docs**: Add to relevant doc files
5. **Run checks**: clippy, fmt, test must pass

## Performance Impact

- Phase 1: Zero impact (all Copy types, zero-cost abstractions)
- Overall: Negligible runtime overhead

## Support & Questions

For questions about:

- **How to use**: See [STYLING_QUICK_START.md](STYLING_QUICK_START.md)
- **Technical details**: See [anstyle-crates-research.md](anstyle-crates-research.md)
- **Architecture**: See [ARCHITECTURE.md](ARCHITECTURE.md)

---

**Last Updated**: November 9, 2025
**Status**: Phase 1 Complete
