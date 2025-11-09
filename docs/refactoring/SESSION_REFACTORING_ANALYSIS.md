# TUI Session Management Refactoring Analysis

## Executive Summary

The `vtcode-core/src/ui/tui/session.rs` file is a critical but complex module (4855 lines, 158 functions, 44 struct fields). This analysis identifies architectural issues, code smells, and refactoring opportunities to improve maintainability, performance, and testability while preserving backward compatibility.

## Current State Analysis

### File Statistics
- **Total Lines:** 4,855
- **Number of Functions:** ~158
- **Struct Fields:** 44 fields in `Session`
- **Cyclomatic Complexity:** Very High (multiple functions exceed 15 CC)
- **Test Coverage:** ~200 tests embedded within the file

### Key Issues Identified

#### 1. **God Object Anti-Pattern** (Critical)
The `Session` struct violates Single Responsibility Principle:
- 44 fields managing disparate concerns
- Combines UI state, input handling, scrolling, modals, palettes, history, and rendering
- No clear separation of concerns

**Fields by Responsibility:**
```
Core Message/Transcript (11):
  lines, transcript_rows, transcript_width, transcript_view_top, 
  scroll_offset, cached_max_scroll_offset, scroll_metrics_dirty,
  transcript_cache, line_revision_counter, in_tool_code_fence, plan

Input Management (7):
  input, cursor, input_enabled, prompt_prefix, prompt_style,
  placeholder, placeholder_style, input_history, input_history_index,
  input_history_draft, last_escape_time

UI State/Rendering (11):
  theme, view_rows, header_rows, input_height, needs_redraw, 
  needs_full_clear, should_exit, cursor_visible, header_context,
  labels, input_status_left, input_status_right, show_timeline_pane

Modal/Dialog Management (4):
  modal, file_palette, file_palette_active, deferred_file_browser_trigger,
  prompt_palette, prompt_palette_active, deferred_prompt_browser_trigger

Navigation (2):
  navigation_state, slash_palette

Queue Management (3):
  queued_inputs, queue_overlay_cache, queue_overlay_version

Configuration (2):
  custom_prompts, deferred_file_browser_trigger, deferred_prompt_browser_trigger
```

#### 2. **Duplicated Code Patterns** (High)

**File Palette & Prompt Palette Rendering:**
Lines 585-714 and 783-877 render nearly identical structures with 95% code overlap:
- `render_file_palette()` and `render_prompt_palette()`
- `render_file_palette_loading()` and `render_prompt_palette_loading()`
- `file_palette_instructions()` and `prompt_palette_instructions()`

**Tool Name Styling (Lines 1305-1435):**
- `strip_tool_status_prefix()` - manually iterates status icons
- `simplify_tool_display()` - multiple hardcoded pattern replacements
- `format_tool_parameters()` - duplicated quote-wrapping logic
- `normalize_tool_name()` - long match statement with groupings
- `tool_inline_style()` - hardcoded color mappings

#### 3. **High Cyclomatic Complexity** (High)

Functions exceeding CC 15:
- `process_key()` - handles 20+ key combinations (CC ~35)
- `render_message_spans()` - multiple nested conditions for different message kinds (CC ~18)
- `render_tool_header_line()` - complex parsing and styling logic (CC ~20)
- `render()` - viewport calculations and layout orchestration (CC ~16)
- `handle_command()` - large match statement with 17 variants

#### 4. **Inefficient Data Structures** (Medium)

**Scroll Metrics Caching:**
- `cached_max_scroll_offset` + `scroll_metrics_dirty` flag is manual cache invalidation
- Could use computed properties or lazy evaluation
- String concatenation in loops: `format_tool_parameters()` uses repeated `replace()`

**Message Searching:**
- Linear search through `Vec<MessageLine>` for filtering/finding
- No indexing structure for quick lookups

**Input History:**
- `input_history`, `input_history_index`, `input_history_draft` separate fields
- Should be a single type-safe data structure

#### 5. **Error Handling Issues** (Medium)

- Unwrap patterns in vector access (lines 801-802, 809)
- Unchecked `.get()` with implicit defaults
- ANSI parsing fallback (line 1035) lacks error context
- No Result-based error propagation

#### 6. **Lifetime & Reference Management** (Medium)

- Multiple `clone()` calls on strings: `segment.text.clone()` (line 1068), labels
- `'static` lifetime requirements force string allocations in span creation
- No borrowed data in modal/palette rendering functions

