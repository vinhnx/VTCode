# Session Refactoring - Implementation Guide

## Overview

This guide provides step-by-step instructions for refactoring `vtcode-core/src/ui/tui/session.rs` to improve code quality, maintainability, and performance.

## Phase 1: Manager Extraction (COMPLETED ✓)

### 1.1 InputManager - DONE
**File:** `vtcode-core/src/ui/tui/session/input.rs`
**Status:** Complete with full test coverage

**What it does:**
- Manages user input text and cursor position
- Handles UTF-8 aware cursor movement
- Implements command history with draft saving
- Escape key double-tap detection

**Key methods:**
```rust
pub fn content(&self) -> &str
pub fn cursor(&self) -> usize
pub fn insert_text(&mut self, text: &str)
pub fn move_cursor_left(&mut self)
pub fn go_to_previous_history(&mut self) -> Option<String>
pub fn add_to_history(&mut self, entry: String)
```

**Benefits:**
- Encapsulates all input-related logic
- Reusable in other TUI contexts
- Well-tested (8 tests)
- Clear API

**Usage Example:**
```rust
let mut input = InputManager::new();
input.insert_text("hello");
input.move_cursor_left();
input.add_to_history("hello".to_string());
```

### 1.2 ScrollManager - DONE
**File:** `vtcode-core/src/ui/tui/session/scroll.rs`
**Status:** Complete with full test coverage

**What it does:**
- Manages viewport scrolling state
- Calculates scroll metrics and bounds
- Handles page and line scrolling
- Tracks scroll progress

**Key methods:**
```rust
pub fn offset(&self) -> usize
pub fn scroll_up(&mut self, lines: usize)
pub fn scroll_down(&mut self, lines: usize)
pub fn scroll_page_up(&mut self)
pub fn scroll_page_down(&mut self)
pub fn set_total_rows(&mut self, total: usize) -> bool
pub fn visible_range(&self) -> (usize, usize)
pub fn progress_percent(&self) -> u8
```

**Benefits:**
- Eliminates manual cache invalidation
- Clear scroll state API
- Reusable scroll mechanism
- Performance optimized
- Well-tested (8 tests)

**Usage Example:**
```rust
let mut scroll = ScrollManager::new(20); // 20 row viewport
scroll.set_total_rows(100);
scroll.scroll_page_down();
let (start, end) = scroll.visible_range();
```

### 1.3 UIState - DONE
**File:** `vtcode-core/src/ui/tui/session/ui_state.rs`
**Status:** Complete with full test coverage

**What it does:**
- Manages rendering flags (dirty, needs_clear)
- Tracks UI dimensions (view rows, input height, header rows)
- Handles cursor visibility and input enabled state
- Timeline pane visibility

**Key methods:**
```rust
pub fn mark_dirty(&mut self)
pub fn take_redraw(&mut self) -> bool
pub fn set_view_rows(&mut self, rows: u16)
pub fn set_input_height(&mut self, height: u16)
pub fn toggle_timeline_pane(&mut self)
pub fn available_transcript_rows(&self) -> u16
```

**Benefits:**
- Centralizes rendering state
- Automatic dirty flag on changes
- Calculated properties for transcript space
- Well-tested (8 tests)

**Usage Example:**
```rust
let mut ui = UIState::new(24, true);
ui.set_view_rows(30);
if ui.take_redraw() {
    // Perform redraw
}
```

## Phase 2: Integration into Session (IN PROGRESS)

### 2.1 Step-by-Step Integration

**Step 1: Add manager fields to Session**
```rust
pub struct Session {
    // Managers
    input_manager: InputManager,
    scroll_manager: ScrollManager,
    ui_state: UIState,
    
    // Keep existing fields that haven't been extracted yet
    lines: Vec<MessageLine>,
    // ... rest of fields
}
```

**Step 2: Update Session::new()**
```rust
impl Session {
    pub fn new(theme: InlineTheme, placeholder: Option<String>, view_rows: u16, show_timeline_pane: bool) -> Self {
        let mut session = Self {
            input_manager: InputManager::new(),
            scroll_manager: ScrollManager::new(10), // Will be updated with actual rows
            ui_state: UIState::new(view_rows, show_timeline_pane),
            // ... initialize other fields
        };
        session
    }
}
```

**Step 3: Replace field access patterns**

Before:
```rust
self.input = content;
self.cursor = content.len();
self.scroll_offset = 0;
```

After:
```rust
self.input_manager.set_content(content);
self.scroll_manager.set_offset(0);
```

**Step 4: Create migration wrapper methods**
```rust
// Temporary wrapper for backward compatibility during migration
impl Session {
    fn input(&self) -> &str {
        self.input_manager.content()
    }
    
    fn input_mut(&mut self) -> &mut InputManager {
        &mut self.input_manager
    }
}
```

### 2.2 Migration Checklist

