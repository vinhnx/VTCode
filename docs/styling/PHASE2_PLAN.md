# Phase 2 - Git Config Integration & Advanced Features

**Status**: Planning  
**Date**: November 9, 2025  
**Scope**: Integrate anstyle-git/ls ecosystem for system-aware styling

## Overview

Phase 1 (Foundation) successfully modernized the core styling system with full Effects support and background colors. Phase 2 focuses on leveraging the anstyle-git and anstyle-ls crates to parse system configuration (Git colors, LS_COLORS) and apply them to vtcode's TUI.

## Key Deliverables

### 1. Git Config Color Parsing
**Goal**: Parse `.git/config` color settings for diff/status visualization

**Implementation**:
- Create `GitColorConfig` struct to hold parsed Git color settings
- Parse `[color "diff"]` section with new/old/context colors
- Parse `[color "status"]` section for staged/unstaged/untracked
- Parse `[color "branch"]` section for current/local/remote
- Integrate into diff renderer and status view

**Files to Create/Modify**:
- `vtcode-core/src/ui/git_config.rs` (NEW)
- `vtcode-core/src/ui/diff_renderer.rs` (use parsed colors)
- `vtcode-core/src/ui/status_view.rs` (if exists, use parsed colors)

**Test Cases**:
- Parse valid `.git/config` with color sections
- Handle missing color sections gracefully
- Fall back to defaults when parsing fails
- Verify ANSI code generation matches Git's output

### 2. LS_COLORS System Integration
**Goal**: Respect system LS_COLORS when displaying files

**Implementation**:
- Create `FileColorizer` struct to manage LS_COLORS parsing
- Extract file type from path (directory, symlink, etc.)
- Apply parsed LS_COLORS styles to file picker UI
- Cache parsed LS_COLORS for performance

**Files to Create/Modify**:
- `vtcode-core/src/ui/file_colorizer.rs` (NEW)
- `vtcode-core/src/ui/tui/session/file_palette.rs` (apply colors)
- `vtcode-core/src/utils/file_type.rs` (file type detection)

**Test Cases**:
- Parse LS_COLORS environment variable
- Apply directory style (di=01;34)
- Apply symlink style (ln=01;36)
- Apply file extension styles (*.rs, *.toml, etc.)
- Handle missing LS_COLORS gracefully

### 3. Theme Configuration File Support
**Goal**: Allow custom theme files with Git/LS syntax

**Implementation**:
- Support `.vtcode/theme.toml` configuration
- Define theme sections: [colors.cli], [colors.diff], [colors.status]
- Parse theme file on startup
- Merge with system LS_COLORS/Git config

**Files to Create/Modify**:
- `vtcode-core/src/config/theme_config.rs` (NEW)
- Update `vtcode.toml` schema documentation
- Add example theme file to `examples/`

**Test Cases**:
- Load and parse custom theme file
- Validate theme syntax
- Fall back to defaults on parse error
- Merge multiple sources (system + config)

## Architecture Overview

```
System Configuration
├── Git Config (.git/config)
│   └── anstyle-git::parse()
├── LS_COLORS (env var)
│   └── anstyle-ls::parse()
└── Custom Theme (.vtcode/theme.toml)
    └── Custom parser

        ↓
    ThemeConfigParser
        ↓
    Style Hierarchy
    ├── CLI Output (style_helpers.rs)
    ├── Diff Rendering (diff_renderer.rs)
    ├── Status View
    └── File Listing (file_colorizer.rs)
        ↓
    Output Rendering
    ├── Terminal (anstyle::render)
    └── TUI (ratatui conversion)
```

## Implementation Priority

### Phase 2.1 - Git Config (2-3 hours)
1. Create `GitColorConfig` struct
2. Implement Git config file parsing
3. Add integration tests
4. Update diff_renderer.rs to use parsed colors
5. Verify Git diff output matches system Git colors

### Phase 2.2 - LS_COLORS (1-2 hours)
1. Create `FileColorizer` struct
2. Implement LS_COLORS parsing
3. Add file type detection
4. Integrate into file_palette.rs
5. Test with various LS_COLORS configurations

### Phase 2.3 - Theme Config (2 hours)
1. Design TOML schema for theme configuration
2. Implement theme file parsing
3. Add merge logic for multiple sources
4. Document theme configuration
5. Add examples

## Dependencies

All required crates already in Cargo.toml:
- ✅ `anstyle-git = "1.1"`
- ✅ `anstyle-ls = "1.0"`
- ✅ `anstyle = "1.0"`
- ✅ `anstyle-query = "1.0"`

## Success Criteria

- [ ] All three components implemented and tested
- [ ] No breaking changes to public API
- [ ] Git diff colors respect `.git/config` settings
- [ ] File picker respects system LS_COLORS
- [ ] Custom theme files can override system settings
- [ ] All tests passing
- [ ] Clippy and fmt clean
- [ ] Documentation complete

## Known Limitations & Risks

### Medium Risk
- Git config parsing is complex (ini-like format)
  - Mitigation: Use regex for pattern matching
- LS_COLORS parsing edge cases (special file types)
  - Mitigation: Comprehensive test matrix
- Performance of repeated file type checks
  - Mitigation: Cache file colorizer instance

### Low Risk
- Breaking changes to existing APIs
  - Mitigation: Add new methods, deprecate old ones
- Terminal color capability detection
  - Mitigation: Graceful degradation

## References

- `docs/styling/anstyle-crates-research.md` - Detailed crate analysis
- `docs/styling/PHASE1_COMPLETION_SUMMARY.md` - Phase 1 baseline
- [anstyle-git docs](https://docs.rs/anstyle-git/latest/anstyle_git/)
- [anstyle-ls docs](https://docs.rs/anstyle-ls/latest/anstyle_ls/)
- [Git Color Configuration](https://git-scm.com/book/en/v2/Git-Customization-Git-Configuration#Colors)
- [LS_COLORS Format](https://linux.die.net/man/5/dir_colors)

## Estimated Timeline

- Phase 2.1 (Git Config): 2-3 hours
- Phase 2.2 (LS_COLORS): 1-2 hours  
- Phase 2.3 (Theme Config): 2 hours
- Testing & Integration: 1-2 hours

**Total**: 6-9 hours

## Notes

- Phase 1 foundation is solid and production-ready
- All required dependencies are already in place
- Excellent research documentation provides clear guidance
- Implementation can proceed incrementally with tests at each stage
