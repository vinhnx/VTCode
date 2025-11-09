# Phase 2 Quick Start - Implementation Guide

## Current State

✅ **Phase 1 Complete**:
- `anstyle-git`, `anstyle-ls`, `anstyle-query` dependencies already added
- `InlineTextStyle` modernized with full Effects and background color support
- `ThemeConfigParser` module already created with Git/LS parsing functions
- All core styling infrastructure in place

## What You'll Build

### Component 1: Git Config Color Parser
**File**: `vtcode-core/src/ui/git_config.rs` (NEW, ~150 lines)

```rust
use anstyle::Style;
use anstyle_git;
use std::path::Path;
use anyhow::Result;

/// Parsed Git configuration colors for diff/status
pub struct GitColorConfig {
    pub diff_new: Style,
    pub diff_old: Style,
    pub diff_context: Style,
    pub diff_header: Style,
    pub status_added: Style,
    pub status_modified: Style,
    pub status_deleted: Style,
}

impl GitColorConfig {
    /// Load colors from .git/config file
    pub fn from_git_config(config_path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(config_path)?;
        
        // Parse [color "diff"] section
        let diff_new = Self::extract_git_color(&content, "diff", "new")?;
        let diff_old = Self::extract_git_color(&content, "diff", "old")?;
        // ... etc
    }
    
    fn extract_git_color(
        content: &str,
        section: &str,
        key: &str,
    ) -> Result<Style> {
        // Find: [color "section"] key = value
        // Parse with anstyle_git::parse()
    }
}
```

**Integration Point**: `diff_renderer.rs`
```rust
// Instead of GitDiffPalette::new(), use:
let git_colors = GitColorConfig::from_git_config(".git/config")?;
```

### Component 2: File Type Colorizer
**File**: `vtcode-core/src/ui/file_colorizer.rs` (NEW, ~200 lines)

```rust
use anstyle::Style;
use anstyle_ls;
use std::path::Path;

/// Applies LS_COLORS to files in listings
pub struct FileColorizer {
    ls_colors: Option<String>,
}

impl FileColorizer {
    pub fn new() -> Self {
        Self {
            ls_colors: std::env::var("LS_COLORS").ok(),
        }
    }
    
    /// Get style for a file path
    pub fn style_for_file(&self, path: &Path) -> Option<Style> {
        let ls_colors = self.ls_colors.as_ref()?;
        
        let file_type = match path {
            p if p.is_dir() => "di",      // directory
            p if p.is_symlink() => "ln",  // symlink
            _ => {
                // Try extension matching: *.rs, *.toml, etc
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| format!("*.{}", ext))
                    .as_deref()
                    .unwrap_or("fi")
            }
        };
        
        // Parse and extract style for this file type
        parse_ls_colors_for_type(ls_colors, file_type)
    }
}
```

**Integration Point**: `file_palette.rs`
```rust
let colorizer = FileColorizer::new();
// When rendering files:
if let Some(style) = colorizer.style_for_file(&file_path) {
    // Convert to ratatui style and apply
}
```

### Component 3: Custom Theme Configuration
**File**: `vtcode-core/src/config/theme_config.rs` (NEW, ~180 lines)

**Schema** (add to `vtcode.toml`):
```toml
[theme]
source = "system"  # system, custom, git, or auto-detect

[theme.colors.cli]
success = "green"
error = "red"
warning = "yellow"

[theme.colors.diff]
added = "green on dark_green"
removed = "red on dark_red"
header = "cyan bold"

[theme.colors.status]
modified = "yellow"
added = "green"
deleted = "red"
```

**Implementation**:
```rust
use anstyle::Style;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ThemeConfig {
    pub source: ThemeSource,
    pub colors: ThemeColors,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ThemeColors {
    pub cli: CliColors,
    pub diff: DiffColors,
    pub status: StatusColors,
}

impl ThemeConfig {
    /// Load from .vtcode/theme.toml
    pub fn from_file(path: &Path) -> Result<Self> {
        // Use toml crate (already in Cargo.toml)
    }
    
    /// Merge multiple sources (system < git < custom)
    pub fn merge(mut self, other: Self) -> Self {
        // Override with non-default values from other
    }
}
```

## Step-by-Step Implementation

### Step 1: Research & Understand (30 mins)
- Read `docs/styling/anstyle-crates-research.md` sections on Git/LS parsing
- Run `cargo doc --open` and explore `anstyle_git` and `anstyle_ls` APIs
- Review `ThemeConfigParser` in `vtcode-core/src/ui/tui/theme_parser.rs`

