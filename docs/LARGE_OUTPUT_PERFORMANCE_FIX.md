# Large Output Performance Fix

## Problem Statement

**Issue #1397**: Agent run loop hangs/becomes unresponsive with extremely large command output (e.g., `git diff` on long sessions).

### Root Causes

1. **Unbounded PTY scrollback accumulation** - No limit on memory growth
2. **Synchronous rendering** - UI blocks while processing massive outputs  
3. **No progressive display** - User sees nothing until entire output is processed
4. **Inefficient line-by-line iteration** - O(n) for each render pass
5. **No early truncation** - All output processed even when only tail matters

### Symptoms

- Program hangs/freezes during large `git diff` or `git log` commands
- UI becomes laggy and unresponsive
- High CPU usage during output processing
- Long delay before showing any output

---

## Solution Overview

### Phase 1: PTY Output Limiting (Immediate)
- Cap PTY scrollback to configurable maximum (default: 100K lines)
- Add overflow detection and warningswith spool-to-disk
- Implement circular buffer for efficient memory usage

### Phase 2: Progressive Rendering (Performance)
- Stream output in chunks instead of waiting for completion
- Show progress indicator for large outputs
- Update UI incrementally (every 100ms max)

### Phase 3: Smart Truncation (UX)
- Detect large output early (first 1000 lines)
- Auto-spool to disk when threshold exceeded
- Show head + tail preview with file location

### Phase 4: TUI Optimization (Rendering)
- Lazy line wrapping (only wrap visible lines)
- Virtual scrolling for large transcripts
- Batch render updates (debounce)

---

## Implementation Plan

### 1. Add PTY Scrollback Limits

**File**: `vtcode-core/src/tools/pty.rs`

```rust
// Add to PtyConfig
pub struct PtyConfig {
    pub scrollback_lines: usize,           // Existing
    pub max_scrollback_bytes: usize,       // NEW: prevent memory explosion
    pub output_chunk_lines: usize,         // NEW: for progressive display
    pub large_output_threshold_kb: usize,  // NEW: auto-spool threshold
}

impl Default for PtyConfig {
    fn default() -> Self {
        Self {
            scrollback_lines: 10_000,
            max_scrollback_bytes: 50_000_000,  // 50MB max
            output_chunk_lines: 500,            // Show 500 lines at a time
            large_output_threshold_kb: 5_000,   // Spool after 5MB
        }
    }
}
```

**Modify `PtyScrollback`**:

```rust
struct PtyScrollback {
    lines: VecDeque<String>,
    pending_lines: VecDeque<String>,
    partial: String,
    pending_partial: String,
    capacity_lines: usize,
    max_bytes: usize,           // NEW
    current_bytes: usize,       // NEW
    overflow_detected: bool,    // NEW
}

impl PtyScrollback {
    fn new(capacity_lines: usize, max_bytes: usize) -> Self {
        Self {
            lines: VecDeque::new(),
            pending_lines: VecDeque::new(),
            partial: String::new(),
            pending_partial: String::new(),
            capacity_lines: capacity_lines.max(1),
            max_bytes,
            current_bytes: 0,
            overflow_detected: false,
        }
    }

    fn push_text(&mut self, text: &str) {
        // Check byte limit BEFORE processing
        let text_bytes = text.len();
        if self.current_bytes + text_bytes > self.max_bytes {
            if !self.overflow_detected {
                self.overflow_detected = true;
                let warning = format!(
                    "\n\n[WARNING: Output size limit exceeded ({} MB). Further output truncated.]\n",
                    self.max_bytes / 1_000_000
                );
                self.lines.push_back(warning.clone());
                self.pending_lines.push_back(warning);
            }
            return; // DROP further output
        }

        for part in text.split_inclusive('\n') {
            self.partial.push_str(part);
            self.pending_partial.push_str(part);
            if part.ends_with('\n') {
                let complete = std::mem::take(&mut self.partial);
                let _ = std::mem::take(&mut self.pending_partial);
                
                self.current_bytes += complete.len();
                self.lines.push_back(complete.clone());
                self.pending_lines.push_back(complete);
                
                // Circular buffer: drop oldest
                while self.lines.len() > self.capacity_lines {
                    if let Some(oldest) = self.lines.pop_front() {
                        self.current_bytes = self.current_bytes.saturating_sub(oldest.len());
                    }
                }
                while self.pending_lines.len() > self.capacity_lines {
                    self.pending_lines.pop_front();
                }
            }
        }
    }

    fn has_overflow(&self) -> bool {
        self.overflow_detected
    }
}
```

### 2. Progressive Output Display

**File**: `src/agent/runloop/tool_output/commands.rs`

Add chunked rendering for large outputs:

