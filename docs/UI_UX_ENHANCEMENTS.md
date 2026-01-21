# VT Code UI/UX Enhancements

This document describes the UI/UX improvements implemented for VT Code's terminal interface, focusing on **clean, grouped message rendering** and responsive layout.

## Overview

The improvements focus on two areas:

1. **Transcript Rendering** - Better visual grouping of messages, tool output, and todo items
2. **Responsive Layout** - Adaptive layout based on terminal size (with minimal chrome in standard mode)

## Transcript Rendering Improvements

### Message Block Grouping

Messages are now visually grouped with consistent spacing:

- **User messages**: Single divider before, blank line after
- **Agent messages**: Blank line after when followed by different message type
- **Tool blocks**: Grouped with blank line before/after, consistent indentation
- **Error/Info/Warning**: Blank line after for separation

### Todo/Checkbox Styling

Todo items are automatically detected and styled:

```markdown
- [ ] Pending item (normal style)
- [x] Completed item (dimmed + strikethrough)
```

Patterns detected:
- `- [ ]`, `* [ ]`, `+ [ ]`, `[ ]` → Pending
- `- [x]`, `- [X]`, `* [x]`, `[x]` → Completed (dimmed)
- `~~strikethrough~~` → Completed (dimmed)

### Tool Output Grouping

Tool output is now visually grouped:
- Blank line before tool block starts
- Consistent 2-space indentation
- Dimmed styling for less visual weight
- Blank line after tool block ends

## Layout System

### LayoutMode

The `LayoutMode` enum provides responsive layout decisions:

```rust
pub enum LayoutMode {
    Compact,   // < 80 cols or < 20 rows: no borders, no footer
    Standard,  // 80-119 cols: borders/titles, no footer (preserves space)
    Wide,      // >= 120 cols: full layout with sidebar and footer
}
```

**Key principle**: Maximize transcript space in standard terminals.

### Layout Regions

```
┌─────────────────────────────────────────┐
│ Header: model, git, tokens, status      │
├─────────────────────────────────────────┤
│                           │             │
│ Main: Transcript          │ Sidebar     │  ← Wide mode only
│ (+ optional logs panel)   │ (Queue,     │
│                           │  Context)   │
├─────────────────────────────────────────┤  ← Wide mode only
│ Footer: status │ hints                  │
└─────────────────────────────────────────┘
```

## New Widgets

### Panel

A consistent wrapper that applies standardized chrome (borders, titles):

```rust
let inner = Panel::new(&styles)
    .title("Transcript")
    .active(is_focused)
    .mode(layout_mode)
    .render_and_get_inner(area, buf);

// Render child widget into `inner`
ChildWidget::new().render(inner, buf);
```

**Features:**
- Consistent border styling based on theme
- Active/inactive state (highlighted vs dimmed borders)
- Respects layout mode (no borders in Compact)
- Titles shown only in Standard/Wide modes

### FooterWidget

Renders the footer with status and contextual hints:

```rust
FooterWidget::new(&styles)
    .left_status("main ✓")
    .right_status("claude-4 | 12K tokens")
    .hint(footer_hints::IDLE)
    .spinner("⠋")  // optional, when processing
    .mode(layout_mode)
    .render(footer_area, buf);
```

**Contextual Hints:**
- `IDLE`: "? help • / command • @ file • # prompt"
- `PROCESSING`: "Ctrl+C cancel"
- `MODAL`: "↑↓ navigate • Enter select • Esc close"
- `EDITING`: "Enter send • Ctrl+C cancel • ↑ history"

### SidebarWidget

Displays queue, context, and tool information in wide mode:

```rust
SidebarWidget::new(&styles)
    .queue_items(queued_inputs)
    .context_info("12K tokens | 45% context")
    .recent_tools(tool_names)
    .active_section(SidebarSection::Queue)
    .mode(layout_mode)
    .render(sidebar_area, buf);
```

**Sections:**
- **Queue**: Pending inputs/tasks
- **Context**: Token usage and context info
- **Tools**: Recent tool calls

## Style Extensions

### PanelStyles Trait

Extends `SessionStyles` with semantic styles for visual hierarchy:

```rust
impl PanelStyles for SessionStyles {
    fn muted_style(&self) -> Style;        // Dimmed secondary content
    fn title_style(&self) -> Style;        // Bold accent for titles
    fn border_active_style(&self) -> Style; // Focused panel border
    fn divider_style(&self) -> Style;      // Section separators
}
```

## Spinner Enhancement

The `ThinkingSpinner` now provides a `current_frame()` method with smooth Braille animation:

```rust
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn current_frame(&self) -> &'static str {
    SPINNER_FRAMES[self.spinner_index % SPINNER_FRAMES.len()]
}
```

## Progressive Disclosure

The layout adapts based on terminal size:

| Mode | Borders | Titles | Logs Panel | Sidebar | Footer Height |
|------|---------|--------|------------|---------|---------------|
| Compact | ❌ | ❌ | ❌ | ❌ | 1 |
| Standard | ✅ | ✅ | ✅ | ❌ | 2 |
| Wide | ✅ | ✅ | ✅ | ✅ | 2 |

## File Structure

```
vtcode-core/src/ui/tui/widgets/
├── mod.rs           # Module exports and documentation
├── layout_mode.rs   # LayoutMode enum and responsive logic
├── panel.rs         # Panel wrapper and PanelStyles trait
├── footer.rs        # FooterWidget and hint constants
├── sidebar.rs       # SidebarWidget with sections
├── session.rs       # SessionWidget (updated for 3-region layout)
├── header.rs        # HeaderWidget
├── transcript.rs    # TranscriptWidget
└── ...
```

## Usage Example

```rust
// In session rendering
let mode = LayoutMode::from_area(viewport);
let layout = compute_layout(viewport, mode);

// Render with consistent styling
HeaderWidget::new(session).render(layout.header, buf);

let inner = Panel::new(&styles)
    .title("Transcript")
    .mode(mode)
    .render_and_get_inner(layout.main, buf);
TranscriptWidget::new(session).render(inner, buf);

if let Some(sidebar) = layout.sidebar {
    SidebarWidget::new(&styles)
        .queue_items(queue)
        .mode(mode)
        .render(sidebar, buf);
}

FooterWidget::new(&styles)
    .left_status(git_status)
    .right_status(model_info)
    .hint(current_hint)
    .mode(mode)
    .render(layout.footer, buf);
```

## Design Principles

1. **Transcript Clarity First**: Prioritize readable message flow over layout features
2. **Maximize Reading Space**: No footer in Standard mode preserves transcript area
3. **Visual Grouping**: Blank lines and indentation create logical blocks
4. **Progressive Disclosure**: Show less chrome in small terminals
5. **Semantic Styling**: Completed items dimmed, tools indented, turns separated

## Testing

Tests are included for:
- `detect_todo_state()` - Pending, Completed, None detection
- `is_list_item()` - Bullet and numbered list detection
- `LayoutMode` - Boundary conditions and mode properties

Run tests with:
```bash
cargo test --package vtcode-core text_utils
cargo test --package vtcode-core layout_mode
```
