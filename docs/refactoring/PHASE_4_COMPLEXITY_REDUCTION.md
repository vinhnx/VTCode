# Phase 4: Complexity Reduction - Implementation Plan

## Overview

Phase 4 targets the highest-complexity functions in the session module. The goal is to break down functions with cyclomatic complexity >15 into smaller, focused handlers with CC <10 each.

**Key Functions to Refactor:**
1. **process_key()** (lines 2013-2150, CC ~35) - Keyboard event handler
2. **render_message_spans()** (lines 970-1100, CC ~18) - Message rendering dispatcher
3. **render_tool_header_line()** (lines 1189-1304, CC ~20) - Tool message styling

**Target Impact:**
- Reduce max CC from ~35 to <10
- Maintain 100% test coverage
- Improve readability and debuggability
- Make key bindings explicit and testable

**Estimated Effort:** 4-5 hours

---

## 4.1 Break Down process_key()

### Problem Analysis

**Current structure (lines 2013-2150, 237 lines):**
```rust
fn process_key(&mut self, key: KeyEvent) -> Option<InlineEvent> {
    match key.code {
        KeyCode::Enter => {
            // 15 lines of entry logic
            // Handles history, submission, various states
        }
        KeyCode::Tab => {
            // 8 lines of autocomplete
        }
        KeyCode::Up | KeyCode::Down => {
            // Complex navigation logic with multiple branches
        }
        // ... 20 more match arms
        _ => None,
    }
}
```

**Complexity breakdown:**
- Top-level match: 20+ arms = CC 20
- Nested conditions within arms: +15
- Total estimated: CC ~35

**Issues:**
- Single function handles 20+ distinct key types
- Nested conditions for modifiers, states, palettes
- History navigation, modal handling mixed together
- Difficult to add new key bindings
- Hard to test individual key handlers
- Line too long to understand at once

### Solution Design

**Extract key handlers into focused methods:**