#### 7. **Module Organization** (Low)

Current submodules organized by feature:
```
session/
  ├─ file_palette
  ├─ header
  ├─ input
  ├─ message
  ├─ modal
  ├─ navigation
  ├─ prompt_palette
  ├─ queue
  ├─ slash
  ├─ slash_palette
  └─ transcript
```

Mixing concerns:
- `input.rs` contains logic that belongs in a separate input manager
- `queue.rs` is small and could be inlined or grouped with transcript
- No clear state management pattern

#### 8. **Testing Impediments** (Medium)

- Tests live in same file (1000+ lines of test code)
- Many helper functions are private, requiring tests to exist in same module
- Hard to test individual components in isolation
- No test fixtures or builder patterns for complex state setup

## Refactoring Plan

### Phase 1: Extract Concerns into Separate Modules (High Impact, Low Risk)

#### 1.1 Create `InputManager` Struct
**Responsibility:** Handle user input state and history

**Extract fields:**
```rust
pub struct InputManager {
    content: String,
    cursor: usize,
    history: Vec<String>,
    history_index: Option<usize>,
    history_draft: Option<String>,
    last_escape_time: Option<Instant>,
    enabled: bool,
}

impl InputManager {
    pub fn insert_text(&mut self, text: &str)
    pub fn backspace(&mut self)
    pub fn move_cursor_left(&mut self)
    pub fn move_cursor_right(&mut self)
    pub fn go_to_history_next(&mut self)
    pub fn go_to_history_prev(&mut self)
    pub fn clear(&mut self)
    pub fn reset_history_navigation(&mut self)
}
```

**Benefits:**
- Encapsulates input logic
- Reusable in other contexts
- Easier to test
- Clearer API

#### 1.2 Create `ScrollManager` Struct
**Responsibility:** Manage viewport scrolling and metrics

**Extract fields:**
```rust
pub struct ScrollManager {
    offset: usize,
    max_offset: usize,
    viewport_rows: u16,
    total_rows: usize,
    metrics_dirty: bool,
}

impl ScrollManager {
    pub fn scroll_up(&mut self, lines: usize)
    pub fn scroll_down(&mut self, lines: usize)
    pub fn scroll_page_up(&mut self)
    pub fn scroll_page_down(&mut self)
    pub fn set_offset(&mut self, offset: usize)
    pub fn invalidate_metrics(&mut self)
    pub fn update_viewport(&mut self, rows: u16, total: usize)
}
```

**Benefits:**
- Separates scrolling logic
- Reusable across different views
- Easier to optimize and test
- Clear scroll state management

#### 1.3 Create `ModalManager` Struct
**Responsibility:** Manage modal dialogs and palettes

**Extract fields:**
```rust
pub struct ModalManager {
    current_modal: Option<ModalState>,
    file_palette: Option<FilePalette>,
    file_palette_active: bool,
    deferred_file_browser_trigger: bool,
    prompt_palette: Option<PromptPalette>,
    prompt_palette_active: bool,
    deferred_prompt_browser_trigger: bool,
}

impl ModalManager {
    pub fn show_modal(&mut self, title: String, lines: Vec<String>)
    pub fn show_file_browser(&mut self)
    pub fn show_prompt_browser(&mut self)
    pub fn close_current(&mut self)
    pub fn is_active(&self) -> bool
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<InlineEvent>
}
```

**Benefits:**
- Isolates modal state
- Clearer modal lifecycle management
- Easier to handle modal interactions
- Supports future modal stacking

#### 1.4 Create `TranscriptManager` Struct
**Responsibility:** Manage message lines and rendering

**Extract fields:**
```rust
pub struct TranscriptManager {
    lines: Vec<MessageLine>,
    cache: Option<TranscriptReflowCache>,
    width: u16,
    height: u16,
    view_top: usize,
    revision_counter: u64,
    in_tool_code_fence: bool,
}

impl TranscriptManager {
    pub fn push_line(&mut self, kind: InlineMessageKind, segments: Vec<InlineSegment>)
    pub fn append_inline(&mut self, kind: InlineMessageKind, segment: InlineSegment)
    pub fn replace_last(&mut self, count: usize, kind: InlineMessageKind, lines: Vec<Vec<InlineSegment>>)
    pub fn reflow_lines(&self, width: u16) -> Vec<Line<'static>>
    pub fn get_line(&self, index: usize) -> Option<&MessageLine>
    pub fn line_count(&self) -> usize
}
```

