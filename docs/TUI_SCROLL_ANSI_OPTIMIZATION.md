# TUI Scroll Performance & ANSI Rendering Optimization Guide

## Overview

This document outlines optimization strategies for scroll performance and ANSI code rendering in the VT Code TUI transcript system. The current implementation has strong foundations with caching and efficient algorithms, but additional optimizations can further improve rendering speed, especially for large transcripts with heavy ANSI content.

---

## Current Architecture

### 1. **Scroll Performance Layer** (`scroll.rs`)
- **ScrollManager**: Efficient scroll state management with metrics caching
- **Benefits**: O(1) scroll calculations, bounded offset clamping
- **Metrics caching**: Invalidation tracking to avoid unnecessary recalculations

### 2. **Transcript Reflow Cache** (`transcript.rs`)
- **TranscriptReflowCache**: Width-specific line wrapping cache
- **Key optimizations**:
  - Binary search on `row_offsets` for fast message lookup
  - Pre-computed `row_offsets` to map global row → message index
  - Content hash tracking to detect changes
  - `get_visible_range()` to extract only viewport lines

### 3. **Viewport Caching** (`session.rs`)
- **visible_lines_cache**: Arc-based zero-copy cache for current viewport
- **Enables**: Reuse of rendered lines when scroll position unchanged
- **Benefit**: Avoids cloning large line vectors on every render

### 4. **ANSI Code Handling** (`ansi.rs`)
- **InlineSink.convert_plain_lines()**: Parses ANSI codes from text
- **Fallback logic**: Plain text processing if ANSI parsing fails
- **UTF-8 validation**: Ensures multi-byte characters work with ANSI codes

---

## Performance Bottlenecks & Solutions

### Bottleneck 1: Large Transcript Reflow on Every Render

**Problem**: If transcript content changes or viewport resizes, reflowing all messages is expensive.

**Current Mitigation**:
- Dirty message tracking (first_dirty index)
- Hash-based change detection
- Only reflow messages that changed

**Further Optimization**:
```rust
// In ensure_reflow_cache() - implement incremental reflow
// Only reflow from first_dirty message onward
for msg_idx in first_dirty..cache.messages.len() {
    if cache.needs_reflow(msg_idx, self.lines[msg_idx].revision) {
        // Reflow this message only
        let reflowed = self.reflow_message(msg_idx, width);
        cache.update_message(msg_idx, self.lines[msg_idx].revision, reflowed);
    }
}
```

**Impact**: Scales linearly with actual changes, not total transcript size.

---

### Bottleneck 2: ANSI Parsing on Every Line Conversion

**Problem**: `InlineSink.convert_plain_lines()` calls `ansi-to-tui` on every tool output line.

**Current Implementation**:
```rust
// In convert_plain_lines():
if let Ok(parsed) = text.as_bytes().into_text() {
    // Parse ANSI codes
    for line in &parsed.lines { ... }
}
```

**Optimization Strategy 1: Cache ANSI Parse Results**
```rust
// Add to InlineSink or session
ansi_parse_cache: HashMap<String, (Vec<Vec<InlineSegment>>, Vec<String>)>,

// In convert_plain_lines():
if let Some((segments, plain)) = ansi_parse_cache.get(text) {
    return (segments.clone(), plain.clone());
}
// ... existing parse logic ...
ansi_parse_cache.insert(text.to_string(), (converted_lines.clone(), plain_lines.clone()));
```

**Cost**: ~256 KB cache for 1000 unique strings
**Benefit**: 10x+ faster for repeated ANSI codes (e.g., same color sequences)

**Optimization Strategy 2: Batch ANSI Parsing**
```rust
// Instead of parsing each chunk individually:
pub fn convert_plain_lines_batch(
    &self,
    texts: &[&str],
    fallback: &InlineTextStyle,
) -> Vec<(Vec<Vec<InlineSegment>>, Vec<String>)> {
    // Parse all at once, reuse state
    texts.iter().map(|text| self.convert_plain_lines(text, fallback)).collect()
}
```

**Benefit**: Reduced overhead for tool output with multiple lines.

---

### Bottleneck 3: Line Cloning in Scroll Rendering

**Problem**: Each scroll event may require cloning large `Vec<Line>` structures.

**Current Mitigation**:
- `visible_lines_cache` with Arc sharing
- Zero-copy on cache hits

**Further Optimization: Cow for Line References**
```rust
// Consider using Cow<'a, Line<'a>> or Rc<Line> for scroll ranges
pub fn get_visible_range(&self, start_row: usize, max_rows: usize) -> Vec<Rc<Line<'static>>> {
    // Return references instead of clones when possible
    // Requires lifetime adjustments
}
```

**Caveat**: Requires careful lifetime management; Arc solution is simpler.

---

### Bottleneck 4: ANSI Code Boundary Issues During Scroll

**Problem**: ANSI codes may be split across viewport boundaries, causing color bleed.

**Current Solution**: Ensured by `docs/ANSI_ESCAPE_CODE_FIXES.md`
- ANSI reset codes placed correctly relative to newlines
- No orphaned color codes at line boundaries