- [ ] Add manager fields to Session struct
- [ ] Create Session::new() initialization
- [ ] Update all input field access to use input_manager
- [ ] Update all scroll field access to use scroll_manager
- [ ] Update all ui state field access to use ui_state
- [ ] Run tests to verify no regressions
- [ ] Remove wrapper methods once migration is complete

## Phase 3: Code Deduplication (NOT STARTED)

### 3.1 Create PaletteRenderer Generic
**Target:** Eliminate ~200 lines of duplicate palette rendering code

**Current duplication:**
- `render_file_palette()` (lines 585-714)
- `render_prompt_palette()` (lines 783-877)
- `render_file_palette_loading()` (lines 716-739)
- `render_prompt_palette_loading()` (lines 880-903)

**Solution:** Generic renderer trait

```rust
trait PaletteItem {
    fn display_name(&self) -> String;
    fn display_icon(&self) -> String { String::new() }
    fn style(&self) -> Option<Style> { None }
}

struct PaletteRenderer<T: PaletteItem> {
    items: Vec<(usize, T, bool)>,
    selected: usize,
    page_size: usize,
}

impl<T: PaletteItem> PaletteRenderer<T> {
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &InlineTheme)
}
```

**Implementation steps:**
1. Define PaletteItem trait
2. Implement for FilePaletteEntry and PromptPaletteEntry
3. Create generic renderer
4. Update file_palette and prompt_palette rendering
5. Remove duplicate code

### 3.2 Extract ToolStyler
**Target:** Consolidate tool styling and formatting (~150 lines)

**Current duplication:**
- `strip_tool_status_prefix()` (lines 1305-1314)
- `simplify_tool_display()` (lines 1317-1344)
- `format_tool_parameters()` (lines 1347-1378)
- `normalize_tool_name()` (lines 1381-1393)
- `tool_inline_style()` (lines 1395-1435)

**Solution:** Dedicated ToolStyler struct

```rust
pub struct ToolStyler {
    color_map: HashMap<String, AnsiColorEnum>,
}

impl ToolStyler {
    pub fn strip_status(&self, text: &str) -> &str
    pub fn simplify_display(&self, text: &str) -> String
    pub fn format_parameters(&self, text: &str) -> String
    pub fn normalize_name(&self, name: &str) -> String
    pub fn get_style(&self, name: &str, theme: &InlineTheme) -> InlineTextStyle
}
```

**Benefits:**
- Centralized tool styling logic
- Configuration-driven color mappings
- Testable in isolation
- Easier to extend with new tools

## Phase 4: Reduce Complexity (NOT STARTED)

### 4.1 Break Down process_key()
**Current complexity:** CC ~35 (extremely high)
**Target complexity:** CC <10 per function

```rust
fn process_key(&mut self, key: KeyEvent) -> Option<InlineEvent> {
    match key.code {
        KeyCode::Enter => self.handle_enter_key(key),
        KeyCode::Tab => self.handle_tab_key(key),
        KeyCode::Up | KeyCode::Down => self.handle_vertical_nav(key),
        KeyCode::Home | KeyCode::End => self.handle_horizontal_nav(key),
        KeyCode::PageUp | KeyCode::PageDown => self.handle_page_nav(key),
        KeyCode::Delete | KeyCode::Backspace => self.handle_deletion(key),
        KeyCode::Char(c) => self.handle_character(c, key.modifiers),
        KeyCode::Esc => self.handle_escape(),
        _ => None,
    }
}

fn handle_enter_key(&mut self, key: KeyEvent) -> Option<InlineEvent>
fn handle_tab_key(&mut self, key: KeyEvent) -> Option<InlineEvent>
// ... etc
```

**Benefits:**
- Each handler is 5-10 CC
- Easier to test
- Clear key binding documentation

### 4.2 Break Down render_message_spans()
**Current complexity:** CC ~18
**Target complexity:** CC <8

```rust
fn render_message_spans(&self, index: usize) -> Vec<Span<'static>> {
    let line = self.lines.get(index)?;
    
    match line.kind {
        InlineMessageKind::Agent => self.render_agent_message(line),
        InlineMessageKind::User => self.render_user_message(line),
        InlineMessageKind::Tool => self.render_tool_message(line),
        InlineMessageKind::Pty => self.render_pty_message(line, index),
        InlineMessageKind::Info => self.render_info_message(line),
        InlineMessageKind::Error => self.render_error_message(line),
    }
}
```

**Benefits:**
- Each renderer is self-contained
- Easier to test individual message types
- Clear extension point for new types

## Phase 5: Optimize Data Structures (NOT STARTED)

### 5.1 Lazy Scroll Metrics
Replace manual cache invalidation with computed values

**Before:**
```rust
cached_max_scroll_offset: usize,
scroll_metrics_dirty: bool,

fn invalidate_scroll_metrics(&mut self) {
    self.scroll_metrics_dirty = true;
}

fn cached_max_scroll(&self) -> usize {
    if self.scroll_metrics_dirty {
        // Recompute
    }
    self.cached_max_scroll_offset
}
```

