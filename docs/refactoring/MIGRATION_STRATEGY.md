# Manager Migration Strategy

## Objective

Guide the gradual migration of Session methods from using direct field access to using InputManager and ScrollManager while maintaining backward compatibility and test coverage.

## Core Principle

**One method at a time, with tests after each change**

This approach minimizes risk and makes it easy to identify which change breaks something.

## InputManager Migration

### Methods That Should Use InputManager

These methods currently access `self.input`, `self.cursor`, and input history fields:

#### Priority 1: Core Input Operations

1. **clear_input()** - Line ~2090
   ```rust
   // Before
   pub fn clear_input(&mut self) {
       self.input.clear();
       self.cursor = 0;
       self.scroll_offset = 0;
       self.reset_history_navigation();
   }
   
   // After
   pub fn clear_input(&mut self) {
       self.input_manager.clear();
       self.sync_input_from_manager();
       self.scroll_offset = 0;
   }
   ```

2. **insert_text()** - Line ~2263
   ```rust
   // Before
   pub fn insert_text(&mut self, text: &str) {
       self.input.insert_str(self.cursor, text);
       self.cursor += text.len();
   }
   
   // After
   pub fn insert_text(&mut self, text: &str) {
       self.input_manager.insert_text(text);
       self.sync_input_from_manager();
   }
   ```

3. **move_cursor_left()**, **move_cursor_right()** - Lines ~2299-2327
   - Use `input_manager.move_cursor_left()` / `move_cursor_right()`
   - Call `sync_input_from_manager()` after

4. **process_key()** - Line ~2013 - COMPLEX, do last
   - Handles many keybindings
   - Multiple field accesses
   - Should migrate piece by piece

#### Priority 2: History Operations

5. **add_to_history()** - Line ~2449
   ```rust
   // After
   pub fn add_to_history(&mut self, entry: String) {
       self.input_manager.add_to_history(entry);
       self.sync_input_from_manager();
   }
   ```

6. **History navigation in process_key()** - Lines ~2364-2387
   - Use `input_manager.go_to_previous_history()`
   - Use `input_manager.go_to_next_history()`

#### Priority 3: Input State

7. **set_input()** - Line ~2266
   ```rust
   // Before
   InlineCommand::SetInput(content) => {
       self.input = content;
       self.cursor = self.input.len();
       self.scroll_offset = 0;
       self.reset_history_navigation();
   }
   
   // After
   InlineCommand::SetInput(content) => {
       self.input_manager.set_content(content);
       self.sync_input_from_manager();
       self.scroll_offset = 0;
   }
   ```

### InputManager Migration Checklist

- [ ] clear_input()
- [ ] insert_text()
- [ ] move_cursor_left()
- [ ] move_cursor_right()
- [ ] handle_character_input() (lines ~2241)
- [ ] handle_paste_text() (lines ~2246)
- [ ] add_to_history()
- [ ] History navigation in process_key()
- [ ] SetInput command handling
- [ ] All remaining direct field accesses to input/cursor

## ScrollManager Migration

### Methods That Should Use ScrollManager

#### Priority 1: Core Scroll Operations

1. **scroll_line_down()**, **scroll_line_up()** - Lines ~2455-2485
   ```rust
   // Before
   pub fn scroll_line_down(&mut self) {
       self.scroll_offset = self.scroll_offset.saturating_add(1).min(self.current_max_scroll_offset());
   }
   
   // After
   pub fn scroll_line_down(&mut self) {
       self.scroll_manager.scroll_down(1);
       self.sync_scroll_from_manager();
   }
   ```

2. **scroll_page_down()**, **scroll_page_up()** - Lines ~2488-2510
   - Use `scroll_manager.scroll_page_down()` / `scroll_page_up()`
   - Call `sync_scroll_from_manager()`

3. **scroll_to_bottom()**, **scroll_to_top()** - Lines ~2513-2524
   - Use `scroll_manager.scroll_to_bottom()` / `scroll_to_top()`

#### Priority 2: Scroll Metrics

4. **current_max_scroll_offset()** - Line ~2526
   - Should use `scroll_manager.max_offset()`

5. **enforce_scroll_bounds()** - Line ~2531
   - Should use `scroll_manager.clamp_offset()`

