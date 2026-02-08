# Compile Time Optimization Guide

This document outlines techniques for minimizing Rust compile times in vtcode.

## Quick Profiling Commands

```bash
# Run the profiling script
./scripts/compile-profile.sh all

# Or use individual commands:
cargo build --timings          # Timing Gantt chart
cargo llvm-lines -p vtcode-core  # LLVM IR analysis
```

## Key Optimization Techniques

### 1. Reduce Generic Function Instantiations

**Problem**: Generic functions are instantiated (monomorphized) for each unique type, generating redundant code.

**Solution**: Extract non-generic code into inner functions.

```rust
// Before: Inner code instantiated for every T
pub fn read<P: AsRef<Path>>(path: P) -> io::Result<Vec<u8>> {
    let mut file = File::open(path.as_ref())?;
    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut bytes = Vec::with_capacity(size as usize);
    io::default_read_to_end(&mut file, &mut bytes)?;
    Ok(bytes)
}

// After: Non-generic code in inner function (only instantiated once)
pub fn read<P: AsRef<Path>>(path: P) -> io::Result<Vec<u8>> {
    fn inner(path: &Path) -> io::Result<Vec<u8>> {
        let mut file = File::open(path)?;
        let size = file.metadata().map(|m| m.len()).unwrap_or(0);
        let mut bytes = Vec::with_capacity(size as usize);
        io::default_read_to_end(&mut file, &mut bytes)?;
        Ok(bytes)
    }
    inner(path.as_ref())
}
```

### 2. Consider Match vs Method Chains

In performance-critical hot paths, `match` expressions can reduce monomorphization compared to `.map()` and `.map_err()`:

```rust
// Standard (fine for most code)
let result = value.map_err(|e| MyError::from(e))?;

// Alternative for hot paths (reduces instantiations)
let result = match value {
    Ok(v) => v,
    Err(e) => return Err(MyError::from(e)),
};
```

> **Note**: Only apply this optimization after profiling confirms it's necessary. Readability usually outweighs compile-time savings.

### 3. Minimize Derive Macro Usage

Each `#[derive(Serialize, Deserialize)]` generates code. For types that don't need serialization, omit these derives.

### 4. Build Configuration

vtcode already optimizes dev builds in `Cargo.toml`:

```toml
[profile.dev]
debug = 0
incremental = false  # Enables sccache; set CARGO_INCREMENTAL=1 to override

[profile.dev.build-override]
opt-level = 3  # Faster proc-macro execution

[profile.dev.package."*"]
opt-level = 1  # Slightly optimize deps (cached anyway)
```

## Current vtcode Profile

Based on `cargo llvm-lines` analysis (Feb 2024):

| Function | Copies | Impact |
|----------|--------|--------|
| `Result::map_err` | 531 | Standard usage |
| `Option::map` | 483 | Standard usage |
| `base_function_declarations` | 1 | Large but singular |

**Conclusion**: vtcode's compile times are reasonable. No aggressive refactoring needed.

## Additional Resources

- [Tips for Faster Rust Compile Times](https://corrode.dev/blog/tips-for-faster-rust-compile-times/)
- [Cargo Timings Documentation](https://doc.rust-lang.org/nightly/cargo/reference/timings.html)
- [cargo-llvm-lines](https://github.com/dtolnay/cargo-llvm-lines/)
