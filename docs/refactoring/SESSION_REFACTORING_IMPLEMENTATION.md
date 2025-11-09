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

**Step 1: Add manager fields to Session** ✓ COMPLETED
- InputManager integrated (lines 78, 151)
- ScrollManager integrated (lines 79, 152)
- UIState not yet integrated (fields still scattered)

**Step 2: Update Session::new()** ✓ COMPLETED
- Managers initialized in constructor (lines 151-152)
- Initial rows properly calculated

**Step 3: Replace field access patterns** ✓ PARTIALLY COMPLETED
- Input manager methods: `set_content()`, `insert_char()`, `insert_text()`
- Scroll manager methods: `set_offset()` used at line 279
- Still need to migrate other scroll operations to use manager

**Step 4: Create migration wrapper methods** ✓ COMPLETED
- `input()` method removed - using direct manager access
- `input_mut()` method removed - using direct manager access

### 2.2 Migration Checklist - STATUS

- [x] Add manager fields to Session struct
- [x] Create Session::new() initialization  
- [x] Update input field access for SetInput command
- [ ] Migrate render_input() to use input_manager methods
- [ ] Migrate process_key() event handlers to use managers
- [ ] Migrate all scroll operations to use scroll_manager
- [ ] Extract UIState for dirty flag, dimensions tracking
- [ ] Run full test suite verification
- [ ] Remove any redundant wrapper methods

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

## Phase 3.5: Reduce Event Handling Complexity (NEW)

### Extract Duplicate Mouse Event Handling

**Current Issue (lines 343-360):**
```rust
MouseEventKind::ScrollDown => {
    self.scroll_line_down();
    self.mark_dirty();
    let event = InlineEvent::ScrollLineDown;
    if let Some(cb) = callback { cb(&event); }
    let _ = events.send(event);
}
MouseEventKind::ScrollUp => {
    self.scroll_line_up();
    self.mark_dirty();
    let event = InlineEvent::ScrollLineUp;
    if let Some(cb) = callback { cb(&event); }
    let _ = events.send(event);
}
```

**Refactoring:**
- Extract into `handle_scroll_event()` helper
- Eliminate duplication
- Reduce event handler complexity

### Separate Inline Event Callback Logic

**Target:** Extract event emission pattern used 3 times in `handle_event()`
```rust
fn emit_inline_event(
    event: InlineEvent,
    callback: Option<&(dyn Fn(&InlineEvent) + Send + Sync + 'static)>,
    events: &UnboundedSender<InlineEvent>,
) {
    if let Some(cb) = callback {
        cb(&event);
    }
    let _ = events.send(event);
}
```

## Phase 4: Reduce Complexity (NOT STARTED)

### 4.1 Break Down process_key()
**Current complexity:** CC ~35 (extremely high)
**Location:** Lines 1916-2150
**Target complexity:** CC <10 per function

**Current structure (too complex):**
- Single massive match statement (234 lines)
- Handles 10+ different key types
- Each branch has nested conditions
- Multiple event type returns scattered throughout

