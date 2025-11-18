# Quick Start: Scroll & ANSI Rendering Optimization

## Priority 1: ANSI Parse Caching (High ROI)

### Problem
Every tool output line goes through ANSI parsing via `ansi-to-tui`. Repeated ANSI codes (same colors, same format) are re-parsed every render.

### Solution: Add LRU Cache to InlineSink

#### Step 1: Update `vtcode-core/src/utils/ansi.rs`

```rust
use lru::LruCache;
use std::num::NonZeroUsize;

struct InlineSink {
    handle: InlineHandle,
    // Add this field:
    ansi_parse_cache: LruCache<String, (Vec<Vec<InlineSegment>>, Vec<String>)>,
}

impl InlineSink {
    fn new(handle: InlineHandle) -> Self {
        Self { 
            handle,
            ansi_parse_cache: LruCache::new(NonZeroUsize::new(512).unwrap()), // 512 entries
        }
    }

    fn convert_plain_lines(
        &mut self,  // Changed to &mut to allow cache mutation
        text: &str,
        fallback: &InlineTextStyle,
    ) -> (Vec<Vec<InlineSegment>>, Vec<String>) {
        // Check cache first
        if let Some(cached) = self.ansi_parse_cache.get(text) {
            return cached.clone();
        }

        // Existing parsing logic (unchanged)
        if text.is_empty() {
            let result = (vec![Vec::new()], vec![String::new()]);
            self.ansi_parse_cache.put(text.to_string(), result.clone());
            return result;
        }

        let had_trailing_newline = text.ends_with('\n');

        if let Ok(parsed) = text.as_bytes().into_text() {
            let mut converted_lines = Vec::with_capacity(parsed.lines.len().max(1));
            let mut plain_lines = Vec::with_capacity(parsed.lines.len().max(1));
            let base_style = RatatuiStyle::default().patch(parsed.style);

            for line in &parsed.lines {
                let mut segments = Vec::new();
                let mut plain_line = String::new();
                let line_style = base_style.patch(line.style);

                for span in &line.spans {
                    let content = span.content.clone().into_owned();
                    if content.is_empty() {
                        continue;
                    }

                    let span_style = line_style.patch(span.style);
                    let inline_style = self.inline_style_from_ratatui(span_style, fallback);
                    plain_line.push_str(&content);
                    segments.push(InlineSegment {
                        text: content,
                        style: inline_style,
                    });
                }

                converted_lines.push(segments);
                plain_lines.push(plain_line);
            }

            let needs_placeholder_line = if converted_lines.is_empty() {
                true
            } else {
                had_trailing_newline && plain_lines.last().is_none_or(|line| !line.is_empty())
            };
            if needs_placeholder_line {
                converted_lines.push(Vec::new());
                plain_lines.push(String::new());
            }

            let result = (converted_lines, plain_lines);
            self.ansi_parse_cache.put(text.to_string(), result.clone());
            return result;
        }

        // Fallback: Plain text (same as before)
        let mut converted_lines = Vec::new();
        let mut plain_lines = Vec::new();

        for line in text.split('\n') {
            let mut segments = Vec::new();
            if !line.is_empty() {
                segments.push(InlineSegment {
                    text: line.to_string(),
                    style: fallback.clone(),
                });
            }
            converted_lines.push(segments);
            plain_lines.push(line.to_string());
        }

        if had_trailing_newline {
            converted_lines.push(Vec::new());
            plain_lines.push(String::new());
        }

        if converted_lines.is_empty() {
            converted_lines.push(Vec::new());
            plain_lines.push(String::new());
        }

        let result = (converted_lines, plain_lines);
        self.ansi_parse_cache.put(text.to_string(), result.clone());
        result
    }
}
```

#### Step 2: Update Call Sites

Update methods that call `convert_plain_lines()` to use `&mut self`:

```rust
fn write_multiline(
    &mut self,  // Already &mut, good
    style: Style,
    indent: &str,
    text: &str,
    kind: InlineMessageKind,
) -> Result<()> {
    // ... existing code ...
    let fallback = self.resolve_fallback_style(style);
    let (converted_lines, plain_lines) = self.convert_plain_lines(text, &fallback);
    // ... rest unchanged ...
}

fn write_inline(&mut self, style: Style, text: &str, kind: InlineMessageKind) {
    // Already &mut
    let fallback = self.resolve_fallback_style(style);
    let (converted_lines, _) = self.convert_plain_lines(text, &fallback);
    // ... rest unchanged ...
}
```