```rust
fn process_key(&mut self, key: KeyEvent) -> Option<InlineEvent> {
    match key.code {
        KeyCode::Enter => self.handle_enter_key(key.modifiers),
        KeyCode::Tab => self.handle_tab_key(key.modifiers),
        KeyCode::Up => self.handle_up_key(key.modifiers),
        KeyCode::Down => self.handle_down_key(key.modifiers),
        KeyCode::Left => self.handle_left_key(key.modifiers),
        KeyCode::Right => self.handle_right_key(key.modifiers),
        KeyCode::Home => self.handle_home_key(key.modifiers),
        KeyCode::End => self.handle_end_key(key.modifiers),
        KeyCode::PageUp => self.handle_page_up_key(),
        KeyCode::PageDown => self.handle_page_down_key(),
        KeyCode::Delete => self.handle_delete_key(key.modifiers),
        KeyCode::Backspace => self.handle_backspace_key(key.modifiers),
        KeyCode::Esc => self.handle_escape_key(),
        KeyCode::Char(c) => self.handle_character_key(c, key.modifiers),
        KeyCode::F(n) => self.handle_function_key(n),
        _ => None,
    }
}

// CC ~5 each: handles single character input
fn handle_character_key(&mut self, c: char, modifiers: KeyModifiers) -> Option<InlineEvent> {
    if self.modal.is_some() {
        self.handle_modal_char(c)
    } else if self.slash_palette.is_active() {
        self.handle_slash_char(c)
    } else if self.file_palette_active {
        self.handle_file_palette_char(c)
    } else if self.prompt_palette_active {
        self.handle_prompt_palette_char(c)
    } else if self.input_enabled {
        self.handle_input_char(c, modifiers)
    } else {
        None
    }
}

// CC ~3: handles enter in various contexts
fn handle_enter_key(&mut self, modifiers: KeyModifiers) -> Option<InlineEvent> {
    if self.modal.is_some() {
        self.handle_modal_enter()
    } else if self.file_palette_active {
        self.handle_file_palette_enter()
    } else if self.prompt_palette_active {
        self.handle_prompt_palette_enter()
    } else if self.input_enabled {
        self.handle_input_enter(modifiers)
    } else {
        None
    }
}

// CC ~4: handles tab completion
fn handle_tab_key(&mut self, modifiers: KeyModifiers) -> Option<InlineEvent> {
    if modifiers.contains(KeyModifiers::SHIFT) {
        self.handle_shift_tab()
    } else if self.input_enabled {
        self.handle_input_tab()
    } else if self.file_palette_active {
        self.handle_file_palette_tab()
    } else {
        None
    }
}

// CC ~3: handles up navigation
fn handle_up_key(&mut self, modifiers: KeyModifiers) -> Option<InlineEvent> {
    if self.modal.is_some() {
        self.handle_modal_up()
    } else if self.file_palette_active {
        self.handle_file_palette_up()
    } else if self.prompt_palette_active {
        self.handle_prompt_palette_up()
    } else if modifiers.contains(KeyModifiers::CTRL) && self.input_enabled {
        self.apply_history_entry(self.input_manager.go_to_previous_history())
    } else if self.input_enabled {
        self.move_cursor_up()
    } else {
        None
    }
}

// CC ~3: handles down navigation
fn handle_down_key(&mut self, modifiers: KeyModifiers) -> Option<InlineEvent> {
    if self.modal.is_some() {
        self.handle_modal_down()
    } else if self.file_palette_active {
        self.handle_file_palette_down()
    } else if self.prompt_palette_active {
        self.handle_prompt_palette_down()
    } else if modifiers.contains(KeyModifiers::CTRL) && self.input_enabled {
        self.apply_history_entry(self.input_manager.go_to_next_history())
    } else if self.input_enabled {
        self.move_cursor_down()
    } else {
        None
    }
}

// CC ~2: horizontal navigation
fn handle_left_key(&mut self, modifiers: KeyModifiers) -> Option<InlineEvent> {
    if modifiers.contains(KeyModifiers::ALT) {
        self.move_left_word()
    } else if self.input_enabled {
        self.input_manager.move_cursor_left();
    }
    None
}

fn handle_right_key(&mut self, modifiers: KeyModifiers) -> Option<InlineEvent> {
    if modifiers.contains(KeyModifiers::ALT) {
        self.move_right_word()
    } else if self.input_enabled {
        self.input_manager.move_cursor_right();
    }
    None
}

fn handle_home_key(&mut self, _modifiers: KeyModifiers) -> Option<InlineEvent> {
    if self.input_enabled {
        self.input_manager.move_cursor_to_start();
    }
    None
}

fn handle_end_key(&mut self, _modifiers: KeyModifiers) -> Option<InlineEvent> {
    if self.input_enabled {
        self.input_manager.move_cursor_to_end();
    }
    None
}

// CC ~2: page navigation
fn handle_page_up_key(&mut self) -> Option<InlineEvent> {
    if self.modal.is_none() && !self.file_palette_active && !self.prompt_palette_active {
        self.scroll_page_up();
        self.mark_dirty();
    }
    None
}

fn handle_page_down_key(&mut self) -> Option<InlineEvent> {
    if self.modal.is_none() && !self.file_palette_active && !self.prompt_palette_active {
        self.scroll_page_down();
        self.mark_dirty();
    }
    None
}

// CC ~4: deletion handling
fn handle_delete_key(&mut self, modifiers: KeyModifiers) -> Option<InlineEvent> {
    if self.input_enabled {
        if modifiers.contains(KeyModifiers::CTRL) {
            self.delete_word_forward();
        } else {
            self.input_manager.delete();
        }
        self.mark_dirty();
    }
    None
}

fn handle_backspace_key(&mut self, modifiers: KeyModifiers) -> Option<InlineEvent> {
    if self.input_enabled {
        if modifiers.contains(KeyModifiers::CTRL) {
            self.delete_word_backward();
        } else {
            self.input_manager.backspace();
        }
        self.mark_dirty();
    }
    None
}

// CC ~3: escape handling
fn handle_escape_key(&mut self) -> Option<InlineEvent> {
    if self.modal.is_some() {
        self.close_modal();
        self.mark_dirty();
    } else if self.file_palette_active || self.prompt_palette_active {
        self.close_palette();
        self.mark_dirty();
    } else if self.input_manager.check_escape_double_tap() {
        Some(InlineEvent::RequestExit)
    } else {
        self.mark_dirty();
        None
    }
}

// CC ~2: function key handling
fn handle_function_key(&mut self, n: u8) -> Option<InlineEvent> {
    match n {
        1 => self.show_help_modal(),
        2 => self.show_settings_modal(),
        // ... other F-keys
        _ => None,
    }
}
```

### Implementation Steps

**Step 4.1.1: Create main process_key dispatcher (30 min)**
- Extract match statement structure
- Create empty handler methods with todo!()
- Run tests (should still pass with todo)

**Step 4.1.2: Implement character handler (45 min)**
- Extract all Char(c) logic
- Handle modal, palette, and input contexts
- Test with various character inputs
- Time: 45 minutes
- Risk: Medium (complex context switching)

