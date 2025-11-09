# Architecture: Anstyle Integration in Vtcode

Visual diagrams and architectural details for the anstyle integration.

## System Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                     Input Sources                                │
├──────────────────────┬──────────────────────┬──────────────────┤
│  Git Config          │  LS_COLORS Env Var   │  Custom Config   │
│  .git/config         │  "di=01;34:ln=01:36"│  .vtcoderc       │
│                      │                      │                  │
│ [color "diff"]       │                      │ ---              │
│ new = green bold     │                      │                  │
│ old = red            │                      │                  │
└──────────────────────┴──────────────────────┴──────────────────┘
                             ↓
                    ┌────────┴────────┐
                    │                 │
         ┌──────────▼────────┐  ┌──────▼─────────┐
         │ anstyle-git 1.1   │  │ anstyle-ls 1.0 │
         │ Parse Git syntax  │  │ Parse ANSI     │
         │ "bold red"        │  │ codes "01;34"  │
         └──────────┬────────┘  └────────┬───────┘
                    │                    │
                    └────────┬───────────┘
                             ↓
                   ┌─────────────────────┐
                   │   anstyle::Style    │
                   │ (ANSI abstraction)  │
                   │                     │
                   │ - fg_color          │
                   │ - bg_color          │
                   │ - effects (bold,    │
                   │   dim, italic,      │
                   │   underline, etc)   │
                   └──────────┬──────────┘
                              ↓
         ┌────────────────────────────────────────┐
         │   vtcode-core/src/ui/tui/style.rs      │
         │   convert_style() function             │
         └──────────────────┬─────────────────────┘
                            ↓
         ┌────────────────────────────────────────┐
         │      InlineTextStyle (NEW)             │
         │                                        │
         │ ┌──────────────────────────────────┐  │
         │ │ color: Option<AnsiColorEnum>     │  │
         │ │ bg_color: Option<AnsiColorEnum> │  │  ← NEW
         │ │ effects: Effects (bitmask)       │  │  ← NEW
         │ └──────────────────────────────────┘  │
         └──────────────────┬─────────────────────┘
                            ↓
         ┌────────────────────────────────────────┐
         │   ratatui_style_from_inline()          │
         └──────────────────┬─────────────────────┘
                            ↓
         ┌────────────────────────────────────────┐
         │   ratatui::style::Style                │
         │                                        │
         │ - foreground: Color                    │
         │ - background: Color                    │
         │ - modifiers: Modifier (BOLD, DIM, etc)│
         └──────────────────┬─────────────────────┘
                            ↓
         ┌────────────────────────────────────────┐
         │   TUI Rendering (Terminal Output)      │
         │                                        │
         │   ┌──────────────────────────┐        │
         │   │ Terminal Escape Codes    │        │
         │   │ ESC[1m bold             │        │
         │   │ ESC[31m red             │        │
         │   │ ESC[44m blue bg         │        │
         │   └──────────────────────────┘        │
         └────────────────────────────────────────┘
```

---

## Data Flow: Style Parsing and Application

```
GitHub User Input Flow:
────────────────────────

User sets LS_COLORS env var
    │
    ├─→ "di=01;34:ln=01;36:ex=01;32"
    │
    ├─→ ThemeConfigParser::parse_ls_colors()
    │
    ├─→ anstyle_ls::parse() [crate function]
    │
    └─→ anstyle::Style { fg: blue, effects: BOLD }
            │
            └─→ convert_style()
                    │
                    └─→ InlineTextStyle {
                        color: Some(Blue),
                        bg_color: None,
                        effects: BOLD
                    }
                        │
                        └─→ ratatui_style_from_inline()
                                │
                                └─→ ratatui::Style {
                                    fg: Color::Blue,
                                    bg: None,
                                    modifiers: BOLD
                                }
                                    │
                                    └─→ Draw directory in terminal
                                        "bold blue text"
```

---

## Module Dependencies

```
vtcode-core
└── src/ui/
    ├── theme.rs (existing)
    │   └─ ThemePalette, ThemeStyles
    │
    ├── styled.rs (existing)
    │   └─ High-level style presets
    │
    ├── tui/
    │   ├── style.rs (MODIFIED)
    │   │   ├─ convert_style()       [updated]
    │   │   ├─ convert_ansi_color()  [existing]
    │   │   └─ ratatui_style_from_inline()  [updated]
    │   │
    │   ├── types.rs (MODIFIED)
    │   │   └─ InlineTextStyle {color, bg_color, effects}  [expanded]
    │   │
    │   ├── theme_parser.rs (NEW)
    │   │   └─ ThemeConfigParser
    │   │       ├─ parse_git_style()
    │   │       ├─ parse_ls_colors()
    │   │       └─ parse_flexible()
    │   │
    │   ├── mod.rs (MODIFIED)
    │   │   └─ pub mod theme_parser  [export new module]
    │   │
    │   ├── session/
    │   │   ├── file_palette.rs (FUTURE: Phase 3)
    │   │   └─ FileColorizer [not yet added]
    │   │
    │   └── tui.rs (existing)
    │
    └── diff_renderer.rs (FUTURE: Phase 2)
        └─ GitColorConfig [not yet added]

External Crates (Cargo.toml):
├── anstyle = "1.0"         (existing)
├── anstyle-git = "1.1"     (NEW in Phase 1)
├── anstyle-ls = "1.0"      (NEW in Phase 1)
├── anstyle-parse = "0.2"   (existing)
├── anstyle-crossterm = "4.0" (existing)
├── ratatui = "0.29"        (existing)
└── catppuccin = "2.5"      (existing)
```

---

## Effect Support Matrix

### Before Integration (Current)

| Effect | Supported | Remark |
|--------|-----------|--------|
| **Bold** | ✅ Yes | Explicit `bold: bool` field |
| **Italic** | ✅ Yes | Explicit `italic: bool` field |
| **Dim** | ❌ No | Not modeled |
| **Underline** | ❌ No | Not modeled |
| **Strikethrough** | ❌ No | Not modeled |
| **Reverse** | ❌ No | Not modeled |
| **Background Color** | ❌ No | InlineTextStyle has no `bg_color` |

### After Integration (Phase 1)

| Effect | Supported | Remark |
|--------|-----------|--------|
| **Bold** | ✅ Yes | From `Effects::BOLD` bitmask |
| **Italic** | ✅ Yes | From `Effects::ITALIC` bitmask |
| **Dim** | ✅ Yes | From `Effects::DIMMED` bitmask |
| **Underline** | ✅ Yes | From `Effects::UNDERLINE` bitmask |
| **Strikethrough** | ✅ Yes | From `Effects::STRIKETHROUGH` bitmask |
| **Reverse** | ✅ Yes | From `Effects::REVERSE` bitmask |
| **Background Color** | ✅ Yes | New `bg_color: Option<AnsiColorEnum>` field |

---

## InlineTextStyle Evolution

### Current (Pre-Integration)
```rust
pub struct InlineTextStyle {
    pub color: Option<AnsiColorEnum>,
    pub bold: bool,        // ← Limited effect modeling
    pub italic: bool,      // ← Only 2 effects
}
```

### After Phase 1
```rust
pub struct InlineTextStyle {
    pub color: Option<AnsiColorEnum>,          // Foreground
    pub bg_color: Option<AnsiColorEnum>,       // ← NEW: Background
    pub effects: Effects,                      // ← NEW: Full bitmask
}
```

**Benefits**:
- All ANSI effects supported
- Aligns with upstream `anstyle` crate design
- Composable effects: `Effects::BOLD | Effects::UNDERLINE`
- One field (`effects`) vs. many bool fields (future-proof)

---

## Parsing Flow Comparison

### Current (Manual Parsing)
```
Hard-coded colors → ThemePalette → convert_ansi_color() → InlineTextStyle
                 (no parsing)    (limited)
```

### After Integration
```
Git Config / LS_COLORS → anstyle-git/anstyle-ls → anstyle::Style → convert_style() → InlineTextStyle
  (user config)            (parse & interpret)    (standard)       (enhanced)
```

---

## Call Site Impact Analysis

### Files with InlineTextStyle Creation

| File | Lines | Change | Impact |
|------|-------|--------|--------|
| `session/input.rs` | 215-217 | Update style field access | Low - Simple field rename |
| `session/header.rs` | 234, 261 | Add to style tuple | Low - Tuple context |
| `session/navigation.rs` | 251, 286 | Update fallback logic | Low - Conditional changes |
| `session/slash.rs` | 371, 380 | Update color merge | Low - Method calls |
| `types.rs` | 82-97 | Core struct expansion | Medium - Must update default() |

**Total Call Sites**: ~15-20 across 5-6 files
**Risk Level**: Low (mostly mechanical updates)

---

## Configuration Resolution Priority (Future: Phase 2+)

When styling a file or element, resolve colors in this order:

```
1. Explicit style (e.g., error message → red)
   ↓ (not specified)
2. LS_COLORS (if available on system)
   ↓ (not available or key not found)
3. Git config [color] section
   ↓ (not configured)
4. Vtcode theme (ThemePalette)
   ↓ (missing)
5. Terminal default color
```

This allows layered customization: system colors → git colors → vtcode theme → fallback.

---

## Backward Compatibility Plan

### Safe Migration Path
```
Step 1: Add new fields (bg_color, effects) to InlineTextStyle
Step 2: Update constructor to use new fields
Step 3: Create helper methods (bold(), italic(), etc.)
Step 4: Update all InlineTextStyle { ... } expressions
Step 5: Remove deprecated bold: bool, italic: bool fields
```

**Key**: Never break public API in step 1-4. Remove deprecated fields only after all internal code is updated.

### Testing Strategy
- Unit tests for each conversion function
- Integration tests for full pipeline (parse → convert → render)
- Visual regression tests (compare TUI output before/after)
- Terminal compatibility tests (different terminal emulators)

---

## Performance Characteristics

### Parsing Performance
```
anstyle-git::parse("bold red")     ~100 ns  (nanoseconds)
anstyle-ls::parse("01;34")         ~80 ns
convert_style(anstyle::Style)      ~50 ns
ratatui_style_from_inline()        ~30 ns
                                   ─────────
Total style pipeline:              ~260 ns per call
```

### Caching Strategy
```
Immutable sources (cache forever):
├─ Git config colors      → Cache in lazy_static
├─ Theme definitions      → Already cached
└─ LS_COLORS env var      → Cache at startup

Hot path (no caching needed):
├─ Per-render style application → Too fast to matter
└─ Dynamic style merging        → Rare operation
```

---

## Error Handling Flow

```
User Input (config string)
    │
    ├─→ ThemeConfigParser::parse_git_style()
    │       │
    │       ├─ Success → anstyle::Style
    │       │
    │       └─ Error → anyhow::Error
    │               │
    │               ├─→ Log warning
    │               ├─→ Fallback to previous style
    │               └─→ Continue rendering (graceful degradation)
    │
    └─→ Never panic, always render something
```

**Design principle**: Styling should never crash the TUI. Invalid colors → fallback → continue.

---

## Testing Architecture

```
Unit Tests (vtcode-core/src/ui/tui/)
├── test_convert_style
│   ├─ Bold color conversion
│   ├─ Background color handling
│   └─ Effect bitmask handling
│
├── test_ratatui_style_from_inline
│   ├─ Color mapping to ratatui
│   ├─ Modifier application
│   └─ Fallback color logic
│
└── test_theme_parser
    ├─ parse_git_style (valid inputs)
    ├─ parse_git_style (error cases)
    ├─ parse_ls_colors (valid inputs)
    ├─ parse_ls_colors (error cases)
    └─ parse_flexible (fallback logic)

Integration Tests (vtcode-core/examples/)
├── style_parsing.rs
│   └─ Full pipeline: parse → convert → render
│
└── theme_integration.rs
    └─ Real .git/config parsing & application

Visual Regression Tests (manual)
├── Compare TUI output (before/after)
├─ File browser coloring
├─ Diff display colors
├─ Error message styling
└─ Different terminal emulators (iTerm2, Terminal.app, Linux)
```

---

## Implementation Sequence

```
Phase 1: Foundation (2-3 hours)
└─ Day 1: Dependencies + Types
   ├─ Cargo.toml (anstyle-git, anstyle-ls)
   └─ types.rs (InlineTextStyle expansion)

   Day 2: Parsing + Conversion
   ├─ theme_parser.rs (new module)
   └─ style.rs (convert_style, ratatui_style_from_inline)

   Day 3: Integration + Tests
   ├─ Update all call sites
   ├─ Write unit tests
   └─ Validate with cargo test + clippy

Phase 2: Integration (2-3 hours)
└─ Week 2: Git Color Config
   ├─ diff_renderer.rs (GitColorConfig)
   ├─ session/header.rs (Git status colors)
   └─ Integration tests

Phase 3: Features (3-4 hours)
└─ Week 3: System Colors
   ├─ session/file_palette.rs (FileColorizer)
   ├─ LS_COLORS env var parsing
   └─ Config file support (.vtcoderc)
```

---

## Deployment Checklist

```
Pre-Deployment
☐ All unit tests pass
☐ Clippy warnings = 0
☐ Code review approved
☐ No performance regressions
☐ Manual TUI testing on 3+ terminals

Deployment
☐ Merge to main
☐ Update CHANGELOG.md
☐ Tag release version
☐ Build release artifacts

Post-Deployment
☐ Monitor for crash reports
☐ Verify styling on different terminals
☐ Gather user feedback
☐ Document any gotchas or quirks
```

---

## Related Documentation

- Implementation guide: `implementation-phase1.md`
- Crate research: `anstyle-crates-research.md`
- Quick reference: `quick-reference.md`
- Executive summary: `EXECUTIVE_SUMMARY.md`

---

**Last Updated**: Nov 9, 2025  
**Status**: Research & Design Complete, Ready for Phase 1 Implementation
