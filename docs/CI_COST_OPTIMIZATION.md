# CI Cost Optimization Guide

This document outlines the cost optimization strategies implemented for VT Code's release workflow.

## Overview

GitHub Actions billing is based on **runner minutes**. Different runner types have different costs:

| Runner           | Cost Multiplier             | Estimated Time | Cost per Release |
| ---------------- | --------------------------- | -------------- | ---------------- |
| `ubuntu-latest`  | 1x (free for public repos)  | ~10 min        | $0               |
| `macos-latest`   | 10x (free for public repos) | ~15 min        | $0               |
| `windows-latest` | 2x (free for public repos)  | ~10 min        | $0               |

**Note:** VT Code is a public repository, so all GitHub Actions usage is **FREE** (unlimited minutes). However, these optimizations still matter for:

- Faster release iterations
- Reduced queue times
- Better resource efficiency

## Implemented Optimizations

### 1. Hybrid Release Strategy (Default - Recommended)

**Script:** `./scripts/release.sh`

```
macOS binaries    → Built locally (0 CI minutes)
Linux binaries    → Built on CI (ubuntu-latest, free)
Windows binaries  → Built on CI (windows-latest, free)
```

**Benefits:**

- Fastest release iteration (macOS builds in parallel with CI)
- No dependency on macOS runner availability
- Reduces CI runner usage by 50%

**Usage:**

```bash
./scripts/release.sh --minor
```

### 2. Reduced Platform Matrix

**Before:** 6 platforms

- macOS x86_64, aarch64
- Linux x86_64, aarch64
- Windows x86_64, aarch64

**After:** 4 platforms (essential only)

- macOS x86_64, aarch64
- Linux x86_64 (most common)
- Windows x86_64 (most common)

**Removed:**

- `aarch64-unknown-linux-gnu` - Low demand, requires expensive ARM runner or Cross
- `aarch64-pc-windows-msvc` - Very low demand, Windows ARM adoption is minimal

**Savings:** ~33% reduction in CI runner time

### 3. Shallow Git Clone

**Before:** `fetch-depth: 0` (full history)
**After:** `fetch-depth: 1` (latest commit only)

**Savings:** ~30-60 seconds per job (faster checkout)

### 4. Optimized Artifact Retention

**Before:** `retention-days: 1` (too short, may expire before use)
**After:** `retention-days: 7` (balanced)

**Benefits:**

- Enough time for manual download if needed
- Automatic cleanup reduces storage costs
- Compression level 6 for smaller artifacts

### 5. Efficient Caching

```yaml
- name: Cache Rust dependencies
  uses: Swatinem/rust-cache@v2
  with:
      cache-on-failure: true
      key: ${{ matrix.target }} # Per-target caching
```

**Benefits:**

- Faster builds on retry
- Reduced redundant compilation

### 6. Removed Unnecessary Jobs

**Removed:**

- `build-summary` job (added no value, consumed runner time)
- Verbose logging steps (`ls -lh`, debug output)
- `npm install` for changelog generation (use simple git log instead)

## Release Modes Comparison

### Default Mode (Recommended)

```bash
./scripts/release.sh --minor
```

- **CI Usage:** 2 runners (Ubuntu + Windows)
- **Local Build:** macOS (parallel)
- **Total Time:** ~15-20 minutes
- **Cost:** Free (public repo)

### Full CI Mode

```bash
./scripts/release.sh --minor --full-ci
```

- **CI Usage:** 4 runners (2x macOS + Ubuntu + Windows)
- **Local Build:** None
- **Total Time:** ~20-30 minutes
- **Cost:** Free (public repo)
- **Use Case:** When local macOS build is not possible

### CI-Only Mode (Linux/Windows)

```bash
./scripts/release.sh --minor --ci-only
```

- **CI Usage:** 2 runners (Ubuntu + Windows)
- **Local Build:** None (skip macOS)
- **Total Time:** ~15 minutes
- **Cost:** Free (public repo)
- **Use Case:** When macOS binaries already built

### Skip Binaries Mode

```bash
./scripts/release.sh --minor --skip-binaries
```

- **CI Usage:** None
- **Local Build:** None
- **Total Time:** ~2-5 minutes
- **Use Case:** Crates.io-only releases

## Cost Breakdown by Workflow

### `build-linux-windows.yml` (Default)

| Job            | Runner         | Time        | Cost     |
| -------------- | -------------- | ----------- | -------- |
| Linux x86_64   | ubuntu-latest  | ~10 min     | Free     |
| Windows x86_64 | windows-latest | ~10 min     | Free     |
| **Total**      |                | **~20 min** | **Free** |

### `release.yml` (Full CI - All Platforms)

| Job            | Runner         | Time        | Cost     |
| -------------- | -------------- | ----------- | -------- |
| macOS aarch64  | macos-latest   | ~15 min     | Free     |
| macOS x86_64   | macos-latest   | ~15 min     | Free     |
| Linux x86_64   | ubuntu-latest  | ~10 min     | Free     |
| Windows x86_64 | windows-latest | ~10 min     | Free     |
| Create Release | ubuntu-latest  | ~2 min      | Free     |
| **Total**      |                | **~52 min** | **Free** |

## Best Practices

### ✅ Do

- Use default mode (`./scripts/release.sh`) for most releases
- Use `--ci-only` when macOS binaries are pre-built
- Use `--skip-binaries` for docs-only or crates.io-only releases
- Keep artifact retention at 7 days
- Use shallow clones (`fetch-depth: 1`)

### ❌ Don't

- Use `--full-ci` unless necessary (slower, more resource usage)
- Build all 6 platforms (low ROI on ARM64 Linux/Windows)
- Keep artifacts indefinitely (storage costs)
- Run full git history fetch (wastes time)

## Future Optimizations

### Potential Improvements

1. **Self-hosted runners** - If release frequency increases
2. **Build caching across workflows** - Share target/ directory
3. **Incremental builds** - Only rebuild changed crates
4. **Platform-on-demand** - Build ARM64 only when requested

### Monitoring

- Track release workflow duration in GitHub Actions tab
- Monitor artifact storage usage
- Review platform download statistics to justify platform support

## Summary

| Optimization                 | Time Saved              | Complexity |
| ---------------------------- | ----------------------- | ---------- |
| Hybrid release (local macOS) | ~15 min                 | Low        |
| Reduced platform matrix      | ~20 min                 | Low        |
| Shallow clone                | ~2 min                  | Low        |
| Removed unnecessary jobs     | ~5 min                  | Low        |
| **Total Savings**            | **~42 min per release** |            |

**Result:** 60% faster releases, 50% less CI usage, simpler workflow.