**Step 4.1.3: Implement enter handler (30 min)**
- Extract Enter key logic
- Handle submission, modal confirm, palette selection
- Test in all contexts

**Step 4.1.4: Implement tab handler (20 min)**
- Extract Tab key logic
- Handle shift+tab vs tab
- Test autocomplete flow

**Step 4.1.5: Implement navigation handlers (45 min)**
- Up/Down with history and palette navigation
- Left/Right with word boundary detection
- Home/End, PageUp/PageDown
- Test all navigation paths

**Step 4.1.6: Implement deletion handlers (20 min)**
- Delete/Backspace with word and sentence deletion
- Test character and word boundaries

**Step 4.1.7: Implement escape and function key handlers (20 min)**
- Escape with double-tap detection
- F1-F12 dispatching
- Modal closing logic

**Step 4.1.8: Test and validate (30 min)**
- Run full test suite
- Verify all key combinations work
- Check for regressions
- Performance validation

### Expected Outcomes

**Complexity reduction:**
- Original process_key(): CC ~35
- New process_key(): CC ~5 (simple dispatcher)
- Individual handlers: CC 2-5 each
- Total reduction: ~25 CC

**Code quality:**
- Each handler is 10-20 lines
- Single responsibility per handler
- Easy to test individually
- Clear key binding documentation

**Testability:**
```rust
#[cfg(test)]
mod process_key_tests {
    #[test]
    fn enter_submits_input() { /* ... */ }
    
    #[test]
    fn tab_completes_at_cursor() { /* ... */ }
    
    #[test]
    fn up_navigates_history() { /* ... */ }
    
    #[test]
    fn escape_double_tap_exits() { /* ... */ }
    
    #[test]
    fn ctrl_delete_removes_word() { /* ... */ }
}
```

---

## 4.2 Break Down render_message_spans()

### Problem Analysis

**Current structure (lines 970-1100, ~130 lines):**
```rust
fn render_message_spans(&self, index: usize) -> Vec<Span<'static>> {
    let Some(line) = self.lines.get(index) else {
        return vec![];
    };
    
    match line.kind {
        InlineMessageKind::Agent => {
            // 20+ lines of agent-specific rendering
            // Label styling, padding, content formatting
        }
        InlineMessageKind::User => {
            // 15+ lines of user-specific rendering
        }
        InlineMessageKind::Tool => {
            // 35+ lines of tool-specific rendering (most complex)
            // Header parsing, tool name detection, color mapping
        }
        InlineMessageKind::Pty => {
            // 20+ lines of PTY output rendering
        }
        InlineMessageKind::Info | InlineMessageKind::Error => {
            // 10+ lines of info/error rendering
        }
    }
}
```

**Complexity breakdown:**
- Top-level match: 5 arms = CC 5
- Nested conditions in Tool branch: +10 (complex)
- Color selection logic: +3
- Total: CC ~18

**Issues:**
- 130 lines in single function
- Tool rendering takes 35 lines within one match arm
- Hard to test individual message type rendering
- Color and style logic intertwined with content logic
- Adding new message types requires editing this function

### Solution Design

**Extract renderer for each message type:**

