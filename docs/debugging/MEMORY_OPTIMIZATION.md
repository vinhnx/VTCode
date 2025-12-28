# Memory Optimization & Debugging Guide for VT Code

This guide provides structured approaches to diagnose and resolve high memory consumption in VT Code, a Rust-based terminal coding agent.

## 1. Initial Diagnosis Steps

### 1.1 Profiling Tools for Rust

#### **Linux/macOS - Valgrind & Massif**
```bash
# Install Valgrind (macOS requires Homebrew)
brew install valgrind

# Run with memory profiler
valgrind --tool=massif --massif-out-file=massif.out cargo run -- ask "test query"

# Visualize results
ms_print massif.out | head -100
```

#### **macOS - Instruments (Xcode)**
```bash
# Profile with Xcode's built-in memory instrument
cargo build --release
xcrun xctrace record --template "System Trace" \
  --output trace.xctrace ./target/release/vtcode ask "test"

# Open in Xcode
open trace.xctrace
```

#### **Cross-Platform - `heaptrack` (Linux preferred)**
```bash
# Install heaptrack
sudo apt-get install heaptrack

# Run profiling
heaptrack cargo run -- ask "test"

# Visualize GUI
heaptrack_gui heaptrack.vtcode.*
```

#### **Built-in Rust Profiling with `perf` (Linux)**
```bash
# Requires Linux
cargo build --release
perf record -F 99 -g ./target/release/vtcode ask "test query"
perf report

# Flame graph generation
cargo install flamegraph
cargo flamegraph --release -- ask "test query"
```

### 1.2 Memory Profiling with Rust Allocator Instrumentation

Add to `Cargo.toml` for memory tracking:

```toml
[profile.dev]
debug = true  # Keep symbols for profiling

[dependencies]
# Optional: Fine-grained memory tracking
dhat = "0.3"  # DHAT-based profiling (lighter than Valgrind)
```

Enable DHAT profiling:

```rust
#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Allocator = dhat::Allocator;

fn main() {
    #[cfg(feature = "dhat-heap")]
    let _guard = dhat::Profiler::new_heap();
    
    // Run your code
}
```

Build and run:
```bash
cargo build --release --features dhat-heap
./target/release/vtcode ask "your query" 2>&1 | grep "DHAT"
```

### 1.3 Log Analysis & Metrics

Enable verbose logging to track memory-related events:

```bash
# Enable trace-level logging
RUST_LOG=vtcode_core=trace,vtcode_core::cache=debug cargo run -- ask "test"

# Filter for cache operations
RUST_LOG=vtcode_core::cache=debug cargo run -- ask "test" 2>&1 | grep -E "cache|evict|alloc"
```

**Key metrics to monitor:**
- Cache evictions (LRU entry removals)
- Large buffer allocations (>10MB)
- PTY output buffer sizes
- Number of concurrent Arc/Mutex allocations

### 1.4 Memory Usage Snapshots

```bash
# Monitor resident set size (RSS) in real-time
while true; do
  ps -o pid,rss,vsz,comm= -p $(pgrep -f "vtcode") | awk '{print $2 " KB"}'
  sleep 2
done

# Or use /proc/self/status (Linux)
cargo run -- ask "test" & \
  PID=$!; \
  sleep 1; \
  grep -E "VmRSS|VmPeak" /proc/$PID/status
```

---

## 2. Potential Causes & Analysis

### 2.1 Cache Growth & Memory Leaks

**Location**: `vtcode-core/src/cache/`, `vtcode-core/src/tools/tree_sitter/parse_cache.rs`

**Risk Areas:**
- LRU caches unbounded growth when eviction policy fails
- Parse tree cache (`ParseCache`) for tree-sitter ASTs
- MCP tool discovery cache (`tool_discovery_cache.rs`)
- Grep/search results cache (`grep_cache.rs`)

**Diagnosis:**
```bash
# Check cache configuration defaults
grep -n "MAX_CAPACITY\|lru\|LRU" vtcode-core/src/cache/mod.rs

# Review eviction policies
grep -n "evict\|remove_oldest" vtcode-core/src/cache/mod.rs
```

**Example: Parse Cache Unbounded Growth**
```rust
// vtcode-core/src/tools/tree_sitter/parse_cache.rs:38-60
pub fn new(capacity: usize) -> Self {
    Self {
        cache: LruCache::new(NonZeroUsize::new(capacity).unwrap()),
    }
}

// ISSUE: If capacity is misconfigured or never evicted, memory grows linearly
// RISK: Parsing large files repeatedly without cache hits
```

