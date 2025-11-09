# Vtcode Styling System Documentation

This directory contains research and implementation guides for improving vtcode's terminal styling system using `anstyle-git` and `anstyle-ls` crates.

## Files

### 1. anstyle-crates-research.md
Comprehensive research on `anstyle-git` and `anstyle-ls` crates:
- What each crate does
- How they parse color configurations
- Current vtcode styling architecture
- Recommended improvements
- Benefits and risks
- Implementation priorities

**Start here** for understanding the strategic approach.

### 2. implementation-phase1.md
Step-by-step implementation guide for Phase 1 (Foundation):
- Update Cargo.toml dependencies
- Expand InlineTextStyle struct
- Create theme_parser module
- Update style conversion functions
- Testing and verification
- Rollback plan

**Read this** to implement Phase 1 changes.

## Quick Summary

### What We're Doing

Improving vtcode's styling system to:
1. Support **full ANSI text effects** (bold, dim, italic, underline, strikethrough, reverse)
2. **Parse standard configs**: Git `.git/config` colors and `LS_COLORS` environment variable
3. **Support background colors** in text styling
4. **Leverage ecosystem tools** instead of custom parsing code

### Key Benefits

| Aspect | Current | After |
|--------|---------|-------|
| Text Effects | bold, italic | bold, dim, italic, underline, strikethrough, reverse |
| Config Sources | Hard-coded themes | Git config, LS_COLORS, custom files |
| Background Colors | Unsupported | Full support |
| Code Quality | Custom parsing | Leverage `anstyle-git`, `anstyle-ls` |

### Timeline

- **Phase 1**: Foundation (Low risk, 1-2 hours)
- **Phase 2**: Integration (Medium risk, 2-3 hours)
- **Phase 3**: Features (High value, 3-4 hours)

## Architecture Overview

```
User Config / Environment
    ↓
anstyle-git / anstyle-ls Parsers
    ↓
anstyle::Style (abstract representation)
    ↓
convert_style() → InlineTextStyle (vtcode internal)
    ↓
ratatui_style_from_inline() → ratatui::style::Style
    ↓
TUI Rendering (Terminal Output)
```

## Dependencies

**Already in Cargo.toml:**
- `anstyle` (1.0)
- `anstyle-parse` (0.2)
- `anstyle-crossterm` (4.0)
- `anstyle-query` (1.0)

**To be added (Phase 1):**
- `anstyle-git` (1.1)
- `anstyle-ls` (1.0)

## Related Code Locations

- Theme definitions: `vtcode-core/src/ui/theme.rs`
- Style conversion: `vtcode-core/src/ui/tui/style.rs`
- Style types: `vtcode-core/src/ui/tui/types.rs`
- File coloring: `vtcode-core/src/ui/tui/session/file_palette.rs`
- Diff rendering: `vtcode-core/src/ui/diff_renderer.rs`
- Main theme: `vtcode-core/src/ui/styled.rs`

## Getting Started

1. Read `anstyle-crates-research.md` for context
2. Follow `implementation-phase1.md` for hands-on steps
3. Run tests after each change
4. Move to Phase 2 once Phase 1 is complete

## References

- [anstyle-git crate docs](https://docs.rs/anstyle-git/)
- [anstyle-ls crate docs](https://docs.rs/anstyle-ls/)
- [anstyle crate docs](https://docs.rs/anstyle/)
- [Git color configuration](https://git-scm.com/book/en/v2/Git-Customization-Git-Configuration#Colors)
- [LS_COLORS format](https://linux.die.net/man/5/dir_colors)
- [WCAG Contrast Requirements](https://www.w3.org/WAI/WCAG21/Understanding/contrast-minimum.html)

## Notes

- All improvements are **backward compatible** if implemented with care
- Tests should be added incrementally
- Ratatui may not support all effects on all terminals (graceful degradation expected)
- Windows support for LS_COLORS is optional (graceful no-op)

## Questions?

For implementation questions, refer to:
- Phase 1 checklist in `implementation-phase1.md`
- Test examples in same file
- Existing code patterns in `vtcode-core/src/ui/`
