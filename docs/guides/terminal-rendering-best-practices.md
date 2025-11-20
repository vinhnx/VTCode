# Terminal Rendering Best Practices for VT Code

This guide documents rendering patterns and best practices specific to VT Code, based on Ratatui conventions.

## Core Principle: Single Draw Per Frame

VT Code follows the **Ratatui recipe** of rendering everything in a single `terminal.draw()` closure per frame cycle.

### ⤫  Anti-Pattern: Multiple Draws

```rust
loop {
    terminal.draw(|f| {
        f.render_widget(widget1, area);
    })?;
    terminal.draw(|f| {
        f.render_widget(widget2, area);
    })?;
    terminal.draw(|f| {
        f.render_widget(widget3, area);
    })?;
}
```

**Why it fails:** Ratatui uses **double buffering**—only the last `draw()` call within a frame cycle gets rendered. The first two calls are overwritten.

### ✓  Correct Pattern: Single Orchestrated Draw

```rust
loop {
    terminal.draw(|f| {
        f.render_widget(widget1, area1);
        f.render_widget(widget2, area2);
        f.render_widget(widget3, area3);
    })?;
}
```

**Implementation in VT Code:**

File: `vtcode-core/src/ui/tui/session.rs`

The `Session::render()` method orchestrates all UI components:
- Header (top bar with model/status)
- Navigation pane (left sidebar)
- Transcript (message history, center)
- Input area (bottom with user input)
- Modals/overlays (palettes, search, etc.)

All rendering happens in one frame cycle.

## Viewport Management

### Single Buffer Concept

Ratatui allocates one rendering buffer for the entire terminal area. When you call `terminal.draw()`, all widgets write to this buffer. At frame end, diffs are sent to the terminal.

**Impact on VT Code:**

1. **Overlapping areas are safe** - Later renders overwrite earlier ones
2. **Off-screen rendering is safe** - Clamped to buffer bounds (mostly)
3. **Partial updates are automatic** - Only changed cells redraw

### Out-of-Bounds Protection

Ratatui **does not prevent** panics from rendering outside the buffer. VT Code must defend against this.

**Pattern (from Ratatui FAQ):**

```rust
fn render_ref(&self, area: Rect, buf: &mut Buffer) {
    // Clamp area to buffer bounds before rendering
    let area = area.intersection(buf.area);
    // Now safe to render...
}
```

**Best practices:**
- Use `Rect::intersection(other)` to clamp to valid regions
- Use `Rect::clamp(constraining_rect)` to clamp coordinates
- Use `Rect::columns()` and `Rect::rows()` iterators (safe by design)

## Layout Computation

### Constraint-Based Layouts

VT Code uses Ratatui's `Layout` system, which guarantees valid region calculations:

```rust
use ratatui::layout::{Constraint, Direction, Layout};

let chunks = Layout::default()
    .direction(Direction::Vertical)
    .constraints([
        Constraint::Length(1),        // Header
        Constraint::Min(1),           // Content
        Constraint::Length(1),        // Input
    ])
    .split(area);
```

**Safety guarantees:**
- `Constraint::Length(n)` - Exactly n rows/cols
- `Constraint::Percentage(p)` - p% of available space
- `Constraint::Min(n)` - At least n, fill remaining
- `Constraint::Max(n)` - At most n

The `Layout` algorithm ensures all constraints fit within `area`.

### Manual Coordinate Calculations

If you calculate regions manually, be defensive:

```rust
// ⤫  Unsafe: Unguarded math can overflow
let width = area.width - 2;
let y = area.top() + some_offset;

// ✓  Safe: Use `saturating_sub()` and `clamp()`
let width = area.width.saturating_sub(2);
let y = (area.top() as u32 + offset).min(area.bottom() as u32) as u16;
```

Better yet, use iterators:
```rust
// ✓  Safest: Iterator-based (can't go out of bounds)
for (i, cell) in f.buffer_mut().content.iter_mut().enumerate() {
    if i < max_items {
        // Safe to write
    }
}
```

## Rendering Widgets

### Composition Pattern

VT Code composes widgets hierarchically. Each "pane" renders into its allocated area:

**Structure:**
```
┌─────────────────────────────────┐
│ Header: Info, status, theme     │  Render size: full_width × 1
├─────────────────────────────────┤
│ │ Nav │ Transcript  │ Modal │  │
│ │     │  (messages) │ (if   │  │
│ │     │             │  any) │  │
├─────────────────────────────────┤
│ Input bar (user text)           │  Render size: full_width × 1-3
└─────────────────────────────────┘
```

**Implementation:**
```rust
pub fn render(&mut self, f: &mut Frame) {
    let area = f.area();
    
    // Split into main regions
    let [header_area, body_area, input_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(3),
    ])
    .areas(area);
    
    // Render each component
    self.render_header(f, header_area);
    self.render_body(f, body_area);      // Handles nav + transcript + modal
    self.render_input(f, input_area);
}
```

### Widget Type Safety