**Benefits:**
- Encapsulates message management
- Separates rendering from state
- Enables caching improvements
- Clear API for transcript operations

#### 1.5 Create `UIState` Struct
**Responsibility:** Manage UI rendering flags and dimensions

**Extract fields:**
```rust
pub struct UIState {
    needs_redraw: bool,
    needs_full_clear: bool,
    view_rows: u16,
    input_height: u16,
    header_rows: u16,
    show_timeline_pane: bool,
}

impl UIState {
    pub fn mark_dirty(&mut self)
    pub fn take_redraw(&mut self) -> bool
    pub fn apply_dimensions(&mut self, rows: u16, input_height: u16)
}
```

**Benefits:**
- Groups related UI flags
- Clear rendering state management
- Easier to reason about redraw logic

### Phase 2: Eliminate Code Duplication (Medium Impact, Medium Risk)

#### 2.1 Create `PaletteRenderer<T>` Generic
Replace file_palette and prompt_palette rendering duplication:

```rust
trait PaletteItem {
    fn display_name(&self) -> String;
    fn style(&self) -> Option<Style>;
    fn is_directory(&self) -> Option<bool> { None }
}

struct PaletteRenderer<T: PaletteItem> {
    title: String,
    items: Vec<T>,
    current_page: usize,
    total_pages: usize,
}

impl<T: PaletteItem> PaletteRenderer<T> {
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &InlineTheme)
}
```

**Benefits:**
- ~200 lines of duplicate code eliminated
- Single source of truth for palette rendering
- Easy to add new palette types
- Testable in isolation

#### 2.2 Create `ToolStyler` Struct
Replace hardcoded tool styling logic:

```rust
pub struct ToolStyler {
    theme: InlineTheme,
    color_map: HashMap<String, AnsiColorEnum>,
    status_icons: &'static [&'static str],
}

impl ToolStyler {
    pub fn style_name(&self, name: &str) -> InlineTextStyle
    pub fn strip_status(&self, text: &str) -> &str
    pub fn simplify_display(&self, text: &str) -> String
    pub fn format_parameters(&self, text: &str) -> String
    pub fn normalize_name(&self, name: &str) -> String
}
```

**Benefits:**
- Centralizes tool styling logic
- Configuration-driven color mappings
- Easier to test styling rules
- Reduces function count by ~5

#### 2.3 Extract `StyleHelpers` Module
Create reusable style building functions:

```rust
pub fn make_span(text: &str, style: &InlineTextStyle, fallback: Option<AnsiColorEnum>) -> Span<'static>
pub fn make_styled_line(spans: Vec<Span<'static>>) -> Line<'static>
pub fn make_paragraph(text: &str, style: Style) -> Paragraph<'static>
pub fn ratatui_style_from_theme(theme: &InlineTheme) -> Style
```

**Benefits:**
- Reduces repeated style conversions
- Single point for style transformations
- Easier to maintain consistent styling

### Phase 3: Reduce Complexity (Medium Impact, Medium Risk)

#### 3.1 Break Down `process_key()` Function
Current CC ~35 (extremely high). Split into smaller handlers:

```rust
fn process_key(&mut self, key: KeyEvent) -> Option<InlineEvent> {
    match key.code {
        KeyCode::Enter => self.handle_enter_key(key.modifiers),
        KeyCode::Tab => self.handle_tab_key(key.modifiers),
        KeyCode::Up | KeyCode::Down => self.handle_vertical_nav(key.code, key.modifiers),
        KeyCode::Home | KeyCode::End => self.handle_horizontal_nav(key.code),
        KeyCode::PageUp | KeyCode::PageDown => self.handle_page_nav(key.code),
        KeyCode::Delete | KeyCode::Backspace => self.handle_deletion(key.code),
        KeyCode::Char(c) => self.handle_character(c, key.modifiers),
        KeyCode::Esc => self.handle_escape(),
        _ => None,
    }
}
```

**Benefits:**
- Reduces CC from ~35 to ~20
- Each handler is 5-10 CC
- Easier to understand and test
- Single responsibility for each key type

#### 3.2 Break Down `render_message_spans()`
Current CC ~18. Separate by message kind:

```rust
fn render_message_spans(&self, index: usize) -> Vec<Span<'static>> {
    let Some(line) = self.lines.get(index) else {
        return vec![Span::raw(String::new())];
    };
    
    match line.kind {
        InlineMessageKind::Agent => self.render_agent_spans(line),
        InlineMessageKind::User => self.render_user_spans(line),
        InlineMessageKind::Tool => self.render_tool_spans(line),
        InlineMessageKind::Pty => self.render_pty_spans(line, index),
        InlineMessageKind::Info => self.render_info_spans(line),
        InlineMessageKind::Error => self.render_error_spans(line),
    }
}
```

**Benefits:**
- Reduces CC from ~18 to ~5 in main function
- Each renderer is self-contained
- Easier to test individual message types
- Clear extension point for new types

#### 3.3 Break Down `handle_command()`
Current CC ~17 (large match). Already reasonably organized, but add helper:

```rust
fn handle_command(&mut self, command: InlineCommand) {
    match command {
        // ... existing cases ...
    }
    self.mark_dirty();  // Move to caller or make optional
}
```

**Benefits:**
- Each case is already isolated
- No additional complexity reduction needed
- Consider moving `mark_dirty()` to caller

### Phase 4: Optimize Data Structures (Low Impact, Medium Risk)

#### 4.1 Replace Manual Cache Invalidation with Lazy Evaluation
Current:
```rust
cached_max_scroll_offset: usize,
scroll_metrics_dirty: bool,
```

Better:
```rust
#[derive(Clone, Copy)]
struct ScrollMetrics {
    max_offset: usize,
    total_rows: usize,
    viewport_height: usize,
}

impl Session {
    fn scroll_metrics(&self) -> ScrollMetrics {
        if let Some(cache) = self.scroll_metrics_cache {
            cache
        } else {
            // Recompute
            let metrics = ScrollMetrics { /* ... */ };
            // Cache would be stored in Rc<RefCell<>> if needed
            metrics
        }
    }
}
```

**Benefits:**
- No manual invalidation needed
- Cleaner logic flow
- Can add memoization if needed

#### 4.2 Use Proper Input History Type
Current:
```rust
input_history: Vec<String>,
input_history_index: Option<usize>,
input_history_draft: Option<String>,
```

Better:
```rust
struct InputHistory {
    entries: Vec<String>,
    current_index: Option<usize>,
    draft: Option<String>,
}

impl InputHistory {
    fn push(&mut self, entry: String)
    fn next(&mut self) -> Option<String>
    fn previous(&mut self) -> Option<String>
    fn save_draft(&mut self, draft: String)
    fn restore_draft(&mut self) -> Option<String>
}
```

**Benefits:**
- Type-safe encapsulation
- Clear semantics
- Reusable in other contexts
- Better invariant checking

### Phase 5: Improve Error Handling (Low Impact, Low Risk)

#### 5.1 Add Error Context
Replace panics/unwraps with proper Result types:

```rust
// Before
fn render_file_palette(&mut self, frame: &mut Frame, viewport: Rect) {
    let Some(palette) = self.file_palette.as_ref() else {
        return;
    };
    let items = palette.current_page_items();  // Could panic
}

// After
fn render_file_palette(&mut self, frame: &mut Frame, viewport: Rect) -> Result<()> {
    let palette = self.file_palette.as_ref().context("file palette not loaded")?;
    let items = palette.current_page_items().context("failed to get palette items")?;
    Ok(())
}
```

**Benefits:**
- Better error visibility
- Easier debugging
- Can log errors properly
- Graceful degradation

#### 5.2 Add Try Operator Usage
```rust
fn render_transcript_cached(&self, width: u16) -> Result<Vec<Line>> {
    let cache = self.transcript_cache.as_ref().ok_or(anyhow!("cache not available"))?;
    let reflow_cache = cache.for_width(width)?;
    Ok(reflow_cache.reflowed_lines.clone())
}
```

### Phase 6: Improve Module Organization (Low Impact, Low Risk)

#### 6.1 Reorganize session/ Submodule
```
session/
├─ mod.rs                    # Main Session struct
├─ input.rs                  # InputManager
├─ scroll.rs                 # ScrollManager
├─ transcript.rs             # TranscriptManager (existing)
├─ modal/
│  ├─ mod.rs                # ModalManager
│  ├─ file_palette.rs       # FilePalette (moved)
│  └─ prompt_palette.rs     # PromptPalette (moved)
├─ rendering/
│  ├─ mod.rs
│  ├─ message.rs            # render_message_spans
│  ├─ header.rs             # render_header
│  ├─ palette.rs            # PaletteRenderer generic
│  └─ tool.rs               # ToolStyler
├─ keys/
│  ├─ mod.rs
│  └─ handlers.rs           # process_key handlers
└─ tests.rs                 # All tests moved here
```