### Step 2: Git Config Parser (1.5 hours)
1. Create `vtcode-core/src/ui/git_config.rs`
2. Implement `GitColorConfig` struct
3. Add parsing logic using `anstyle_git::parse()`
4. Add tests:
   - Parse valid Git config colors
   - Handle missing colors (fallback to defaults)
   - Verify ANSI code output matches Git
5. Integrate into `diff_renderer.rs` to use parsed colors

### Step 3: File Colorizer (1 hour)
1. Create `vtcode-core/src/ui/file_colorizer.rs`
2. Implement `FileColorizer` struct
3. Add file type detection logic
4. Implement LS_COLORS parsing and caching
5. Add tests for common file types
6. Integrate into file picker (if exists: `file_palette.rs`)

### Step 4: Theme Config (1 hour)
1. Create `vtcode-core/src/config/theme_config.rs`
2. Define TOML schema structures
3. Implement file loading and parsing
4. Add merge logic for multiple sources
5. Create example theme file in `examples/`
6. Document in `docs/styling/THEME_CONFIGURATION.md`

### Step 5: Testing & Integration (1 hour)
1. Run `cargo test` - all tests pass
2. Run `cargo clippy` - no new warnings
3. Add integration tests combining all components
4. Verify no visual regressions in TUI

## Key Files to Reference

| File | Purpose | Key Types |
|------|---------|-----------|
| `vtcode-core/src/ui/tui/theme_parser.rs` | Parsing API | `ThemeConfigParser` |
| `vtcode-core/src/utils/style_helpers.rs` | Style factory | `ColorPalette`, `render_styled()` |
| `vtcode-core/src/ui/diff_renderer.rs` | Integration point | `DiffRenderer` |
| `docs/styling/anstyle-crates-research.md` | Reference | Code examples |

## Testing Checklist

- [ ] Git config with all color sections parses correctly
- [ ] Invalid Git config falls back gracefully
- [ ] LS_COLORS environment variable is read
- [ ] File types are detected (dir, symlink, extensions)
- [ ] Custom theme file loads and merges
- [ ] Multiple theme sources stack correctly
- [ ] All unit tests pass
- [ ] Integration tests pass
- [ ] No clippy warnings
- [ ] No visual regressions in TUI

## Build & Test Commands

```bash
# During development
cargo check                          # Quick compile check
cargo test vtcode_ui                 # Run UI tests only
cargo clippy --lib                   # Lint check

# Final validation
cargo test                           # All tests
cargo clippy                         # Full lint
cargo fmt --check                    # Format check
```

## Expected Outcomes

After Phase 2 completion:

1. **Git Diff Colors**: Respects `.git/config` [color "diff"] settings
   - Files like `git diff` visually
   - Support for hex colors (#0000ee), named colors, and effects

2. **File Listing Colors**: Respects system LS_COLORS
   - Directory colors work as expected
   - File extension colors match system preferences
   - Symlinks styled distinctly

3. **Custom Themes**: Support via TOML configuration
   - Users can customize colors without code changes
   - Multiple sources can be merged (system + custom)
   - Easy to add new color sections (cli, diff, status, etc)

## Common Patterns

### Using ThemeConfigParser (Already Created)
```rust
use vtcode_core::ui::tui::ThemeConfigParser;

// Parse Git style string
let style = ThemeConfigParser::parse_git_style("bold red")?;

// Parse LS_COLORS code
let style = ThemeConfigParser::parse_ls_colors("01;34")?;
```

### Converting to Ratatui
```rust
use vtcode_core::utils::ratatui_styles::anstyle_to_ratatui;

let anstyle = style;
let ratatui_style = anstyle_to_ratatui(anstyle);
// Use ratatui_style in TUI rendering
```

## Potential Issues & Solutions

| Issue | Solution |
|-------|----------|
| Git config has complex INI format | Use regex or ini parser crate |
| LS_COLORS uses special codes (e.g., orphaned links) | Handle gracefully, fallback to default |
| Performance of repeated file checks | Cache FileColorizer instance in session |
| Windows doesn't have LS_COLORS | Check OS, graceful no-op on Windows |
| User's theme file has syntax error | Validate on load, provide error message |

## Resources

- [anstyle-git documentation](https://docs.rs/anstyle-git/latest/anstyle_git/)
- [anstyle-ls documentation](https://docs.rs/anstyle-ls/latest/anstyle_ls/)
- [Git Color Configuration](https://git-scm.com/book/en/v2/Git-Customization-Git-Configuration#Colors)
- [LS_COLORS Format](https://linux.die.net/man/5/dir_colors)
- [Existing Phase 1 Code](PHASE1_COMPLETION_SUMMARY.md)

---

**Total Estimated Time**: 4-5 hours for complete implementation with tests

Start with Phase 2.1 (Git Config) as it has the highest value and is well-defined.