### 2.2 Streaming & Buffer Allocation Issues

**Location**: `vtcode-core/src/tools/pty.rs`, `vtcode-core/src/utils/ansi.rs`

**Risk Areas:**
- PTY output buffering: `Vec::with_capacity` allocations can be large
- ANSI parsing vectors retain all color spans
- Diff rendering pre-allocates large strings

**Example: PTY Buffer Issue**
```rust
// vtcode-core/src/tools/pty.rs:435-436
let mut output_buffer = String::with_capacity(LARGE_BUFFER_SIZE); // 1MB+ allocation

// ISSUE: Single long command can exhaust memory if buffer isn't drained
// RISK: Multiple concurrent PTY sessions accumulate buffers
```

**Diagnosis:**
```bash
# Find all Vec::with_capacity and String::with_capacity calls
grep -r "with_capacity" vtcode-core/src/tools/pty.rs | head -10

# Check for unbounded growth patterns
grep -A5 "with_capacity" vtcode-core/src/tools/pty.rs
```

### 2.3 Global State & Lazy Initialization

**Location**: `vtcode-core/src/utils/vtcodegitignore.rs`, `vtcode-core/src/ui/theme.rs`

**Risk Areas:**
- `once_cell::sync::Lazy` static initialization can accumulate state
- Global gitignore patterns loaded once (good) but never freed (memory held indefinitely)
- Theme data statically allocated

**Example: Global Gitignore**
```rust
// vtcode-core/src/utils/vtcodegitignore.rs:208
lazy_static::lazy_static! {
    pub static ref VTCODE_IGNORE: VtcodeIgnore = {
        VtcodeIgnore::new() // Loaded once, never freed
    };
}

// ISSUE: Large regex patterns + gitignore rules held in memory forever
// RISK: In long-running processes, cannot be reclaimed
```

### 2.4 Arc/Mutex & Cloning Overhead

**Location**: `vtcode-core/src/ui/tui/session/messages.rs`, `vtcode-core/src/ui/tui/session/transcript.rs`

**Risk Areas:**
- Message styling wrapped in `Arc<Style>` (cloned repeatedly)
- Transcript cache duplicates `Vec<Line<'static>>` per width
- Thread-safe caches use `Arc<RwLock<>>` with excessive cloning

**Example: Style Cloning**
```rust
// vtcode-core/src/ui/tui/session/messages.rs:177
style: std::sync::Arc::new(style.clone()), // Arc + clone = memory overhead

// ISSUE: Each message clones style into Arc; style data duplicated
// RISK: 1000s of messages = style data replicated 1000s of times
```

### 2.5 Recursive Functions & Stack Exhaustion

**Location**: `vtcode-core/src/tree_sitter/`, parser operations

**Risk Areas:**
- Tree-sitter traversal with deep ASTs
- Recursive diff computation for large files

**Diagnosis:**
```bash
# Search for recursive patterns
grep -r "fn.*->.*Self\|recurse\|recursive" vtcode-core/src/tree_sitter/

# Check for unbounded recursion
grep -n "depth\|max_depth\|recursion_limit" vtcode-core/src/
```

---

## 3. Detailed Fix Steps

### 3.1 Fix: Bounded Cache with Eviction Policy

**File**: `vtcode-core/src/cache/mod.rs`

**Current Issue**: Caches may not evict entries properly.

**Fix**:
```rust
// Before: Unbounded or misconfigured eviction
pub const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(300); // 5 min
pub const MAX_CACHE_ENTRIES: usize = 10_000; // Too large?

// After: Right-sized with aggressive eviction for memory-constrained envs
pub const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(120); // 2 min
pub const MAX_CACHE_ENTRIES: usize = 1000; // Reduced

// Add memory-aware eviction
pub fn get_cache_capacity() -> usize {
    let available_memory = sys_info::mem_info().ok()
        .map(|m| m.avail as usize)
        .unwrap_or(2 * 1024 * 1024 * 1024); // 2GB default
    
    // Use 5% of available memory for cache
    (available_memory / 20) / 64_000 // ~64KB per entry estimate
}
```

**Verification**:
```bash
# Test cache eviction under load
cargo test cache_eviction_policy --release -- --nocapture

# Monitor memory during long session
watch -n 1 'ps aux | grep vtcode | grep -v grep | awk "{print \$6}"'
```

