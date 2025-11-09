# Phase 3: Code Deduplication - Implementation Plan

## Overview

Phase 3 focuses on eliminating duplicated code patterns that proliferate throughout the session.rs file. Based on analysis of the codebase, we've identified ~300 lines of duplicated code (6% of the file) that can be consolidated through generic abstractions and helper utilities.

**Target:** Reduce duplication to <1% while maintaining backward compatibility and performance.

**Estimated Effort:** 4-5 hours across multiple sub-phases

## Current Status

**Previous Phase (Phase 2):**
- ✓ InputManager integrated and tested
- ✓ ScrollManager integrated and tested  
- ✓ Event handler refactoring (scroll down/up extraction)
- ✓ emit_inline_event() helper created
- Tests passing: 112+

**This Phase (Phase 3):**
- [ ] Generic PaletteRenderer<T> (HIGH IMPACT - ~200 lines)
- [ ] ToolStyler consolidation (MEDIUM IMPACT - ~150 lines)
- [ ] StyleHelpers extraction (LOW IMPACT - ~50 lines)
- [ ] Message renderer extraction (DEFERRED to Phase 4)

---

## 3.1 Generic PaletteRenderer<T> Extraction

### Problem Analysis

**File palette rendering (lines 585-714):**
```rust
fn render_file_palette(&mut self, frame: &mut Frame, viewport: Rect) {
    let Some(palette) = self.file_palette.as_ref() else {
        return;
    };
    
    let mut list_state = ListState::default();
    list_state.select(Some(palette.selected_index));
    
    let items: Vec<ListItem> = palette
        .current_page_items()
        .iter()
        .map(|entry| {
            let style = if entry.selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            ListItem::new(entry.display_name.clone()).style(style)
        })
        .collect();
    
    let list = List::new(items)
        .block(Block::default().title("Files").borders(Borders::ALL))
        .style(Style::default());
    
    frame.render_stateful_widget(list, viewport, &mut list_state);
}
```

**Prompt palette rendering (lines 783-877):**
```rust
fn render_prompt_palette(&mut self, frame: &mut Frame, viewport: Rect) {
    let Some(palette) = self.prompt_palette.as_ref() else {
        return;
    };
    
    let mut list_state = ListState::default();
    list_state.select(Some(palette.selected_index));
    
    let items: Vec<ListItem> = palette
        .current_page_items()
        .iter()
        .map(|entry| {
            let style = if entry.selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            ListItem::new(entry.display_name.clone()).style(style)
        })
        .collect();
    
    let list = List::new(items)
        .block(Block::default().title("Prompts").borders(Borders::ALL))
        .style(Style::default());
    
    frame.render_stateful_widget(list, viewport, &mut list_state);
}
```

**Issue:** 95% code overlap - only differences are:
- Title: "Files" vs "Prompts"
- Palette source: `file_palette` vs `prompt_palette`
- Item styling logic is identical

**Loading states (lines 716-739, 880-903):**
```rust
fn render_file_palette_loading(&mut self, frame: &mut Frame, viewport: Rect) {
    let paragraph = Paragraph::new("Loading files...")
        .style(Style::default());
    frame.render_widget(paragraph, viewport);
}

fn render_prompt_palette_loading(&mut self, frame: &mut Frame, viewport: Rect) {
    let paragraph = Paragraph::new("Loading prompts...")
        .style(Style::default());
    frame.render_widget(paragraph, viewport);
}
```

**Impact:** ~130 lines of duplicated rendering logic that's hard to maintain

### Solution Design

**Create trait for palette items:**
```rust
pub trait PaletteItem: Send + Sync {
    fn display_name(&self) -> String;
    fn selected(&self) -> bool;
    fn icon(&self) -> Option<&str> { None }
}

impl PaletteItem for FilePaletteEntry {
    fn display_name(&self) -> String { /* ... */ }
    fn selected(&self) -> bool { /* ... */ }
}

impl PaletteItem for PromptPaletteEntry {
    fn display_name(&self) -> String { /* ... */ }
    fn selected(&self) -> bool { /* ... */ }
}
```