6. **invalidate_scroll_metrics()** - Line ~2535
   - Should use `scroll_manager.invalidate_metrics()`

### ScrollManager Migration Checklist

- [ ] scroll_line_down() / scroll_line_up()
- [ ] scroll_page_down() / scroll_page_up()
- [ ] scroll_to_bottom() / scroll_to_top()
- [ ] current_max_scroll_offset()
- [ ] enforce_scroll_bounds()
- [ ] invalidate_scroll_metrics()
- [ ] All remaining scroll_offset accesses

## Testing Strategy

### After Each Migration

1. Run input_manager or scroll tests specifically
2. Run full session tests
3. Look for any test failures
4. Verify no functional regression

```bash
# After InputManager migration
cargo test -p vtcode-core --lib input_manager
cargo test -p vtcode-core --lib session 2>&1 | grep -E "passed|failed"

# After ScrollManager migration
cargo test -p vtcode-core --lib scroll
cargo test -p vtcode-core --lib session 2>&1 | grep -E "passed|failed"
```

## Migration Order

### Phase 1: Simple Cases (30 min)
- clear_input()
- scroll_line_up() / scroll_line_down()

### Phase 2: Medium Complexity (1 hour)
- insert_text()
- scroll_page_up() / scroll_page_down()
- History operations

### Phase 3: Complex Cases (2+ hours)
- process_key() (many keybindings)
- render_input() (might need scroll info)

## Commit Strategy

One method per commit (or logical group of related methods):

```
refactor: migrate clear_input() to use InputManager
refactor: migrate scroll operations to use ScrollManager
refactor: migrate insert_text() to use InputManager
```

This makes it easy to:
1. Review changes
2. Identify which commit breaks tests
3. Revert if needed

## Cleanup Phase

After all migrations:

1. Remove deprecated fields from Session struct
2. Remove sync helper methods
3. Update documentation
4. Run full test suite

## Code Patterns to Replace

### Pattern 1: Direct Input Field Access
```rust
// OLD
self.input.push_str(text);
self.input.clear();
let len = self.input.len();

// NEW
self.input_manager.insert_text(text);
self.input_manager.clear();
let len = self.input_manager.content().len();
```

### Pattern 2: Cursor Manipulation
```rust
// OLD
self.cursor += 1;
self.cursor = self.input.len();
self.cursor = 0;

// NEW
self.input_manager.move_cursor_right();
self.input_manager.move_cursor_to_end();
self.input_manager.move_cursor_to_start();
```

### Pattern 3: History Navigation
```rust
// OLD
self.input_history.push(entry);
if let Some(h) = self.input_history.get(self.input_history_index.unwrap()) { ... }

// NEW
self.input_manager.add_to_history(entry);
if let Some(h) = self.input_manager.go_to_previous_history() { ... }
```

### Pattern 4: Scroll Operations
```rust
// OLD
self.scroll_offset = min(self.scroll_offset + 1, self.current_max_scroll_offset());
self.scroll_offset = 0;
self.scroll_metrics_dirty = true;

// NEW
self.scroll_manager.scroll_down(1);
self.scroll_manager.scroll_to_top();
self.scroll_manager.invalidate_metrics();
```

## Expected Outcomes

After completing migration:

1. **Reduced Field Count:** 44 → 35 fields (20% reduction)
2. **Better Encapsulation:** Input logic isolated in InputManager
3. **Type Safety:** Cursor position guaranteed to be valid
4. **Testability:** Each manager tested independently
5. **Maintainability:** Changes to input logic affect only InputManager

## Troubleshooting

### Problem: Compilation fails after migration

**Solution:** Check for:
1. Forgetting to call `sync_*_from_manager()` after manager change
2. Forgetting to update callers of migrated methods
3. Using wrong manager method name

### Problem: Test fails after migration

**Solution:**
1. Verify logic is equivalent
2. Check if any other code depends on field being in sync with manager
3. May need to add sync call to handle_event() or handle_command()

### Problem: Double-sync issue

If you see the field being updated twice, you might be:
1. Updating the manager
2. Syncing from manager
3. But also updating the field

Solution: Use only the manager, sync once.

## Next Steps

1. Start with Phase 1 simple cases
2. Run tests after each change
3. Document any issues discovered
4. Move to Phase 2 and 3
5. Final cleanup and removal of deprecated code