### 3.2 Fix: PTY Buffer Streaming

**File**: `vtcode-core/src/tools/pty.rs`

**Current Issue**: Large buffers allocated per command.

**Fix**:
```rust
// Before: Single large allocation
let mut output_buffer = String::with_capacity(1024 * 1024); // 1MB

// After: Incremental streaming with bounded size
const MAX_PTY_BUFFER_SIZE: usize = 256 * 1024; // 256KB max

pub async fn read_pty_output(&mut self) -> Result<Option<String>> {
    let mut chunk = String::with_capacity(4096); // 4KB chunks
    
    loop {
        match self.pty.read(&mut chunk)? {
            Some(data) => {
                // Process and flush chunk immediately
                self.flush_output(&data)?;
                chunk.clear(); // Reuse buffer
            }
            None => break,
        }
        
        // Don't let buffer grow unbounded
        if chunk.len() > MAX_PTY_BUFFER_SIZE {
            self.flush_output(&chunk)?;
            chunk.clear();
        }
    }
    Ok(None)
}

fn flush_output(&mut self, data: &str) -> Result<()> {
    // Process data immediately instead of buffering
    self.process_output(data)?;
    Ok(())
}
```

**Verification**:
```bash
# Test with long-running command
cargo run -- execute "while true; do echo 'test'; sleep 0.1; done | head -10000"

# Monitor memory
ps aux | grep vtcode | awk '{print "Memory: " $6 " KB"}'
```

### 3.3 Fix: Transcript Cache Size Optimization

**File**: `vtcode-core/src/ui/tui/session/transcript.rs`

**Current Issue**: Width-specific cache duplicates data.

**Fix**:
```rust
// Before: Cache per width (unbounded if many widths)
pub width_specific_cache: HashMap<u16, Vec<Vec<Line<'static>>>>,

// After: Single-width cache with LRU eviction
pub struct TranscriptReflowCache {
    pub messages: Vec<CachedMessage>,
    pub cached_width: Option<u16>,
    pub cached_lines: Vec<Vec<Line<'static>>>,
    #[serde(skip)]
    max_cache_widths: usize, // Limit width variations cached
}

impl TranscriptReflowCache {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            cached_width: None,
            cached_lines: Vec::new(),
            max_cache_widths: 3, // Cache only last 3 widths
        }
    }
    
    pub fn get_or_reflow(&mut self, width: u16) -> Vec<Vec<Line<'static>>> {
        // If width matches cached, return without recomputation
        if self.cached_width == Some(width) {
            return self.cached_lines.clone();
        }
        
        // Rotate out old widths if too many cached
        if self.cached_lines.len() > self.max_cache_widths {
            self.cached_lines.clear(); // Evict
        }
        
        // Reflow and cache
        let reflowed = self.reflow_for_width(width);
        self.cached_width = Some(width);
        self.cached_lines = reflowed.clone();
        reflowed
    }
}
```

**Verification**:
```bash
# Test with repeated terminal resizing
cargo test transcript_reflow_cache_bounded --release -- --nocapture
```

### 3.4 Fix: Reduce Style Arc Cloning

**File**: `vtcode-core/src/ui/tui/session/messages.rs`

**Current Issue**: Style cloned into Arc repeatedly.

**Fix**:
```rust
// Before: Clone into Arc per message
style: std::sync::Arc::new(style.clone()),

// After: Use reference counting without clone
use std::sync::Arc;

pub struct MessageSpan {
    content: String,
    style: Arc<Style>, // Shared reference, not cloned
}

impl MessageSpan {
    pub fn new(content: String, style_ref: Arc<Style>) -> Self {
        Self {
            content,
            style: style_ref, // Just increment refcount
        }
    }
}

// Factory function to create shared styles once
pub fn create_message_styles() -> MessageStyles {
    MessageStyles {
        default: Arc::new(Style::default()),
        error: Arc::new(Style::new().fg(Color::Red)),
        success: Arc::new(Style::new().fg(Color::Green)),
        // ... reuse these Arcs across all messages
    }
}
```

**Verification**:
```bash
cargo test message_span_arc_sharing --release
```

### 3.5 Fix: Global State with Drop Implementation

**File**: `vtcode-core/src/utils/vtcodegitignore.rs`

**Current Issue**: Global gitignore data never freed.