**Create generic renderer:**
```rust
pub struct PaletteRenderer<T: PaletteItem> {
    items: Vec<T>,
    selected_index: usize,
    page_size: usize,
    title: String,
}

impl<T: PaletteItem> PaletteRenderer<T> {
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let mut list_state = ListState::default();
        list_state.select(Some(self.selected_index));
        
        let items: Vec<ListItem> = self.items
            .iter()
            .take(self.page_size)
            .map(|entry| {
                let style = if entry.selected() {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default()
                };
                ListItem::new(entry.display_name()).style(style)
            })
            .collect();
        
        let list = List::new(items)
            .block(Block::default().title(self.title.clone()).borders(Borders::ALL))
            .style(Style::default());
        
        frame.render_stateful_widget(list, area, &mut list_state);
    }
}
```

**Use in Session:**
```rust
// OLD: Two nearly identical functions
fn render_file_palette(&mut self, frame: &mut Frame, viewport: Rect) { /* ... */ }
fn render_prompt_palette(&mut self, frame: &mut Frame, viewport: Rect) { /* ... */ }

// NEW: Single generic call
fn render_palette<T: PaletteItem>(&self, frame: &mut Frame, area: Rect, 
                                   items: &[T], selected: usize, title: &str) {
    let renderer = PaletteRenderer {
        items: items.to_vec(),
        selected_index: selected,
        page_size: 10,
        title: title.to_string(),
    };
    renderer.render(frame, area);
}
```

### Implementation Steps

**Step 3.1.1: Define PaletteItem trait**
```bash
# Create in session/ directory or as inline module
# Time: 30 minutes
# Risk: Low - trait definition only
```

**Step 3.1.2: Implement for existing palette types**
```bash
# Impl PaletteItem for FilePaletteEntry
# Impl PaletteItem for PromptPaletteEntry
# Time: 30 minutes
# Risk: Low - straightforward impls
```

**Step 3.1.3: Create generic PaletteRenderer**
```bash
# Implement PaletteRenderer<T>
# Add render method with unified logic
# Time: 30 minutes
# Risk: Low - new functionality, not replacing yet
```

**Step 3.1.4: Update Session to use generic renderer**
```bash
# Replace render_file_palette with call to generic renderer
# Replace render_prompt_palette with call to generic renderer
# Run tests
# Time: 45 minutes
# Risk: Medium - replacing existing rendering
```

**Step 3.1.5: Remove duplicate functions**
```bash
# Delete old render_file_palette()
# Delete old render_prompt_palette()
# Delete old loading state rendering
# Time: 15 minutes
# Risk: Low - cleanup after migration
```

### Expected Outcomes

- **Code reduced:** ~130 lines
- **Functions reduced:** 4 → 1
- **Cyclomatic complexity:** Reduced by ~8
- **Reusability:** Can now add new palette types trivially
- **Testability:** Generic renderer can be tested independently

### Testing Strategy

```rust
#[cfg(test)]
mod palette_renderer_tests {
    use super::*;
    
    #[test]
    fn render_file_palette_via_generic() {
        // Test that file palette renders correctly
    }
    
    #[test]
    fn render_prompt_palette_via_generic() {
        // Test that prompt palette renders correctly
    }
    
    #[test]
    fn palette_selection_highlighting() {
        // Test that selection styling works
    }
}
```

---

## 3.2 ToolStyler Consolidation

### Problem Analysis

**Tool styling scattered across multiple functions (lines 1305-1435):**

1. **strip_tool_status_prefix()** (lines 1305-1314)
   ```rust
   fn strip_tool_status_prefix(text: &str) -> &str {
       const ICONS: &[&str] = &["✓", "✗", "⚠", "◌"];
       ICONS.iter()
           .find_map(|icon| text.strip_prefix(icon))
           .unwrap_or(text)
   }
   ```

2. **simplify_tool_display()** (lines 1317-1344)
   ```rust
   fn simplify_tool_display(display: &str) -> String {
       display
           .replace("executing", "")
           .replace("completed", "")
           .replace("failed", "")
           // ... many more replacements
   }
   ```

3. **format_tool_parameters()** (lines 1347-1378)
   ```rust
   fn format_tool_parameters(params: &str) -> String {
       params
           .lines()
           .map(|line| format!("  {}", line))
           .collect::<Vec<_>>()
           .join("\n")
   }
   ```