**Verification Checklist**:
1. **Trailing newlines**: Ensure reset codes come before `\n`
2. **Viewport edges**: Test scrolling with colored output
3. **Multi-line colors**: Verify colors don't persist beyond intended range

**Test Pattern**:
```bash
# Terminal output with ANSI color codes
"\u{1b}[31mRed text\u{1b}[0m\nNext line should be normal\n"
```

---

## Implementation Roadmap

### Phase 1: Measurement (Baseline)
```rust
// Add performance tracing
#[instrument(skip(self))]
fn collect_transcript_window(&mut self, width: u16, start_row: usize, max_rows: usize) {
    // Existing code with timing
    let start = std::time::Instant::now();
    let result = /* reflow logic */;
    let elapsed = start.elapsed();
    warn!(elapsed_ms = elapsed.as_millis(), "transcript_reflow");
    result
}

#[instrument(skip_all)]
fn convert_plain_lines(&self, text: &str, fallback: &InlineTextStyle) {
    let start = std::time::Instant::now();
    let result = /* parse logic */;
    warn!(elapsed_us = start.elapsed().as_micros(), "ansi_parse");
    result
}
```

### Phase 2: ANSI Parse Caching (High Impact)
1. Add LRU cache to `InlineSink`
2. Test with large tool outputs (10K+ lines)
3. Measure cache hit rate

### Phase 3: Incremental Reflow (Medium Impact)
1. Implement dirty message tracking
2. Test with large transcript size changes
3. Verify correctness with random transcript modifications

### Phase 4: Viewport Optimization (Low Impact)
1. Profile current viewport caching hit rate
2. Consider batch line allocation for large viewports
3. Test on low-end hardware (Raspberry Pi, older terminals)

---

## Testing Strategy

### Unit Tests
```rust
#[test]
fn test_ansi_parse_cache_hit() {
    // Verify cache returns identical results
}

#[test]
fn test_scroll_with_ansi_content() {
    // Test color codes don't bleed across viewport boundaries
}

#[test]
fn test_large_transcript_reflow() {
    // Benchmark reflow time for 100K+ lines
}
```

### Integration Tests
```bash
# Test scroll performance with actual PTY output
./run.sh
# Type commands that generate colored output
# Scroll rapidly and check frame rate
```

### Benchmarks
```rust
// In benches/transcript_perf.rs
criterion::black_box(
    cache.get_visible_range(0, 1000)
);
```

---

## Configuration Constants

Add to `vtcode-core/src/config/constants.rs`:

```rust
/// Maximum size of ANSI parse result cache (bytes)
pub const ANSI_PARSE_CACHE_LIMIT: usize = 256 * 1024; // 256 KB

/// Dirty message threshold before full reflow
pub const REFLOW_DIRTY_THRESHOLD: usize = 10;

/// Viewport cache validity window (lines)
pub const VIEWPORT_CACHE_WINDOW: usize = 500;
```

---

## Monitor & Logging

### Metrics to Track
1. **Scroll latency**: Time from scroll event to frame render
2. **ANSI parse time**: Per-line parsing cost
3. **Cache hit rate**: visible_lines_cache effectiveness
4. **Memory usage**: Transcript cache size growth

### Log Points
```rust
// In session.rs scroll handler
info!(
    scroll_offset = %self.scroll_manager.offset(),
    cache_hit = visible_lines_cache.is_some(),
    duration_ms = elapsed.as_millis(),
    "scroll_render_complete"
);

// In ansi.rs ANSI parsing
debug!(
    text_len = text.len(),
    cache_hit = ansi_parse_cache.contains_key(text),
    duration_us = elapsed.as_micros(),
    "ansi_parse"
);
```

---

## Compatibility Notes

### Terminal Support
- **256-color terminals**: Full ANSI color support
- **Truecolor (24-bit RGB)**: Handled by `anstyle` library
- **No-color mode**: Graceful fallback to plain text

### Platforms
- **macOS/Linux**: Full ANSI support
- **Windows Terminal**: Modern ANSI support via VT100 mode
- **Legacy terminals**: Graceful degradation (strip ANSI codes)

---

## References

- **ANSI Escape Codes**: `docs/ANSI_ESCAPE_CODE_FIXES.md`
- **Ratatui Documentation**: Line wrapping and styling
- **ansi-to-tui crate**: ANSI → Ratatui conversion
- **anstyle crate**: ANSI color definitions

---

## Future Enhancements

### Short Term (1-2 sprints)
- [ ] Implement ANSI parse result caching
- [ ] Add performance metrics/logging
- [ ] Profile scroll latency under load

### Medium Term (2-4 sprints)
- [ ] Incremental message reflow
- [ ] Batch ANSI parsing API
- [ ] Viewport prefetching (cache lines above/below visible area)

### Long Term (Future)
- [ ] GPU-accelerated rendering (for very large transcripts)
- [ ] Persistent transcript storage with lazy loading
- [ ] Streaming ANSI parser (process codes incrementally)