**After:**
```rust
fn max_scroll_offset(&self) -> usize {
    // Always computed on demand
    self.total_transcript_rows(self.transcript_width)
        .saturating_sub(self.transcript_rows as usize)
}
```

### 5.2 Proper Input History Type
Replace three fields with structured type

**Before:**
```rust
input_history: Vec<String>,
input_history_index: Option<usize>,
input_history_draft: Option<String>,
```

**After:**
```rust
input_manager: InputManager,  // Encapsulates all of the above
```

## Current Metrics

### Before Refactoring
- **File Size:** 4,855 lines
- **Functions:** ~158
- **Struct Fields:** 44 (Session)
- **Max Complexity:** CC ~35
- **Code Duplication:** ~300 lines (~6%)
- **Tests:** 200+ (inline)

### Target After Phase 1
- **File Size:** ~4,700 lines (minimal change, managers are external)
- **Functions:** ~150
- **Struct Fields:** 35 (Session) - reduced by 9
- **Max Complexity:** CC ~35 (no change yet)
- **Code Duplication:** ~300 lines (no change yet)

### Target After All Phases
- **File Size:** ~3,200 lines (-34%)
- **Functions:** ~120 (-24%)
- **Struct Fields:** 15 (Session) - reduced by 66%
- **Max Complexity:** CC <10 for all functions
- **Code Duplication:** <50 lines (<1%)
- **Tests:** Organized in separate files

## Testing Strategy

### Unit Tests for New Managers
✓ InputManager - 8 tests
✓ ScrollManager - 8 tests
✓ UIState - 8 tests

**Run tests:**
```bash
cargo test input_manager
cargo test scroll_manager
cargo test ui_state
```

### Integration Tests for Session
- Test Session with new managers
- Verify all event handlers still work
- Check rendering output
- Validate scroll behavior

**Run tests:**
```bash
cargo test session
cargo test render
```

### Regression Testing
- Run entire test suite: `cargo test`
- Run clippy: `cargo clippy`
- Check for panics: `RUST_BACKTRACE=1 cargo test`

## Performance Impact

### InputManager
- **No overhead:** Functions are simple, inlinable
- **Benefit:** Better cache locality from grouping related fields

### ScrollManager
- **Lazy computation:** Only compute when needed
- **Benefit:** Reduces repeated calculations

### UIState
- **Zero overhead:** Copy type, simple operations
- **Benefit:** Automatic dirty flag reduces bugs

### PaletteRenderer (future)
- **Better locality:** Generic code specialization
- **No overhead:** Zero-cost abstraction

## Documentation

### For Users of Session
No changes to public API. All methods still work the same way.

### For Developers
New managers have comprehensive doc comments:
```rust
/// Manages user input state including text, cursor, and history
/// 
/// # Example
/// ```
/// let mut input = InputManager::new();
/// input.insert_text("hello");
/// ```
pub struct InputManager { ... }
```

### For Future Contributors
New organization makes it easier to understand code structure:
- Input handling in one place (InputManager)
- Scrolling logic in one place (ScrollManager)
- Rendering state in one place (UIState)

## Migration Path for Existing Code

### Old Way (Still Works)
```rust
session.input.push_str("text");
session.cursor += 1;
session.scroll_offset = 0;
```

### New Way (After Phase 1)
```rust
session.input_manager.insert_text("text");
session.scroll_manager.set_offset(0);
```

### Transition Period
Both patterns work during migration. Gradually update code to use new managers.

## Checkpoints

- [x] Phase 1.1 - InputManager complete
- [x] Phase 1.2 - ScrollManager complete  
- [x] Phase 1.3 - UIState complete
- [ ] Phase 2 - Integrate managers into Session
- [ ] Phase 3 - Eliminate duplication
- [ ] Phase 4 - Reduce complexity
- [ ] Phase 5 - Optimize data structures
- [ ] Phase 6 - Reorganize modules

## Next Steps

1. **Review the three new managers** in their respective files
2. **Run tests** to verify they work correctly
3. **Plan Phase 2 integration** - decide on integration approach
4. **Schedule remaining phases** - plan complexity reduction work

## Questions & Clarifications

### Q: Why extract to separate modules instead of nested structs?
**A:** Separate files allow:
- Independent testing and development
- Easier to navigate and understand
- Potential reuse in other TUI components
- Natural organization

### Q: Will this affect performance?
**A:** No negative impact:
- Managers are small (copy or Arc references)
- All hot paths remain optimized
- Better organization may improve cache locality

### Q: How do we handle backward compatibility?
**A:** 
- Public API methods remain unchanged
- Internal refactoring doesn't affect external consumers
- Gradual migration of internal code to new managers

### Q: What about tests?
**A:**
- New managers have comprehensive tests (24 total)
- Existing Session tests continue to work
- Will add integration tests during Phase 2