4. **normalize_tool_name()** (lines 1381-1393)
   ```rust
   fn normalize_tool_name(name: &str) -> String {
       match name {
           "bash_runner" => "shell",
           "file_browser" => "files",
           "grep_file" => "search",
           // ... hardcoded mappings
           _ => name,
       }
       .to_string()
   }
   ```

5. **tool_inline_style()** (lines 1395-1435)
   ```rust
   fn tool_inline_style(name: &str, theme: &InlineTheme) -> InlineTextStyle {
       let color = match name {
           "shell" => theme.primary,
           "files" => theme.secondary,
           // ... hardcoded color mappings
           _ => None,
       };
       InlineTextStyle { color, /* ... */ }
   }
   ```

**Issues:**
- 5 separate functions handling tool-related concerns
- Hardcoded icon list, status keywords, tool mappings
- No centralized configuration
- Difficult to add new tools or customize styling
- ~150 lines of scattered logic

### Solution Design

**Create ToolStyler struct:**
```rust
pub struct ToolStyler {
    status_icons: &'static [&'static str],
    status_keywords: &'static [&'static str],
    tool_name_map: HashMap<&'static str, &'static str>,
    theme: InlineTheme,
}

impl ToolStyler {
    pub fn new(theme: InlineTheme) -> Self {
        Self {
            status_icons: &["✓", "✗", "⚠", "◌"],
            status_keywords: &["executing", "completed", "failed"],
            tool_name_map: vec![
                ("bash_runner", "shell"),
                ("file_browser", "files"),
                ("grep_file", "search"),
            ].into_iter().collect(),
            theme,
        }
    }

    /// Strips status prefix from tool output
    pub fn strip_status(&self, text: &str) -> &str {
        self.status_icons.iter()
            .find_map(|icon| text.strip_prefix(icon))
            .unwrap_or(text)
    }

    /// Simplifies tool display text by removing status keywords
    pub fn simplify_display(&self, display: &str) -> String {
        let mut result = display.to_string();
        for keyword in self.status_keywords {
            result = result.replace(keyword, "");
        }
        result.trim().to_string()
    }

    /// Formats tool parameters with indentation
    pub fn format_parameters(&self, params: &str) -> String {
        params
            .lines()
            .map(|line| format!("  {}", line))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Normalizes tool name to display form
    pub fn normalize_name(&self, name: &str) -> String {
        self.tool_name_map
            .get(name)
            .copied()
            .unwrap_or(name)
            .to_string()
    }

    /// Gets style for tool name
    pub fn get_style(&self, name: &str) -> InlineTextStyle {
        let color = match name {
            "shell" => self.theme.primary,
            "files" => self.theme.secondary,
            // ... theme-based mappings
            _ => None,
        };
        InlineTextStyle { color, /* ... */ }
    }
}
```

**Usage in Session:**
```rust
// In render_tool_header_line():
let styler = ToolStyler::new(self.theme.clone());
let normalized = styler.normalize_name(tool_name);
let style = styler.get_style(&normalized);
let simplified = styler.simplify_display(line_text);
```

### Implementation Steps

**Step 3.2.1: Create ToolStyler struct (30 min)**
- Define ToolStyler with configuration fields
- Implement new() with default tool mappings
- Add unit tests for configuration

**Step 3.2.2: Migrate strip_status_prefix (15 min)**
- Create strip_status() method
- Update all call sites to use styler.strip_status()
- Remove old function

**Step 3.2.3: Migrate simplify_tool_display (15 min)**
- Create simplify_display() method
- Update call sites
- Remove old function

**Step 3.2.4: Migrate normalize_tool_name (15 min)**
- Create normalize_name() method
- Update call sites
- Remove old function

**Step 3.2.5: Migrate tool_inline_style (20 min)**
- Create get_style() method
- Update call sites
- Remove old function

**Step 3.2.6: Clean up and test (20 min)**
- Remove all old tool styling functions
- Run full test suite
- Verify no regressions

### Expected Outcomes

- **Code reduced:** ~80 lines
- **Functions reduced:** 5 → 1
- **Cyclomatic complexity:** Reduced by ~6
- **Maintainability:** Centralized tool configuration
- **Extensibility:** Easy to add new tools or customize colors