**Fix**:
```rust
// Implement proper cleanup for long-running processes
pub struct VtcodeIgnore {
    patterns: Vec<IgnorePattern>,
}

impl Drop for VtcodeIgnore {
    fn drop(&mut self) {
        // Explicit cleanup for long-running sessions
        self.patterns.clear();
        eprintln!("Gitignore patterns unloaded");
    }
}

// Alternative: Implement cache invalidation
impl VtcodeIgnore {
    pub fn clear_cache(&mut self) {
        self.patterns.clear();
    }
    
    pub fn reload() -> Result<Self> {
        // Allow explicit reload for long-running processes
        Self::new()
    }
}
```

---

## 4. Performance Optimization Suggestions

### 4.1 Enable Link-Time Optimization (LTO)

**File**: `Cargo.toml`

**Current**: LTO already enabled in release profile (good!)

```toml
[profile.release]
lto = true        # ✓ Already enabled
codegen-units = 1 # ✓ Already optimal
```

**Further optimization for memory**:
```toml
[profile.release]
lto = "fat"           # More aggressive LTO
codegen-units = 1
strip = true          # Remove symbols
opt-level = 3
panic = "abort"       # Smaller panic paths

# Memory-optimized alternative
[profile.release-memory]
inherits = "release"
opt-level = "z"       # Optimize for size
lto = "thin"          # Balance between speed and size
```

### 4.2 Implement Streaming with Generators

**Location**: PTY output processing

**Before**:
```rust
// Accumulate all output before processing
let mut results = Vec::new();
for chunk in command_output {
    results.push(process(chunk));
}
```

**After**:
```rust
// Stream and process incrementally
async fn process_pty_stream(
    pty: &mut PtySession,
) -> impl futures::Stream<Item = Result<ProcessedOutput>> {
    futures::stream::iter(Vec::new()).then(|_| async move {
        match pty.read().await {
            Some(chunk) => Ok(process_chunk(&chunk)),
            None => Err(anyhow::anyhow!("PTY closed")),
        }
    })
}

// Usage: Stream results without holding entire output
let mut stream = process_pty_stream(&mut pty);
while let Some(result) = stream.next().await {
    render_output(result?); // Render immediately
}
```

### 4.3 Implement Garbage Collection Tuning

**Rust doesn't have GC**, but you can optimize allocator behavior:

```bash
# Enable jemalloc for better memory fragmentation handling
# Add to Cargo.toml:
[dependencies]
jemallocator = "0.5"

# In main.rs:
#[global_allocator]
static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;
```

**Environment tuning**:
```bash
# Reduce memory fragmentation
MALLOC_CONF="dirty_decay_ms:0,muzzy_decay_ms:0" cargo run -- ask "test"

# Or with system allocator
MALLOC_TRIM_THRESHOLD_="256000" cargo run -- ask "test"
```

### 4.4 Implement Caching Strategies

**Metrics-driven caching**:

```rust
pub struct CacheMetrics {
    hits: u64,
    misses: u64,
    evictions: u64,
}

impl CacheMetrics {
    pub fn hit_rate(&self) -> f64 {
        self.hits as f64 / (self.hits + self.misses) as f64
    }
    
    pub fn should_increase_capacity(&self) -> bool {
        self.hit_rate() < 0.7 // If <70% hit rate, expand
    }
    
    pub fn should_decrease_ttl(&self) -> bool {
        self.evictions as f64 / self.hits as f64 > 0.3 // If >30% eviction rate, reduce TTL
    }
}

// Auto-tune based on metrics
pub fn adjust_cache_policy(metrics: &CacheMetrics) {
    if metrics.should_increase_capacity() {
        eprintln!("Low cache hit rate ({}%); increasing capacity",
                  (metrics.hit_rate() * 100) as u32);
    }
    if metrics.should_decrease_ttl() {
        eprintln!("High eviction rate; reducing TTL");
    }
}
```

### 4.5 Monitor Memory with Metrics Export

```rust
// Add to dependencies
[dependencies]
prometheus = "0.13"

// Instrument key operations
lazy_static::lazy_static! {
    pub static ref MEMORY_GAUGE: prometheus::Gauge =
        prometheus::Gauge::new("vtcode_memory_bytes", "Current memory usage")
            .expect("Failed to create gauge");
    
    pub static ref CACHE_SIZE_GAUGE: prometheus::IntGauge =
        prometheus::IntGauge::new("cache_entries", "Number of entries in cache")
            .expect("Failed to create gauge");
}

// Update metrics periodically
async fn record_metrics() {
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        
        if let Ok(status) = procfs::process::Process::myself() {
            if let Ok(stat) = status.stat() {
                MEMORY_GAUGE.set(stat.rss_bytes() as i64);
            }
        }
    }
}
```

