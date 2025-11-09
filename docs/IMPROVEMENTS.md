# list_files Tool Output Improvements

## Overview

Optimized the `list_files` tool to reduce TUI buffer overflow and improve pagination efficiency. Changed the default page size from 50 to 20 items to align with TUI display limits, eliminating wasteful over-fetching.

## Changes Made

### 1. Reduced Default Page Size (`vtcode-core/src/tools/file_ops.rs`)

**What changed:**
- Changed default `per_page` from 50 to 20 items in `execute_basic_list()`
- This prevents the core tool from fetching more items than the TUI can efficiently display

**Benefits:**
- Less data transferred per request (token efficiency)
- More accurate pagination metadata (doesn't fetch unused items)
- No wasted computation for items that won't be displayed

**Code change:**
```rust
// Before: input.per_page.unwrap_or(50).max(1),
// After:  input.per_page.unwrap_or(20).max(1),
```

### 2. Simplified TUI Rendering (`src/agent/runloop/tool_output/files.rs`)

**What changed:**
- Removed the need for in-app truncation since we now fetch exactly what we display
- Improved pagination summary messaging
- All items from a page are now displayed (no per-page truncation needed)

**Before:**
- Fetched up to 50 items per page
- Displayed only 20 items in TUI
- Added "… and X more items in this page" message

**After:**
- Fetch exactly 20 items per page
- Display all 20 items in TUI
- Cleaner, more efficient rendering

### 3. Enhanced Pagination Messaging

**Improved user feedback:**
- Single page results: `"20 items"`
- Multi-page results: `"Page 1 of ~5 (20 items per page, 427 total)"`
- Clear guidance: `"Use page=N to view other pages (e.g., page=2, page=3)"`

**Benefits:**
- Users see exactly how much pagination work remains
- Clear page navigation instructions
- Estimate of total pages helps with perspective

## Architecture

```
┌─ Core Tool (file_ops.rs)
│  ├─ Default: per_page=20 (optimized for TUI)
│  ├─ Returns: full pagination metadata
│  └─ Supports: custom per_page if needed
│
├─ TUI Renderer (files.rs)
│  ├─ Displays: all items from current page
│  ├─ Shows: clear pagination summary
│  └─ Guidance: navigation hints for multi-page results
│
└─ ACP Bridge (acp/zed.rs)
   ├─ Uses: TOOL_LIST_FILES_SUMMARY_MAX_ITEMS = 20
   └─ Shows: summary with truncation indicator
```

## Testing

- ✅ Compilation verified with `cargo check`
- ✅ Library tests passing (`cargo test --lib`)
- ✅ File paging tests passing (5/5)
- ✅ No new clippy warnings
- ✅ Code formatted with `cargo fmt`

## Token Efficiency Improvements

**Metrics:**
- **Reduced data transfer**: 20 items instead of 50 per default request (60% reduction)
- **Eliminated over-fetching**: No longer retrieve items that won't be displayed
- **Cleaner output**: Direct display of fetched items, no truncation logic overhead
- **Better pagination**: More accurate page count estimates since fetch limit matches display limit

## Backward Compatibility

- ✅ Users can still request `per_page=50` (or any value) explicitly
- ✅ All pagination parameters remain supported
- ✅ Tool output structure unchanged
- ✅ Existing code continues to work
