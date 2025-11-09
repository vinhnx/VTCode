# Phase 2: Manager Integration Plan

## Overview

Phase 2 integrates the three extracted managers (InputManager, ScrollManager) into the Session struct. This is a delicate refactoring that must maintain backward compatibility and functionality while reducing field count and improving code organization.

## Current State

**Session struct:** 44 fields, 4,859 lines of code

**Managers ready for integration:**
- InputManager (9 fields → 1 field)
- ScrollManager (6 fields → 1 field)

## Integration Strategy

### Approach: Gradual Field Replacement

Rather than replacing all fields at once, we'll follow this strategy:

1. **Add manager fields to Session** (non-breaking)
2. **Gradually migrate methods** to use managers instead of direct field access
3. **Deprecate old field access** patterns through private helpers
4. **Test thoroughly** at each step

### Why This Approach?

- **Reduces risk:** Each change is small and testable
- **Maintains functionality:** Old tests continue to pass
- **Clear audit trail:** Git history shows exactly what changed and why
- **Easy rollback:** If needed, can revert specific changes

## Phase 2 Breakdown

### Step 1: Add Manager Fields (30 minutes)

**Task:** Add manager fields to Session struct while keeping old fields temporarily

```rust
pub struct Session {
    // NEW: Managers
    input_manager: InputManager,
    scroll_manager: ScrollManager,
    
    // OLD: Keep for now (will be removed later)
    input: String,
    cursor: usize,
    // ... etc
}
```

**Rationale:** This allows gradual migration without breaking existing code.

### Step 2: Update Session::new() (30 minutes)

Initialize managers in constructor:

```rust
impl Session {
    pub fn new(...) -> Self {
        let mut session = Self {
            input_manager: InputManager::new(),
            scroll_manager: ScrollManager::new(10), // Will be updated
            // ... rest of initialization
        };
        // Sync managers with legacy fields
        session.sync_input_manager();
        session
    }
}
```

### Step 3: Create Helper Methods (1 hour)

Create helper methods to sync between managers and legacy fields:

```rust
impl Session {
    /// Sync session's input to manager
    fn update_input_manager(&mut self) {
        self.input_manager.set_content(self.input.clone());
    }
    
    /// Sync manager's input back to session
    fn sync_input_from_manager(&mut self) {
        self.input = self.input_manager.content().to_string();
        self.cursor = self.input_manager.cursor();
    }
}
```

**Why needed:** Allows gradual replacement of field access while keeping code working.

### Step 4: Migrate Input-Related Methods (2-3 hours)

Gradually replace field access with manager calls:

**Before:**
```rust
pub fn insert_text(&mut self, text: &str) {
    self.input.insert_str(self.cursor, text);
    self.cursor += text.len();
}
```

**After:**
```rust
pub fn insert_text(&mut self, text: &str) {
    self.input_manager.insert_text(text);
    self.sync_input_from_manager();
}
```

### Step 5: Migrate Scroll-Related Methods (2-3 hours)

Similar pattern for scroll methods.

### Step 6: Testing & Cleanup (1-2 hours)

- Run all tests
- Remove temporary sync helpers if no longer needed
- Clean up code

## Estimated Timeline

- **Step 1:** 30 min
- **Step 2:** 30 min
- **Step 3:** 1 hour
- **Step 4:** 2-3 hours
- **Step 5:** 2-3 hours
- **Step 6:** 1-2 hours

**Total:** ~8-10 hours

## Key Considerations

### Testing

1. Run tests after each step
2. Maintain 100% of existing test coverage
3. Add integration tests for manager usage

### Documentation

1. Document sync patterns
2. Explain why gradual migration was chosen
3. Note what will be removed in later phases

### Code Review

1. Each commit should be reviewable (not too large)
2. Clear commit messages explaining why
3. Link to this document in commit messages

## Backward Compatibility

✓ **Fully maintained**

- Session public API unchanged
- All existing tests continue to pass
- No changes to external interfaces
- Users of Session see no difference

## Success Criteria

- [ ] InputManager integrated (input, cursor, history)
- [ ] ScrollManager integrated (scroll_offset, transcript_rows)
- [ ] All tests passing
- [ ] No performance regressions
- [ ] Code is cleaner and more maintainable
- [ ] Field count reduced from 44 to ~32

## Risk Assessment

### Low Risk

- Small, focused changes
- Each step is testable independently
- Can roll back individual changes
- Existing tests provide safety net

### Mitigation

- Run tests frequently
- Create feature branch if unsure
- Get code review before merging
- Document any tricky transitions

## Next Steps

1. ✓ Ensure InputManager & ScrollManager modules are added
2. ✓ Update mod declarations and imports
3. Begin Step 1: Add manager fields to Session
4. Work through steps in order
5. Document any discoveries or adjustments

## Notes

This document will be updated as we progress through the steps. Any deviations from the plan should be documented here with rationale.