**Refactoring strategy:**
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
        KeyCode::Backspace | KeyCode::Delete => self.handle_deletion(key),
        KeyCode::F(_) => self.handle_function_key(key),
        _ => None,
    }
}
```

**Implementation steps:**
1. Extract `handle_enter_key()` - handles input submission (CC 5)
2. Extract `handle_tab_key()` - handles autocomplete (CC 4)
3. Extract `handle_vertical_nav()` - Up/Down key handling (CC 6)
4. Extract `handle_horizontal_nav()` - Home/End key handling (CC 3)
5. Extract `handle_page_nav()` - PageUp/PageDown handling (CC 4)
6. Extract `handle_deletion()` - Delete/Backspace handling (CC 4)
7. Extract `handle_character()` - Regular char input (CC 5)
8. Extract `handle_escape()` - Escape key logic (CC 6)
9. Extract `handle_function_key()` - F-key handling (CC 4)

### 4.2 Break Down render_message_spans()
**Current complexity:** CC ~18
**Location:** Lines 970-1100
**Target complexity:** CC <8

**Refactoring strategy:**
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

**Implementation steps:**
1. Extract `render_agent_message()` - agent message styling
2. Extract `render_user_message()` - user message styling
3. Extract `render_tool_message()` - tool message header/body logic
4. Extract `render_pty_message()` - PTY output styling
5. Extract `render_info_message()` - info level messages
6. Extract `render_error_message()` - error styling

**Benefits:**
- Each renderer is self-contained and testable
- Clear extension point for new message types
- Main function becomes simple dispatcher (CC 6)

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
- [x] Phase 2 - Integrate managers into Session (80% complete)
- [ ] Phase 3.5 - Extract event handling helpers (NEW)
- [ ] Phase 4 - Reduce complexity of key handlers
- [ ] Phase 5 - Reduce complexity of rendering
- [ ] Phase 6 - Optimize data structures
- [ ] Phase 7 - Extract message renderer (future)

## Code Analysis & Issues Found

### Duplicate Code Patterns in handle_event()

**Location:** Lines 343-360 (Mouse event handling)

```rust
// PROBLEM: ScrollDown and ScrollUp branches are identical except for action
MouseEventKind::ScrollDown => {
    self.scroll_line_down();
    self.mark_dirty();
    let event = InlineEvent::ScrollLineDown;
    if let Some(cb) = callback { cb(&event); }
    let _ = events.send(event);
}
MouseEventKind::ScrollUp => {
    self.scroll_line_up();
    self.mark_dirty();
    let event = InlineEvent::ScrollLineUp;
    if let Some(cb) = callback { cb(&event); }
    let _ = events.send(event);
}
```

**Impact:** 17 lines of duplicated code, same pattern repeated

**Solution:** Extract event emission into helper function
- Reduces duplication
- Makes patterns explicit and testable
- Easier to add new event types

---

### Inefficient Data Structures

**Location:** Session struct (lines 75-130)

**Issues identified:**

1. **Scattered UI state** (lines 98-109)
   - `needs_redraw: bool` (101)
   - `needs_full_clear: bool` (102)
   - `cursor_visible: bool` (100)
   - `view_rows: u16` (104)
   - `input_height: u16` (105)
   - `transcript_rows: u16` (106)
   - `transcript_width: u16` (107)
   - `input_enabled: bool` (99)
   
   **Problem:** 8 fields all managing UI state scattered throughout struct
   
   **Solution:** Extract into dedicated `UIState` struct (planned for Phase 2)

2. **Palette management fields** (lines 122-129)
   - `custom_prompts: Option<CustomPromptRegistry>` (122)
   - `file_palette: Option<FilePalette>` (123)
   - `file_palette_active: bool` (124)
   - `deferred_file_browser_trigger: bool` (125)
   - `prompt_palette: Option<PromptPalette>` (126)
   - `prompt_palette_active: bool` (127)
   - `deferred_prompt_browser_trigger: bool` (128)
   
   **Problem:** 7 fields managing two palettes with duplicate patterns
   
   **Solution:** Extract into dedicated `PaletteManager` struct

3. **Message caching** (lines 111-114)
   - `transcript_cache: Option<TranscriptReflowCache>` (111)
   - `queued_inputs: Vec<String>` (112)
   - `queue_overlay_cache: Option<QueueOverlay>` (113)
   - `queue_overlay_version: u64` (114)
   
   **Problem:** Cache invalidation logic scattered, version tracking manual
   
   **Solution:** Extract into dedicated `TranscriptCache` struct

---

### High Cyclomatic Complexity

**Location:** process_key() method (lines 1916-2150, ~234 lines)

**Complexity analysis:**
- Single match with 10+ arms
- Each arm has nested conditions (if/match)
- Multiple return paths scattered throughout
- Estimated CC: 35-40 (DANGER ZONE)

**Example complexity pattern:**
```rust
KeyCode::Char(c) => {
    if self.modal.is_some() {
        // ... 15 lines of modal handling
    } else if self.slash_palette.is_active() {
        // ... 8 lines of slash handling
    } else if self.file_palette_active {
        // ... 7 lines of file palette handling
    } else if self.prompt_palette_active {
        // ... 7 lines of prompt palette handling  
    } else if self.input_enabled {
        // ... 25 lines of input handling
        if c == '@' {
            // ... file reference check
        } else if c == '#' {
            // ... prompt reference check
        }
    }
    // Total: 62 nested lines in one match arm!
}
```

**Solution:** Extract into separate handler methods (Phase 4)

---

### Inefficient Error Handling Patterns

**Location:** Various key handlers

**Issues:**
1. No validation before state mutations
2. Unwrap/expect calls in non-test code (lines 2372, 2919, 2943)
3. Errors logged but context lost in rendering path

**Example (line 2372):**
```rust
let input = self.input_manager.content().to_string();
```

Should be:
```rust
let input = self.input_manager.content().to_owned();  // More efficient
```

---

### Memory Efficiency Issues

**Location:** Various methods

**Issues identified:**

1. **Unnecessary clone() calls**
   - Line 522: `session.input = "notes".to_string();` (test)
   - Multiple `.clone()` of input content in handlers

2. **Inefficient string handling**
   - Frequent `to_string()` conversions
   - Multiple allocations for temporary strings

3. **Vec allocation patterns**
   - Some Vecs could use `with_capacity()`
   - Frequent re-allocations when building spans

**Solution (Phase 5):**
- Profile hot paths
- Replace `.clone()` with references where possible
- Use `to_owned()` only when necessary
- Pre-allocate vectors with known sizes

---

## Next Steps

1. **Phase 3.5 (THIS SESSION):** Extract event helpers to reduce duplication
   - Create `handle_scroll_event()` helper
   - Create `emit_inline_event()` helper
   - Consolidate event emission logic
   
2. **Phase 4 (NEXT):** Reduce cyclomatic complexity
   - Break down `process_key()` into handlers
   - Each handler CC < 10
   - Make key bindings explicit and testable

3. **Phase 5 (FUTURE):** Reduce rendering complexity
   - Extract message type renderers
   - Each renderer self-contained
   - Support new message types easily

4. **Phase 6 (FUTURE):** Optimize data structures
   - Extract UIState for dimension tracking
   - Extract PaletteManager for palette logic
   - Extract TranscriptCache for caching logic

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
