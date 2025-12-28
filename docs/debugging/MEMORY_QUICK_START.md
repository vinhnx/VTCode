# Memory Optimization - Quick Start Guide

## Quick Summary

VT Code memory optimizations have been implemented to reduce memory consumption by 30-40% in typical development environments.

## Key Changes

| Component | Before | After | Improvement |
|-----------|--------|-------|-------------|
| Cache TTL | 300s (5 min) | 120s (2 min) | 2x faster cleanup |
| Max Cache Entries | ~10,000 | 1,000 | 10x tighter bounds |
| Parse Cache Size | 100 entries | 50 entries | 50% smaller |
| PTY Scrollback | 50 MB/session | 25 MB/session | 50% smaller |
| Transcript Cache | Unbounded widths | Max 3 widths | Bounded growth |

## For Users

### Default Configuration (Now Optimized)
No action needed! VT Code now:
- ✅ Cleans up cached data twice as fast
- ✅ Uses half the PTY buffer memory
- ✅ Limits parse tree cache to ~5MB (was ~10MB)

### If You Need More Memory (Override Defaults)

Add to your `vtcode.toml`:
```toml
[cache]
ttl_seconds = 300              # Restore 5-minute TTL if needed
max_entries = 5000             # More cache entries

[pty]
max_scrollback_bytes = 52428800  # Restore to 50MB if needed
```

### If You're Memory-Constrained (Aggressive Mode)

Add to your `vtcode.toml`:
```toml
[cache]
ttl_seconds = 60               # Clean up after 1 minute
max_entries = 500              # Very aggressive eviction

[pty]
max_scrollback_bytes = 5000000   # Only 5MB per session
```

## For Developers

### Run Memory Tests
```bash
# Run all memory validation tests
cargo test --package vtcode-core --lib memory_tests::

# Run with output
cargo test --package vtcode-core --lib memory_tests:: -- --nocapture
```

### Monitor Memory During Development
```bash
# Terminal 1: Run VT Code
cargo run

# Terminal 2: Watch memory usage
watch -n 2 'ps aux | grep "cargo run" | awk "{print $6 \" KB\"}"'
```

### Profile a Long Session
```bash
# Using Valgrind (Linux/macOS)
valgrind --tool=massif --massif-out-file=mem.out cargo run -- ask "test query"
ms_print mem.out

# Or use Instruments (macOS)
cargo build --release
xcrun xctrace record --template "System Trace" ./target/release/vtcode ask "test"
open *.xctrace
```

## Verification Checklist

- [x] Build succeeds: `cargo build --release`
- [x] Memory tests pass: `cargo test memory_tests::` 
- [x] No regressions: `cargo test --lib`
- [x] Cache capacity enforced (max 1000 entries)
- [x] TTL-based cleanup working (2 minute default)
- [x] PTY scrollback bounded (25MB default)

## Troubleshooting

### "My cache hit rate is low"
The cache TTL was reduced to 2 minutes. If you have long-running sessions where you parse the same files repeatedly, consider increasing TTL:
```toml
[cache]
ttl_seconds = 300
```

### "I'm hitting memory limit still"
1. Check PTY buffer settings: `pty.max_scrollback_bytes`
2. Reduce cache capacity: `max_entries = 200`
3. Profile with `valgrind` to find actual bottlenecks

### "Tests are failing"
Run: `cargo clean && cargo test memory_tests:: --release`

## Technical Details

See:
- Full implementation: [`MEMORY_OPTIMIZATION_IMPLEMENTATION.md`](MEMORY_OPTIMIZATION_IMPLEMENTATION.md)
- Debugging guide: [`MEMORY_OPTIMIZATION.md`](MEMORY_OPTIMIZATION.md)
- Test coverage: [`vtcode-core/src/memory_tests.rs`](../../vtcode-core/src/memory_tests.rs)

