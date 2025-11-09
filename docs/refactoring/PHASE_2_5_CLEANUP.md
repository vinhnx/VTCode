# Phase 2.5: Cleanup and Finalization

## Overview

Phase 2.5 completes the manager integration by removing deprecated fields and cleaning up sync calls. All input and scroll logic has been migrated to managers; now we finalize by removing the legacy code paths.

**Status:** In Progress
**Estimated Duration:** 1-2 hours

## Analysis of Deprecated Fields

### Input-Related Deprecated Fields

**Fields:**
```rust
input: String,                      // DEPRECATED - use input_manager
cursor: usize,                      // DEPRECATED - use input_manager
input_history: Vec<String>,        // DEPRECATED - use input_manager
input_history_index: Option<usize>, // DEPRECATED - use input_manager
input_history_draft: Option<String>, // DEPRECATED - use input_manager
last_escape_time: Option<Instant>,  // DEPRECATED - use input_manager
```

**Current Usage:**
- Sync methods are the only places still writing to these fields
- Read paths have been mostly migrated to input_manager
- Some rendering paths may still reference `self.input` or `self.cursor` for backward compatibility

**Migration Status:**
- input_manager handles all input operations ✓
- Scroll manager handles all scroll operations ✓
- Deprecated fields only updated via sync calls

### Scroll-Related Deprecated Fields

**Fields:**
```rust
scroll_offset: usize,                // DEPRECATED - use scroll_manager
transcript_rows: u16,                // DEPRECATED - local layout dimension
transcript_width: u16,               // DEPRECATED - local layout dimension  
transcript_view_top: usize,          // DEPRECATED - use scroll_manager
cached_max_scroll_offset: usize,     // DEPRECATED - computed on demand
scroll_metrics_dirty: bool,          // DEPRECATED - removed in optimization
```

**Current Usage:**
- scroll_offset: Updated by sync_scroll_from_manager, read in rendering
- transcript_rows: Still used for layout calculations (not deprecated)
- cached_max_scroll_offset: Computed but can be eliminated

## Cleanup Strategy

### Step 1: Remove Sync Method Calls (20 minutes)

Find all calls to sync methods and determine if they're needed:

```bash
grep -n "sync_input_from_manager\|sync_input_to_manager\|sync_scroll_from_manager\|sync_scroll_to_manager" session.rs
```

**Expected locations:**
- After clear_input() - needed for render
- After delete operations - needed for render
- After word navigation - needed for render
- After scroll operations - needed for render

**Analysis needed:**
- Can these methods be removed entirely?
- Or should we keep sync at high-level boundaries only?
- Do rendering methods need the old fields?

### Step 2: Evaluate Rendering Dependencies (30 minutes)

Check if rendering paths still read from deprecated fields:

**Candidates:**
- `fn render()` - Does it read `self.input` or `self.cursor`?
- `fn render_input_line()` - Does it read deprecated fields?
- `fn modal rendering` - Does it read `self.input` or `self.cursor`?
- Output/debug methods - Any reads?

### Step 3: Update Rendering Methods (30 minutes)

If rendering still uses deprecated fields:

**Pattern:**
```rust
// OLD: Reading from deprecated fields
let displayed = &self.input[..self.cursor];

// NEW: Reading from manager
let displayed = &self.input_manager.content()[..self.input_manager.cursor()];
```

### Step 4: Remove Deprecated Fields (20 minutes)

Once all code is migrated:

1. Remove fields from struct definition
2. Remove initialization from `Session::new()`
3. Remove sync helper methods
4. Verify code still compiles

### Step 5: Remove Unused Manager Methods (10 minutes)

Some manager methods are currently unused:
- InputManager: `content_mut`, `history_draft`, `is_in_history`, etc.
- ScrollManager: `max_offset`, `set_total_rows`, `invalidate_metrics`, etc.

These can be removed or kept for future use depending on architecture plan.

### Step 6: Final Verification (20 minutes)

- Run `cargo check`
- Run `cargo test`
- Run `cargo clippy`
- Manual testing of input/scroll functionality