### Testing Strategy

```rust
#[cfg(test)]
mod tool_styler_tests {
    use super::*;
    
    #[test]
    fn strip_status_removes_icon() {
        let styler = ToolStyler::new(InlineTheme::default());
        assert_eq!(styler.strip_status("✓ completed"), "completed");
    }
    
    #[test]
    fn simplify_removes_keywords() {
        let styler = ToolStyler::new(InlineTheme::default());
        assert_eq!(styler.simplify_display("shell executing"), "shell");
    }
    
    #[test]
    fn normalize_maps_tool_names() {
        let styler = ToolStyler::new(InlineTheme::default());
        assert_eq!(styler.normalize_name("bash_runner"), "shell");
    }
}
```

---

## 3.3 StyleHelpers Extraction

### Problem Analysis

**Style conversion code scattered throughout rendering (lines scattered):**

```rust
// Pattern 1: Converting InlineTextStyle to ratatui Style (appears 40+ times)
let ratatui_style = Style::default()
    .fg(ratatui_color_from_ansi(inline_style.color))
    .add_modifier(/* ... */)
    .sub_modifier(/* ... */);

// Pattern 2: Creating spans (appears 80+ times)
Span::styled(text.to_string(), style)

// Pattern 3: Creating lines from spans (appears 50+ times)
Line::from(vec![span1, span2, span3])

// Pattern 4: Building paragraphs with style (appears 30+ times)
Paragraph::new(text)
    .style(style)
    .wrap(Wrap { trim: true })
```

**Issues:**
- Repeated boilerplate code
- Inconsistent style handling
- Hard to make global style changes
- ~50 lines of duplicated patterns

### Solution Design

**Create StyleHelpers module:**
```rust
pub mod style_helpers {
    use super::*;
    
    /// Convert InlineTextStyle to ratatui Style
    pub fn inline_to_ratatui(style: &InlineTextStyle, theme: &InlineTheme) -> Style {
        let mut s = Style::default();
        
        if let Some(color) = style.color {
            s = s.fg(ratatui_color_from_ansi(Some(color)));
        }
        
        if style.bold {
            s = s.add_modifier(Modifier::BOLD);
        }
        
        if style.italic {
            s = s.add_modifier(Modifier::ITALIC);
        }
        
        if style.underline {
            s = s.add_modifier(Modifier::UNDERLINED);
        }
        
        if style.dimmed {
            s = s.add_modifier(Modifier::DIM);
        }
        
        s
    }
    
    /// Create a styled span from text and InlineTextStyle
    pub fn span(text: impl Into<String>, style: &InlineTextStyle, theme: &InlineTheme) -> Span<'static> {
        let text = text.into();
        let ratatui_style = inline_to_ratatui(style, theme);
        Span::styled(text, ratatui_style)
    }
    
    /// Create a line from spans
    pub fn line(spans: Vec<Span<'static>>) -> Line<'static> {
        Line::from(spans)
    }
    
    /// Create a paragraph with style
    pub fn paragraph(text: impl Into<String>, style: Style) -> Paragraph<'static> {
        Paragraph::new(text)
            .style(style)
            .wrap(Wrap { trim: true })
    }
    
    /// Create a styled line from text
    pub fn styled_line(text: impl Into<String>, style: &InlineTextStyle, theme: &InlineTheme) -> Line<'static> {
        let text = text.into();
        let ratatui_style = inline_to_ratatui(style, theme);
        Line::from(vec![Span::styled(text, ratatui_style)])
    }
}
```

**Usage in Session:**
```rust
// OLD
let style = Style::default().fg(ratatui_color_from_ansi(inline_style.color));
let span = Span::styled(text.to_string(), style);

// NEW
let span = style_helpers::span(text, &inline_style, &self.theme);
```

### Implementation Steps

**Step 3.3.1: Create style_helpers module (30 min)**
- Define helper functions
- Add comprehensive doc comments
- Create unit tests

**Step 3.3.2: Update render functions to use helpers (30 min)**
- Replace Style construction boilerplate
- Replace Span creation patterns
- Update Line construction calls