**Benefits:**
- Clearer logical grouping
- Tests separated from implementation
- Easier to navigate
- Better discoverability

## Implementation Strategy

### Step 1: Prepare Infrastructure (Low Risk)
1. Move tests to `tests.rs`
2. Add `InputManager` struct
3. Add `ScrollManager` struct
4. Update existing functions to use new types

### Step 2: Extract Managers (Medium Risk)
1. Extract `TranscriptManager` fields
2. Extract `ModalManager` fields
3. Extract `UIState` fields
4. Wire into Session through composition

### Step 3: Reduce Duplication (Medium Risk)
1. Create `PaletteRenderer<T>` generic
2. Create `ToolStyler` struct
3. Remove duplicate palette rendering
4. Remove duplicate styling logic

### Step 4: Reduce Complexity (Medium Risk)
1. Break down `process_key()` into handlers
2. Break down `render_message_spans()` by kind
3. Add helper functions for common patterns
4. Keep CC below 10 for new functions

### Step 5: Optimize Data (Low Risk)
1. Replace manual cache invalidation
2. Use proper history type
3. Add error context

### Step 6: Reorganize (Low Risk)
1. Create new module structure
2. Move functions to appropriate modules
3. Update imports and re-exports
4. Document module purposes

## Backward Compatibility Guarantees

All public APIs will remain unchanged:
- `pub fn new()`
- `pub fn handle_command()`
- `pub fn handle_event()`
- `pub fn render()`
- Public accessor methods

Internal implementation details can change:
- Field access pattern (through methods instead of direct)
- Error handling (return Result instead of panic)
- Caching mechanism (lazy vs explicit)

Tests will remain comprehensive and cover all public APIs.

## Expected Outcomes

### Code Quality
- Reduce struct fields from 44 to ~15 (Session core only)
- Reduce cyclomatic complexity from max ~35 to max ~10
- Eliminate ~300 lines of duplicate code
- Reduce file from 4855 to ~3500 lines

### Maintainability
- Clear separation of concerns (5 focused managers)
- Easier to understand and modify individual features
- Reusable components for future TUI work
- Better test organization and coverage

### Performance
- Lazy evaluation eliminates unnecessary computations
- Better data structure choices reduce overhead
- No performance regressions on hot paths

### Extensibility
- New message types easily added
- New palette types via `PaletteRenderer<T>`
- New key handlers via match arms
- Plugin-friendly component structure

## Risk Assessment

| Phase | Risk | Mitigation | Effort |
|-------|------|-----------|--------|
| 1 | Low | Comprehensive testing | 2h |
| 2 | Low | Keep old functions as thin wrappers initially | 3h |
| 3 | Medium | Incremental refactoring, test each change | 4h |
| 4 | Low | Verify no perf regression | 2h |
| 5 | Low | Add error context incrementally | 1h |
| 6 | Medium | Update imports carefully | 2h |

**Total Estimated Effort:** 14 hours
**Complexity:** Medium
**Risk Level:** Low (with careful testing)

## Acceptance Criteria

- [ ] All 200+ existing tests pass
- [ ] No clippy warnings introduced
- [ ] Cyclomatic complexity reduced below 12 for all functions
- [ ] Struct fields reduced to <20 in Session
- [ ] Code duplication <5% (100+ lines eliminated)
- [ ] Documentation updated for new modules
- [ ] Public API backward compatible
- [ ] No performance regressions in hot paths
- [ ] New code follows AGENTS.md style guide

## Timeline

- **Week 1:** Phases 1-2 (extract managers, generic palette renderer)
- **Week 2:** Phases 3-4 (reduce complexity, optimize data)
- **Week 3:** Phases 5-6 (error handling, reorganization, testing)

## Future Improvements

1. **State Machine Pattern:** Use `enum` for clear state transitions
2. **Event Bus:** Decouple components via events instead of direct calls
3. **Async Rendering:** Non-blocking reflow calculations for large transcripts
4. **Component Composition:** Build complex UIs from simpler components
5. **Performance Profiling:** Benchmark hot paths before/after refactoring