## Deprecated Fields Breakdown

### Fields to Remove Completely

These are only written by sync methods and can be removed:

```rust
// Input history - all managed by input_manager now
input_history: Vec<String>,
input_history_index: Option<usize>,
input_history_draft: Option<String>,
last_escape_time: Option<Instant>,

// Scroll state - all managed by scroll_manager now  
scroll_offset: usize,
transcript_view_top: usize,
cached_max_scroll_offset: usize,
scroll_metrics_dirty: bool,
```

### Fields to Keep (Layout/Display State)

These control layout and are NOT managed by managers:

```rust
// These are not deprecated - they control viewport layout
transcript_rows: u16,      // Current viewport height
transcript_width: u16,     // Current viewport width
input_height: u16,         // Height of input area
view_rows: u16,            // Total view height
```

### Fields with Dual Purpose

These might still be needed for some paths:

```rust
// Input content - could be removed if rendering uses input_manager
input: String,             // CHECK: Is this read anywhere?
cursor: usize,             // CHECK: Is this read anywhere?
```

## Verification Checklist

After each step, verify:

- [ ] Code compiles without warnings
- [ ] All tests pass
- [ ] No `self.input` reads outside of sync methods
- [ ] No `self.scroll_offset` reads outside of sync methods
- [ ] No `self.cursor` reads outside of sync methods
- [ ] No `self.input_history` reads anywhere
- [ ] Input insertion still works
- [ ] Input history navigation still works
- [ ] Scroll operations still work
- [ ] Escape key double-tap still works

## Implementation Plan

### Phase 2.5a: Verify Current State (IN PROGRESS)

1. Identify all uses of deprecated fields
2. Map which ones are read vs written
3. Determine which can be safely removed

### Phase 2.5b: Remove Sync Calls (NEXT)

1. Evaluate if sync calls are actually needed
2. Remove or consolidate sync calls
3. Test all functionality still works

### Phase 2.5c: Clean Up Methods (NEXT)

1. Remove unused manager methods
2. Remove sync helper methods
3. Document what methods are public API

### Phase 2.5d: Remove Deprecated Fields (NEXT)

1. Remove fields from struct definition
2. Remove from initialization
3. Run full test suite

### Phase 2.5e: Final Polish (NEXT)

1. Run cargo fmt
2. Run cargo clippy
3. Document completion
4. Update refactoring summary

## Expected Code Impact

### Before Phase 2.5

```
Session struct:
  Fields: 44 (12 deprecated)
  Sync calls: ~20 locations
  Methods: 158
```

### After Phase 2.5

```
Session struct:
  Fields: 32 (all active)
  Sync calls: 0 (removed)
  Methods: ~155 (removed sync methods)
```

### Lines Changed

- Remove ~100 lines (deprecated fields, sync methods)
- Add ~10 lines (direct manager access where needed)
- Net: -90 lines of code

## Notes

- This is the final step of Phase 2 (Manager Integration)
- After this, we move to Phase 3 (Code Deduplication)
- Phase 3 will focus on eliminating duplicate palette rendering code
- Phase 4 will reduce complexity of large methods like process_key()

## Risk Assessment

**Low Risk:**
- Input manager is fully tested and working
- Scroll manager is fully tested and working
- All read paths have been migrated

**Medium Risk:**
- Need to ensure rendering still works after removing fields
- Need to verify no hidden reads of deprecated fields

**Mitigation:**
- Run full test suite after each change
- Check clippy warnings for unused code
- Manual testing of key functionality

## Timeline

```
Step 1: Remove sync calls           20 min ✓
Step 2: Evaluate rendering deps     30 min
Step 3: Update rendering methods    30 min
Step 4: Remove deprecated fields    20 min
Step 5: Remove unused methods       10 min
Step 6: Final verification          20 min
---
Total: 130 minutes (2-2.5 hours)
```

---

**Last updated:** 2025-11-09
**Status:** Phase 2.5 Analysis Complete, Cleanup Ready to Begin