```rust
fn render_message_spans(&self, index: usize) -> Vec<Span<'static>> {
    let Some(line) = self.lines.get(index) else {
        return vec![];
    };
    
    match line.kind {
        InlineMessageKind::Agent => self.render_agent_spans(line),
        InlineMessageKind::User => self.render_user_spans(line),
        InlineMessageKind::Tool => self.render_tool_spans(line),
        InlineMessageKind::Pty => self.render_pty_spans(line),
        InlineMessageKind::Info => self.render_info_spans(line),
        InlineMessageKind::Error => self.render_error_spans(line),
    }
}

// CC ~3: agent message styling
fn render_agent_spans(&self, line: &MessageLine) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    
    // Add agent label if configured
    if let Some(label) = &self.labels.agent {
        let label_style = ratatui_style_from_inline(&InlineTextStyle::default(), Some(self.theme.primary));
        spans.push(Span::styled(label.clone(), label_style));
    }
    
    // Add padding
    spans.push(Span::raw(ui::INLINE_AGENT_MESSAGE_LEFT_PADDING));
    
    // Add content
    for segment in &line.segments {
        let style = ratatui_style_from_inline(&segment.style, None);
        spans.push(Span::styled(segment.text.clone(), style));
    }
    
    spans
}

// CC ~2: user message styling
fn render_user_spans(&self, line: &MessageLine) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    
    if let Some(label) = &self.labels.user {
        let label_style = ratatui_style_from_inline(&InlineTextStyle::default(), None);
        spans.push(Span::styled(label.clone(), label_style));
    }
    
    for segment in &line.segments {
        let style = ratatui_style_from_inline(&segment.style, None);
        spans.push(Span::styled(segment.text.clone(), style));
    }
    
    spans
}

// CC ~8: tool message with header parsing (most complex)
fn render_tool_spans(&self, line: &MessageLine) -> Vec<Span<'static>> {
    if line.segments.is_empty() {
        return vec![];
    }
    
    let content = &line.segments[0].text;
    
    if self.is_tool_header_line(content) {
        self.render_tool_header(content)
    } else {
        self.render_tool_body(line)
    }
}

// CC ~6: tool header parsing
fn render_tool_header(&self, header: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    
    // Tool indicator
    spans.push(Span::styled(
        format!("[{}]", ui::INLINE_TOOL_HEADER_LABEL),
        self.tool_label_style(),
    ));
    
    // Extract and style tool name
    if let Some(name) = self.extract_tool_name(header) {
        spans.push(Span::styled(
            format!("[{}]", name),
            self.tool_name_style(&name),
        ));
    }
    
    // Remaining text (e.g., "executing", "completed")
    if let Some(tail) = self.extract_tool_tail(header) {
        spans.push(Span::styled(
            tail,
            self.tool_tail_style(),
        ));
    }
    
    spans
}

// CC ~2: tool body rendering
fn render_tool_body(&self, line: &MessageLine) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    
    for segment in &line.segments {
        let style = ratatui_style_from_inline(&segment.style, None);
        spans.push(Span::styled(segment.text.clone(), style));
    }
    
    spans
}

// CC ~3: PTY output rendering
fn render_pty_spans(&self, line: &MessageLine) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    
    for segment in &line.segments {
        let style = ratatui_style_from_inline(&segment.style, None);
        spans.push(Span::styled(segment.text.clone(), style));
    }
    
    spans
}

// CC ~2: info message styling
fn render_info_spans(&self, line: &MessageLine) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    
    for segment in &line.segments {
        let style = ratatui_style_from_inline(&segment.style, Some(AnsiColorEnum::Cyan));
        spans.push(Span::styled(segment.text.clone(), style));
    }
    
    spans
}

// CC ~2: error message styling
fn render_error_spans(&self, line: &MessageLine) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    
    for segment in &line.segments {
        let style = ratatui_style_from_inline(&segment.style, Some(AnsiColorEnum::Red));
        spans.push(Span::styled(segment.text.clone(), style));
    }
    
    spans
}
```

### Implementation Steps

**Step 4.2.1: Create dispatcher function (20 min)**
- Extract match statement
- Create empty handler methods
- Run tests

**Step 4.2.2: Implement agent/user renderers (30 min)**
- Extract label styling
- Extract content rendering
- Test agent and user message rendering

**Step 4.2.3: Implement PTY/Info/Error renderers (30 min)**
- Extract simple message type rendering
- Test rendering of each type

**Step 4.2.4: Implement tool header renderer (60 min)**
- Extract tool name detection
- Extract header parsing logic
- Create tool name and tail style helpers
- Test all tool message variations

**Step 4.2.5: Test and validate (30 min)**
- Run full test suite
- Verify visual rendering unchanged
- Check for edge cases

### Expected Outcomes

**Complexity reduction:**
- Original function: CC ~18
- New dispatcher: CC ~5
- Individual renderers: CC 2-8
- Each renderer is single-purpose

**Code quality:**
- Each renderer is 10-25 lines
- Clear separation of concerns
- Easy to extend for new message types
- Better code reusability

---

## 4.3 Break Down render_tool_header_line()

### Problem Analysis

**Current structure (lines 1189-1304, ~115 lines):**
```rust
fn render_tool_header_line(&self, line: &str) -> Vec<Span<'static>> {
    // Complex parsing and styling logic
    // 10+ nested conditions
    // Tool name extraction
    // Parameter formatting
    // Color mapping
    // Total: CC ~20
}
```

**Issues:**
- Complex parsing mixed with styling
- Hardcoded patterns and regular expressions
- Multiple responsibilities (parse, format, style)
- Difficult to test parsing logic in isolation

### Solution Design