#### Step 3: Add Dependency

Update `vtcode-core/Cargo.toml`:

```toml
[dependencies]
lru = "0.12"
```

### Expected Impact
- **Cache hit rate**: 40-70% for typical tool outputs (tests/compiles/etc.)
- **Performance**: 5-10x faster for cached entries
- **Memory**: ~512 KB for 512 entries

---

## Priority 2: Scroll Performance Monitoring

### Problem
No visibility into scroll render latency. Hard to identify bottlenecks.

### Solution: Add Performance Instrumentation

#### Step 1: Update `vtcode-core/src/ui/tui/session.rs`

```rust
use std::time::Instant;

impl TranscriptSession {
    fn collect_transcript_window_cached(
        &mut self,
        width: u16,
        start_row: usize,
        max_rows: usize,
    ) -> Vec<Line<'static>> {
        let timer_start = Instant::now();

        // Check if we have cached visible lines for this exact position and width
        if let Some((cached_offset, cached_width, cached_lines)) = &self.visible_lines_cache {
            if *cached_offset == start_row && *cached_width == width {
                let elapsed = timer_start.elapsed();
                if elapsed.as_micros() > 100 {
                    tracing::debug!(
                        elapsed_us = elapsed.as_micros(),
                        cache_hit = true,
                        "scroll_visible_lines_cache"
                    );
                }
                return (**cached_lines).clone();
            }
        }

        // Not in cache, fetch from transcript
        let visible_lines = self.collect_transcript_window(width, start_row, max_rows);
        let elapsed = timer_start.elapsed();

        if elapsed.as_millis() > 10 {
            tracing::warn!(
                elapsed_ms = elapsed.as_millis(),
                cache_hit = false,
                lines = visible_lines.len(),
                "slow_scroll_visible_lines"
            );
        }

        // Cache for next render if scroll position unchanged
        self.visible_lines_cache = Some((start_row, width, Arc::new(visible_lines.clone())));

        visible_lines
    }

    #[tracing::instrument(skip(self))]
    fn collect_transcript_window(
        &mut self,
        width: u16,
        start_row: usize,
        max_rows: usize,
    ) -> Vec<Line<'static>> {
        let timer_start = Instant::now();
        
        // ... existing code ...
        
        // Log if reflow was slow
        let elapsed = timer_start.elapsed();
        if elapsed.as_millis() > 50 {
            tracing::warn!(
                elapsed_ms = elapsed.as_millis(),
                width = width,
                start_row = start_row,
                max_rows = max_rows,
                "slow_transcript_reflow"
            );
        }
        
        lines
    }
}
```

#### Step 2: Monitor in Tests

```rust
#[tokio::test]
async fn test_scroll_performance_large_transcript() {
    let mut session = TranscriptSession::new();
    
    // Create large transcript (1000 messages)
    for i in 0..1000 {
        session.add_message(Message {
            content: format!("Line {} with some content\n", i),
            ..Default::default()
        });
    }
    
    let timer = Instant::now();
    let _ = session.collect_transcript_window_cached(80, 0, 20);
    assert!(timer.elapsed().as_millis() < 100, "First render should be < 100ms");
    
    let timer = Instant::now();
    let _ = session.collect_transcript_window_cached(80, 0, 20);
    assert!(timer.elapsed().as_micros() < 1000, "Cached render should be < 1ms");
    
    let timer = Instant::now();
    let _ = session.collect_transcript_window_cached(80, 100, 20);
    assert!(timer.elapsed().as_millis() < 50, "Scroll to offset should be < 50ms");
}
```

---

## Priority 3: Verify ANSI Code Boundaries

### Problem
ANSI color codes can bleed across scroll boundaries.

### Solution: Test Pattern

Create `tests/ansi_scroll_safety.rs`:

```rust
#[test]
fn test_ansi_codes_at_viewport_boundaries() {
    use vtcode_core::ui::tui::session::transcript::TranscriptReflowCache;
    use ratatui::text::{Line, Span};
    use ratatui::style::{Color, Style};

    let mut cache = TranscriptReflowCache::new(80);

    // Create messages with ANSI color sequences
    let red_style = Style::default().fg(Color::Red);
    let normal_style = Style::default();

    // Message 1: Red text ending without reset
    let msg1 = vec![
        Line::from(vec![
            Span::styled("Red line 1", red_style),
        ]),
        Line::from(vec![
            Span::styled("Red line 2", red_style),
        ]),
    ];

    // Message 2: Should NOT inherit red from previous message
    let msg2 = vec![
        Line::from(vec![
            Span::raw("Normal line 1"),
        ]),
    ];

    cache.update_message(0, 1, msg1);
    cache.update_message(1, 2, msg2);
    cache.update_row_offsets();

    // Get viewport that spans message boundary
    let visible = cache.get_visible_range(1, 2);

    // Verify: second message should not have red style
    assert_eq!(visible.len(), 2);
    assert_eq!(visible[1].spans[0].style.fg, Some(Color::default()));
}

#[test]
fn test_ansi_reset_codes_preserved_across_newlines() {
    let mut cache = TranscriptReflowCache::new(80);

    // ANSI text with explicit reset before newline
    let msg = vec![
        Line::from("\u{1b}[31mRed text\u{1b}[0m\nNext line"),
    ];

    cache.update_message(0, 1, msg);
    cache.update_row_offsets();

    let visible = cache.get_visible_range(0, 1);
    
    // Verify reset code is present and valid
    assert!(visible[0].to_string().contains("\u{1b}[0m"));
}
```

Run with:
```bash
cargo test test_ansi_codes_at_viewport_boundaries -- --nocapture
cargo test test_ansi_reset_codes_preserved_across_newlines -- --nocapture
```

---

## Priority 4: Monitor Cache Effectiveness

### Problem
Don't know if visible_lines_cache is actually helping.

### Solution: Add Cache Stats

```rust
// In session.rs
#[derive(Default, Debug)]
struct CacheStats {
    total_renders: u64,
    cache_hits: u64,
    cache_misses: u64,
}

impl TranscriptSession {
    cache_stats: CacheStats,

    fn collect_transcript_window_cached(
        &mut self,
        width: u16,
        start_row: usize,
        max_rows: usize,
    ) -> Vec<Line<'static>> {
        self.cache_stats.total_renders += 1;

        if let Some((cached_offset, cached_width, cached_lines)) = &self.visible_lines_cache {
            if *cached_offset == start_row && *cached_width == width {
                self.cache_stats.cache_hits += 1;
                return (**cached_lines).clone();
            }
        }

        self.cache_stats.cache_misses += 1;
        let visible_lines = self.collect_transcript_window(width, start_row, max_rows);
        self.visible_lines_cache = Some((start_row, width, Arc::new(visible_lines.clone())));

        visible_lines
    }

    fn cache_hit_rate(&self) -> f64 {
        if self.cache_stats.total_renders == 0 {
            0.0
        } else {
            self.cache_stats.cache_hits as f64 / self.cache_stats.total_renders as f64
        }
    }
}

// Debug output in UI (e.g., status bar)
format!("Cache Hit: {:.1}%", session.cache_hit_rate() * 100.0)
```

---

## Testing Checklist

- [ ] ANSI parse cache integration test
- [ ] Scroll performance benchmark (target < 50ms per scroll)
- [ ] ANSI boundary test (no color bleed)
- [ ] Cache hit rate > 50% under normal usage
- [ ] Memory footprint increase < 5 MB
- [ ] Compatibility with 256-color and truecolor terminals

---

## Deployment Notes

### Backward Compatibility
- All changes are internal optimization, no API changes
- ANSI handling logic unchanged, only cached
- Safe to enable immediately

### Configuration
Consider adding to `vtcode.toml`:
```toml
[tui.performance]
# Enable ANSI parse result caching
ansi_cache_enabled = true
# Maximum cache entries
ansi_cache_size = 512
# Enable scroll performance logging
scroll_perf_logging = false
```

### Rollback Plan
If issues arise, simply remove LRU cache and revert to original `convert_plain_lines()` logic.