---

## 5. Testing & Verification

### 5.1 Memory Regression Tests

```rust
#[test]
#[ignore] // Run with: cargo test -- --ignored
fn test_memory_usage_bounded() {
    let initial = memory_usage();
    
    // Simulate long session
    for _ in 0..1000 {
        let cache = ParseCache::new(100);
        // Cache operations...
    }
    
    let final_mem = memory_usage();
    let growth = (final_mem - initial) as f64 / initial as f64;
    
    assert!(growth < 0.1, "Memory grew by {}%", growth * 100.0);
}

#[cfg(unix)]
fn memory_usage() -> u64 {
    use std::fs;
    let status = fs::read_to_string("/proc/self/status").unwrap();
    for line in status.lines() {
        if line.starts_with("VmRSS:") {
            let kb: u64 = line.split_whitespace().nth(1).unwrap().parse().unwrap();
            return kb * 1024; // Convert to bytes
        }
    }
    0
}
```

### 5.2 Benchmark Long-Running Sessions

```bash
# Create benchmark command
cat > bench_long_session.sh << 'EOF'
#!/bin/bash
for i in {1..100}; do
    echo "Running iteration $i..."
    cargo run -- ask "analyze code in /tmp/sample_$i.rs" 2>&1 | grep -E "Memory|Cache"
    sleep 2
done
EOF

chmod +x bench_long_session.sh
./bench_long_session.sh
```

### 5.3 Profiling Production Builds

```bash
# Enable profiling feature
cargo build --release --features profiling

# Run with tracing
RUST_LOG=vtcode_core::cache=debug \
  RUST_BACKTRACE=1 \
  cargo run --release -- ask "test query" 2>&1 | tee memory.log

# Analyze logs
grep -E "evict|alloc|cache_size" memory.log | sort | uniq -c
```

---

## 6. Trade-offs & Decisions

| Optimization | Benefit | Trade-off |
|---|---|---|
| **Smaller cache capacity** | Lower memory footprint | More frequent cache misses (slower) |
| **Shorter cache TTL** | Fresher data, faster cleanup | Re-parsing/re-indexing overhead |
| **Buffer streaming** | Constant memory usage | Increased I/O syscalls |
| **Arc style sharing** | Reduced duplication | Slightly higher lock contention |
| **jemalloc allocator** | Better fragmentation handling | Requires external dependency |
| **LTO + aggressive optimization** | Smaller binary, faster execution | Longer compile times (dev cycle slower) |

---

## 7. Monitoring in Production

```bash
# Long-term memory monitoring script
cat > monitor_memory.sh << 'EOF'
#!/bin/bash
LOG_FILE="memory_$(date +%Y%m%d_%H%M%S).log"

echo "timestamp,rss_kb,vsz_kb,cache_entries" > "$LOG_FILE"

while true; do
  PID=$(pgrep -f "vtcode" | head -1)
  if [ -n "$PID" ]; then
    STATS=$(ps -o pid,rss,vsz= -p "$PID" 2>/dev/null)
    CACHE=$(curl -s http://localhost:9090/metrics 2>/dev/null | grep cache_entries | awk '{print $NF}')
    echo "$(date +%s),$(echo $STATS | awk '{print $2}'),$(echo $STATS | awk '{print $3}'),$CACHE" >> "$LOG_FILE"
  fi
  sleep 30
done
EOF

chmod +x monitor_memory.sh
./monitor_memory.sh
```

---

## 8. Quick Reference Checklist

- [ ] Profile with `valgrind --tool=massif` or `heaptrack`
- [ ] Check cache configuration in `vtcode-core/src/cache/mod.rs`
- [ ] Review PTY buffer sizes in `vtcode-core/src/tools/pty.rs`
- [ ] Verify LRU eviction policies trigger correctly
- [ ] Monitor global state initialization in `lazy_static` blocks
- [ ] Reduce Arc/Mutex cloning in message rendering
- [ ] Enable LTO in release profile (already done)
- [ ] Test memory usage with `#[test]` + `memory_usage()`
- [ ] Run benchmarks on representative workloads
- [ ] Export metrics for long-running sessions