VT Code uses Ratatui's widget trait (`Widget`) for reusable components. Rendering happens via `widget.render()` call.

**Pattern:**
```rust
// Stateless widget (implements Widget trait)
impl Widget for MyCustomWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Write to buf directly
        if area.width < 2 { return; }  // Defensive
        // Safe rendering...
    }
}

// In main render loop:
f.render_widget(my_widget, area);
```

## Reflow and Text Wrapping

### Dynamic Text Reflow

VT Code's message transcript must reflow when terminal resizes. This is expensive, so VT Code caches reflowed text:

**Pattern (from vtcode-core):**
```rust
struct Message {
    text: String,
    cached_lines: Mutex<Option<Vec<String>>>,
    cached_width: Mutex<u16>,
}

fn get_reflowed_lines(&self, width: u16) -> Vec<String> {
    let mut cache = self.cached_lines.lock();
    let mut cache_width = self.cached_width.lock();
    
    if *cache_width != width {
        // Reflow needed
        let lines = textwrap::wrap(&self.text, width);
        *cache = Some(lines.clone());
        *cache_width = width;
        return lines;
    }
    
    cache.clone().unwrap_or_default()
}
```

### Handling Terminal Resize

On `Event::Resize(w, h)`:
1. Clear reflow caches
2. Recalculate layout constraints
3. Trigger next `Event::Render`

VT Code automatically clears caches and re-renders on resize.

## Color and Styling

### Ratatui Color Model

VT Code uses Ratatui's style system for portable colors:

```rust
use ratatui::style::{Color, Modifier, Style};

let style = Style::default()
    .fg(Color::Cyan)
    .bg(Color::Black)
    .add_modifier(Modifier::BOLD);

f.render_widget(
    Paragraph::new("Hello").style(style),
    area
);
```

**Portable color options:**
- Named: `Color::Red`, `Color::Blue`, etc.
- Indexed: `Color::Indexed(200)` (256-color palette)
- RGB: `Color::Rgb(255, 0, 0)` (24-bit color, terminal permitting)

### ANSI SGR Code Parsing

VT Code parses terminal ANSI escape codes and converts them to Ratatui styles:

**File:** `vtcode-core/src/ui/tui/style.rs`

Converts ANSI SGR codes (like `\x1b[1;31m` for bold red) to Ratatui `Style`.

## Performance Considerations

### Double Buffering Efficiency

Ratatui's double buffer means:
- Only cells that changed are sent to terminal
- No "flicker" (frame is complete before display)
- Terminal I/O is optimized via escape sequence diffing

**VT Code optimization:**
- Only send `Event::Render` at 60 FPS (not on every state change)
- Batch state updates into `Event::Tick` (4 Hz)
- Let Ratatui handle diff logic

### Lazy Rendering

VT Code uses the **Tick/Render split pattern**:

```
Event::Tick (4 Hz)
  └─ Update internal state (messages, selection, etc.)
  
Event::Render (60 FPS)
  └─ Redraw UI from state (Ratatui handles diffing)
```

This separates "state updates" from "rendering," making both efficient.

### Minimize String Allocations

Text widgets cache strings when possible:

```rust
// ⤫  Bad: Allocates new string every frame
loop {
    let text = format!("Status: {}", status);
    f.render_widget(Paragraph::new(text), area);
}

// ✓  Good: Reuse string if unchanged
if status_changed {
    self.status_str = format!("Status: {}", status);
}
f.render_widget(Paragraph::new(&self.status_str), area);
```

## Common Rendering Issues

### Issue 1: Widget Disappears on Resize

**Cause:** Hard-coded area sizes (e.g., `area.top() + 10` without bounds checking)

**Fix:** Use `Constraint` and `Layout`, not manual offsets.

### Issue 2: Text Overlaps at Terminal Edge

**Cause:** Not clamping to buffer area

**Fix:** Use `area.intersection(buf.area)` before rendering.

### Issue 3: Panic on Render

**Cause:** Off-bounds cell access in custom widget

**Fix:** Add bounds checks and use `Rect::intersection()`.

## Testing Rendering

VT Code includes render tests in `vtcode-core`:

```rust
#[test]
fn test_render_header() {
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    
    terminal.draw(|f| {
        let mut session = Session::new();
        session.render_header(f, f.area());
    }).unwrap();
    
    let buffer = terminal.backend().buffer();
    // Assert expected cell content
}
```

Use `TestBackend` for offline rendering tests (no terminal needed).

## See Also

- [Ratatui: Layout](https://ratatui.rs/how-to/render/layout/)
- [Ratatui: Widgets](https://ratatui.rs/how-to/render/widgets/)
- [Ratatui: Styling](https://ratatui.rs/how-to/render/styling/)
- [Ratatui: Custom Widgets](https://ratatui.rs/how-to/render/custom-widgets/)
- `src/tui.rs` - Terminal event handler
- `vtcode-core/src/ui/tui/session.rs` - Main render orchestration