**Step 3.3.3: Test and verify (20 min)**
- Run test suite
- Check rendering output
- Verify no visual regressions

### Expected Outcomes

- **Code reduced:** ~40 lines
- **Boilerplate eliminated:** 50+ instances
- **Maintainability:** Centralized style conversions
- **Consistency:** Single source of truth for styling

---

## 3.4 Integration and Validation

### Testing Checklist

- [ ] All unit tests pass (palette, tool, style helpers)
- [ ] All session tests pass (message rendering, etc.)
- [ ] No performance regressions detected
- [ ] Visual rendering unchanged (manual verification)
- [ ] No clippy warnings introduced
- [ ] Code coverage maintained

### Code Review Points

- [ ] Is the trait design flexible enough?
- [ ] Are error cases handled gracefully?
- [ ] Is documentation sufficient?
- [ ] Are there opportunities for further consolidation?
- [ ] Do naming conventions match AGENTS.md?

### Metrics Before/After Phase 3

```
Before Phase 3:
- File size: ~4,855 lines
- Functions: ~158
- Cyclomatic complexity: Max ~35
- Code duplication: 6%

After Phase 3:
- File size: ~4,600 lines (-250 lines, -5%)
- Functions: ~150 (-8)
- Cyclomatic complexity: Max ~35 (reduced by ~20 in aggregate)
- Code duplication: ~3% (-3%)
```

---

## Migration Timeline

| Step | Task | Est. Time | Risk |
|------|------|-----------|------|
| 3.1.1 | PaletteItem trait | 30 min | Low |
| 3.1.2 | Implement for types | 30 min | Low |
| 3.1.3 | PaletteRenderer | 30 min | Low |
| 3.1.4 | Session integration | 45 min | Medium |
| 3.1.5 | Cleanup | 15 min | Low |
| **3.1 Total** | **Generic Palette** | **2.5 hours** | **Low** |
| 3.2.1 | ToolStyler struct | 30 min | Low |
| 3.2.2-5 | Migrate functions | 70 min | Low |
| 3.2.6 | Test & verify | 20 min | Low |
| **3.2 Total** | **Tool Consolidation** | **2 hours** | **Low** |
| 3.3.1-3 | Style helpers | 1.5 hours | Low |
| **3.3 Total** | **Style Helpers** | **1.5 hours** | **Low** |
| **Phase 3 Total** | **Complete Deduplication** | **6 hours** | **Low** |

---

## Acceptance Criteria

- [ ] All 200+ existing tests pass
- [ ] No clippy warnings introduced
- [ ] Code duplication reduced from 6% to <3%
- [ ] PaletteRenderer generic works for both file and prompt palettes
- [ ] ToolStyler consolidates 5 functions into 1
- [ ] StyleHelpers eliminates 50+ boilerplate patterns
- [ ] Documentation updated for new modules
- [ ] Performance verified (no regressions)
- [ ] Manual testing shows unchanged visual behavior
- [ ] Public API backward compatible

---

## Next Phase (Phase 4): Complexity Reduction

After Phase 3 completion, Phase 4 will focus on breaking down high-complexity functions:

- **process_key()** (CC ~35) → Multiple handlers (CC <10 each)
- **render_message_spans()** (CC ~18) → Render by kind (CC <8 each)
- **render_tool_header_line()** (CC ~20) → Dedicated renderer

**Estimated effort:** 4-5 hours

---

## Questions & Decisions

### Q: Should PaletteRenderer be in a separate module?
**A:** Yes, create `session/palette.rs` to keep Session focused on composition, not implementation.

### Q: How to handle theme in ToolStyler?
**A:** Store as field, accept in constructor. Allows creating pre-configured instances.

### Q: Will generic rendering impact performance?
**A:** No - generics are specialized at compile time, zero runtime overhead.

### Q: Should StyleHelpers be public?
**A:** Yes, other TUI components could benefit. Place in `session/style.rs` or `tui/style.rs`.

---

## References

- Phase 2 Progress: `PHASE_2_PROGRESS.md`
- Analysis: `SESSION_REFACTORING_ANALYSIS.md`
- Implementation Guide: `SESSION_REFACTORING_IMPLEMENTATION.md`