```rust
pub(crate) async fn render_terminal_command_panel(
    renderer: &mut AnsiRenderer,
    payload: &Value,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
    vt_config: Option<&VTCodeConfig>,
    allow_ansi: bool,
    token_budget: Option<&TokenBudgetManager>,
) -> Result<()> {
    // ... existing code ...

    // NEW: Detect large output early
    let output_size = stdout.len() + stderr.len();
    let size_threshold = vt_config
        .and_then(|cfg| cfg.pty.large_output_threshold_kb)
        .unwrap_or(5_000) * 1024;

    if output_size > size_threshold {
        // Auto-spool large outputs
        return render_large_output_with_spool(
            renderer,
            &stdout,
            &stderr,
            &command,
            vt_config,
        ).await;
    }

    // ... existing rendering logic ...
}

async fn render_large_output_with_spool(
    renderer: &mut AnsiRenderer,
    stdout: &str,
    stderr: &str,
    command: &str,
    vt_config: Option<&VTCodeConfig>,
) -> Result<()> {
    use std::io::Write;
    
    // Spool to disk
    let spool_dir = PathBuf::from(".vtcode/large-output");
    std::fs::create_dir_all(&spool_dir)?;
    
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let filename = format!("cmd-{}.log", timestamp);
    let log_path = spool_dir.join(&filename);
    
    let mut file = std::fs::File::create(&log_path)?;
    file.write_all(stdout.as_bytes())?;
    if !stderr.is_empty() {
        file.write_all(b"\n\n=== STDERR ===\n")?;
        file.write_all(stderr.as_bytes())?;
    }
    file.flush()?;

    // Show preview: first 100 + last 100 lines
    let stdout_lines: Vec<&str> = stdout.lines().collect();
    let total_lines = stdout_lines.len();
    
    renderer.line(
        MessageStyle::Warning,
        &format!(
            "⚠️  Large output detected ({} lines, {} KB)",
            total_lines,
            (stdout.len() + stderr.len()) / 1024
        ),
    )?;
    renderer.line(
        MessageStyle::Info,
        &format!("📁 Full output saved to: {}", log_path.display()),
    )?;
    renderer.line(MessageStyle::Info, "")?;
    renderer.line(MessageStyle::Info, "Preview (first 100 + last 100 lines):")?;
    renderer.line(MessageStyle::Info, "─".repeat(60))?;

    // Head
    for line in stdout_lines.iter().take(100) {
        renderer.line(MessageStyle::Response, line)?;
    }

    if total_lines > 200 {
        renderer.line(
            MessageStyle::Info,
            &format!("\n[... {} lines omitted ...]\n", total_lines - 200),
        )?;
    }

    // Tail
    for line in stdout_lines.iter().skip(total_lines.saturating_sub(100)) {
        renderer.line(MessageStyle::Response, line)?;
    }

    renderer.line(MessageStyle::Info, "─".repeat(60))?;
    renderer.line(
        MessageStyle::Info,
        &format!("💡 View full output: cat {}", log_path.display()),
    )?;

    Ok(())
}
```

### 3. Update PTY Manager Initialization

**File**: `vtcode-core/src/tools/pty.rs`

```rust
// In create_session()
let scrollback = Arc::new(Mutex::new(PtyScrollback::new(
    self.config.scrollback_lines,
    self.config.max_scrollback_bytes,  // NEW parameter
)));

// In run_command() - same update
```

### 4. Add Configuration Keys

**File**: `vtcode.toml.example`

```toml
[pty]
scrollback_lines = 10000           # Maximum lines to keep in memory
max_scrollback_bytes = 50000000    # Maximum 50MB of output per session
output_chunk_lines = 500           # Lines to display per chunk
large_output_threshold_kb = 5000   # Auto-spool outputs > 5MB
```

---

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrollback_enforces_byte_limit() {
        let mut scrollback = PtyScrollback::new(1000, 1024); // 1KB limit
        
        // Fill with 2KB of data
        for _ in 0..100 {
            scrollback.push_text("012345678901234567890\n"); // 22 bytes each
        }
        
        assert!(scrollback.has_overflow());
        assert!(scrollback.current_bytes <= 1024);
    }

    #[test]
    fn scrollback_circular_buffer_drops_oldest() {
        let mut scrollback = PtyScrollback::new(3, 10_000);
        
        scrollback.push_text("line1\n");
        scrollback.push_text("line2\n");
        scrollback.push_text("line3\n");
        scrollback.push_text("line4\n"); // Should drop line1
        
        let snapshot = scrollback.snapshot();
        assert!(!snapshot.contains("line1"));
        assert!(snapshot.contains("line4"));
    }
}
```

### Integration Tests

```bash
# Test 1: Large git diff
cargo run -- exec "git diff HEAD~100"

# Test 2: Huge log output
cargo run -- exec "git log --all --oneline"

# Test 3: Large file cat
cargo run -- exec "cat CHANGELOG.md"

# Test 4: Long-running command
cargo run -- exec "find / -name '*.rs' 2>/dev/null"
```

---

## Performance Targets

| Metric | Before | After | Improvement |
|--------|--------|-------|-------------|
| Max output size | Unlimited | 50MB | ∞ → 50MB |
| Memory growth | O(n) | O(1) capped | Bounded |
| First render | Blocks | \u003c100ms | Immediate |
| Large diff (10MB) | Hangs | \u003c1s | 100x faster |
| UI responsiveness | Frozen | Smooth | Always interactive |

---

## Rollout Plan

### Phase 1: Immediate Fix (This PR)
- [x] Add byte limit to PTY scrollback
- [x] Implement circular buffer
- [x] Add overflow detection

### Phase 2: Enhanced UX (Follow-up)
- [ ] Progressive rendering with chunks
- [ ] Auto-spool for large outputs
- [ ] Preview head+tail display

### Phase 3: Future Optimizations
- [ ] Streaming output to UI (live updates)
- [ ] Virtual scrolling for transcript
- [ ] Background spooling during command execution

---

## Backward Compatibility

✅ **Fully backward compatible**
- New config keys have sensible defaults
- Existing `scrollback_lines` behavior preserved
- No breaking API changes

---

## Related Issues

- #1397 - Agent hangs on large output
- Related to: `docs/TERMINAL_OUTPUT_OPTIMIZATION.md`
- Related to: `docs/scroll-optimization/`

---

## Author Notes

This fix targets the immediate problem (hangs/unresponsiveness) by:
1. **Preventing memory explosion** with byte caps
2. **Guaranteeing bounded processing** with circular buffers
3. **Early detection** of problematic outputs

Future PRs will add progressive rendering and better UX for large outputs.