**Extract concerns into focused methods:**
```rust
fn render_tool_header_line(&self, line: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    
    // Extract components
    let (indent, tool_name, tail) = self.parse_tool_header(line);
    
    // Render indent
    if !indent.is_empty() {
        spans.push(Span::raw(indent));
    }
    
    // Render tool label
    spans.push(Span::styled(
        format!("[{}]", ui::INLINE_TOOL_HEADER_LABEL),
        self.tool_label_style(),
    ));
    
    // Render tool name
    if !tool_name.is_empty() {
        spans.push(Span::styled(
            format!("[{}]", tool_name),
            self.tool_name_style(&tool_name),
        ));
    }
    
    // Render tail
    if !tail.is_empty() {
        spans.push(Span::styled(
            tail,
            self.tool_tail_style(),
        ));
    }
    
    spans
}

// CC ~4: parsing logic isolated
fn parse_tool_header(&self, line: &str) -> (&str, &str, &str) {
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];
    
    // Extract tool name from [name] pattern
    let tool_name = if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed[start..].find(']') {
            &trimmed[start+1..start+end]
        } else {
            ""
        }
    } else {
        ""
    };
    
    // Extract tail (remaining text after tool name)
    let tail = if let Some(idx) = trimmed.find(']') {
        trimmed[idx+1..].trim()
    } else {
        trimmed
    };
    
    (indent, tool_name, tail)
}

// CC ~3: styling logic
fn tool_label_style(&self) -> Style {
    Style::default()
        .fg(ratatui_color_from_ansi(self.theme.primary))
        .add_modifier(Modifier::BOLD)
}

fn tool_name_style(&self, _name: &str) -> Style {
    Style::default()
        .fg(ratatui_color_from_ansi(self.theme.primary))
        .add_modifier(Modifier::BOLD)
}

fn tool_tail_style(&self) -> Style {
    Style::default()
        .add_modifier(Modifier::ITALIC)
}
```

### Implementation Steps

**Step 4.3.1: Extract parsing logic (30 min)**
- Create parse_tool_header() method
- Test parsing with various inputs
- Verify correctness

**Step 4.3.2: Extract styling methods (20 min)**
- Create tool_label_style()
- Create tool_name_style()
- Create tool_tail_style()

**Step 4.3.3: Refactor main function (20 min)**
- Simplify render_tool_header_line()
- Use extracted methods
- Run tests

**Step 4.3.4: Test and validate (20 min)**
- Verify tool header rendering
- Test various tool names and formats
- Check for regressions

### Expected Outcomes

**Complexity reduction:**
- Original: CC ~20
- New main function: CC ~5
- Parsing logic: CC ~4
- Styling methods: CC ~2 each
- Total reduction: ~15 CC

---

## Integration and Validation

### Testing Checklist

- [ ] All key handler tests pass
- [ ] All message renderer tests pass
- [ ] All tool header tests pass
- [ ] Full session test suite passes
- [ ] No performance regressions
- [ ] Visual rendering unchanged
- [ ] No clippy warnings

### Metrics Before/After Phase 4

```
Before Phase 4:
- Max Cyclomatic Complexity: ~35 (process_key)
- Functions: ~150
- File size: ~4,600 lines

After Phase 4:
- Max Cyclomatic Complexity: ~8
- Functions: ~180 (+30 small handlers)
- File size: ~4,800 lines (+200 comments/spacing)

But quality metrics:
- Average CC reduced from ~8 to ~5
- Maintainability index increased significantly
- Code reusability improved
- Test coverage maintained at 100%
```

---

## Timeline

| Phase | Task | Est. Time | Risk |
|-------|------|-----------|------|
| 4.1 | process_key extraction | 4 hours | Medium |
| 4.2 | render_message_spans extraction | 2.5 hours | Medium |
| 4.3 | render_tool_header extraction | 1.5 hours | Low |
| **Total** | **Phase 4 Completion** | **8 hours** | **Medium** |

---

## Acceptance Criteria

- [ ] All existing tests pass (200+)
- [ ] Cyclomatic complexity of all functions <10
- [ ] No performance regressions detected
- [ ] Visual rendering identical to before
- [ ] Public API unchanged
- [ ] Code follows AGENTS.md style guide
- [ ] Each handler is easily testable
- [ ] Key binding logic is explicit and documented

---

## Transition to Phase 5

Phase 5 will focus on final optimizations:
- Optimize scroll metrics caching
- Reduce string allocations in hot paths
- Profile and benchmark key operations
- Final code cleanup and documentation

**Estimated effort:** 2-3 hours
