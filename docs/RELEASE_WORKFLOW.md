# Release Workflow - Cost Optimization

## Overview

This document describes the cost-optimized release workflow for VT Code, which builds binaries across multiple platforms while minimizing GitHub Actions costs.

## Architecture

### Hybrid Approach (Default)

```
┌─────────────────────────────────────────────────────────────────┐
│                    Local Release Script                          │
│                    ./scripts/release.sh                          │
└─────────────────────────────────────────────────────────────────┘
                              │
         ┌────────────────────┼────────────────────┐
         │                    │                    │
         ▼                    ▼                    ▼
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│   macOS Build   │  │  Trigger CI     │  │  Create GitHub  │
│   (Local)        │  │  (Linux + Win)  │  │  Release        │
│                 │  │                 │  │                 │
│ • x86_64        │  │ • ubuntu-latest │  │ • Draft release │
│ • aarch64       │  │ • windows-      │  │ • Upload all    │
│                 │  │   latest        │  │   binaries      │
└─────────────────┘  └─────────────────┘  └─────────────────┘
                            │
                            ▼
                   ┌─────────────────┐
                   │  Download CI    │
                   │  Artifacts      │
                   │                 │
                   │ • Linux x86_64  │
                   │ • Windows x86_64│
                   └─────────────────┘
```

### Full CI Approach (--full-ci flag)

```
┌─────────────────────────────────────────────────────────────────┐
│                    Local Release Script                          │
│              ./scripts/release.sh --full-ci                      │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
                   ┌─────────────────────────┐
                   │  Trigger Full CI        │
                   │  release.yml            │
                   │                         │
                   │  • macOS (both archs)   │
                   │  • Linux (both archs)   │
                   │  • Windows (both archs) │
                   └─────────────────────────┘
                              │
                              ▼
                   ┌─────────────────────────┐
                   │  CI Builds All Platforms│
                   │  & Creates Release      │
                   └─────────────────────────┘
```

## Workflows

### 1. `release.yml` - Full CI Build (All Platforms)
- **Purpose**: Manual releases building ALL platforms on GitHub Actions
- **Platforms**: macOS (x86_64 + aarch64), Linux (x86_64 + aarch64), Windows (x86_64 + aarch64)
- **Cost**: $0 for public repos (GitHub-hosted runners are free)
- **Use case**: 
  - When you want GitHub to build everything
  - Local Mac is slow or unavailable
  - Re-running failed builds
- **Trigger**: `gh workflow run release.yml --field tag=0.81.2` or `./scripts/release.sh --full-ci`

### 2. `build-linux-windows.yml` - CI for Linux + Windows Only
- **Purpose**: Triggered by local release script to build non-macOS platforms
- **Platforms**: Linux x86_64, Windows x86_64
- **Cost**: $0 for public repos
- **Use case**: Hybrid releases (macOS built locally)
- **Trigger**: Automatically by `./scripts/release.sh` (without `--full-ci`)

### 3. `release.sh` - Local Release Script
- **Purpose**: Orchestrates the entire release process
- **Modes**:
  - **Hybrid (default)**: Build macOS locally + trigger CI for Linux/Windows
  - **Full CI (`--full-ci`)**: Trigger CI for ALL platforms
- **Flow (Hybrid)**:
  1. Build macOS binaries locally (both architectures)
  2. Update changelog from commits
  3. Run `cargo release` (publish to crates.io, create git tag)
  4. Trigger CI workflow for Linux + Windows builds
  5. Wait for CI to complete (with 60-minute timeout)
  6. Download CI artifacts
  7. Create GitHub Release with ALL binaries
  8. Update Homebrew formula
- **Flow (Full CI)**:
  1. Update changelog from commits
  2. Run `cargo release` (publish to crates.io, create git tag)
  3. Trigger `release.yml` workflow for all platforms
  4. CI builds all platforms and creates release

## Usage

### Recommended: Local Release Script (Hybrid Approach)

```bash
# Release with patch version bump (default)
# Builds macOS locally, triggers CI for Linux/Windows
./scripts/release.sh

# Release with specific version
./scripts/release.sh 0.81.2

# Release with minor version bump
./scripts/release.sh --minor

# Dry run (no publishing)
./scripts/release.sh --dry-run

# Skip crates.io publishing
./scripts/release.sh --skip-crates

# Skip binary builds (only cargo release)
./scripts/release.sh --skip-binaries
```

### Alternative: Full CI Build (All Platforms on GitHub Actions)

```bash
# Option 1: Use release.sh with --full-ci flag
./scripts/release.sh --full-ci 0.81.2

# Option 2: Trigger workflow directly
gh workflow run release.yml --field tag=0.81.2

# Or use GitHub Actions UI: https://github.com/vinhnx/vtcode/actions/workflows/release.yml
```

### When to Use Each Approach

| Approach | Command | Best For |
|----------|---------|----------|
| **Hybrid (default)** | `./scripts/release.sh` | Regular releases, faster macOS builds |
| **Full CI** | `./scripts/release.sh --full-ci` | When local Mac is slow/unavailable |
| **Manual Workflow** | `gh workflow run release.yml` | Re-running failed builds, testing |

## Cost Analysis

### Hybrid Approach (Default: macOS Local + Linux/Windows CI)
- **Local macOS build**: 2 builds × ~10 min = ~20 min (your machine)
- **CI Linux + Windows**: 2 builds × ~10 min = ~20 minutes/release
- **Multiple releases/week (4/month)**: ~80 minutes/month on CI
- **Cost**: **$0** (public repo - free!)

### Full CI Approach (--full-ci: All Platforms on GitHub Actions)
- **CI macOS + Linux + Windows**: 6 builds × ~10 min = ~60 minutes/release
- **Multiple releases/week (4/month)**: ~240 minutes/month on CI
- **Cost**: **$0** (public repo - free!)

> **Note**: Since your repo is **public**, GitHub Actions is **completely free** for standard runners on both approaches! The hybrid approach mainly:
> - Reduces CI queue time
> - Uses your local Mac for faster iteration
> - Keeps CI minutes available for other workflows

## Platform Coverage

| Platform | Architecture | Build Location | Notes |
|----------|-------------|----------------|-------|
| macOS | aarch64 (M1-M4) | Local | Apple Silicon |
| macOS | x86_64 (Intel) | Local | Older Macs |
| Linux | x86_64 | CI (GitHub Actions) | Most common |
| Linux | aarch64 (ARM) | ❌ Skipped | Niche use case |
| Windows | x86_64 | CI (GitHub Actions) | Most common |
| Windows | aarch64 (ARM) | ❌ Skipped | Rare devices |

## Troubleshooting

### CI Build Fails
```bash
# Check workflow run logs
gh run view <run-id> --log

# Re-trigger CI for specific tag
gh workflow run build-linux-windows.yml --field tag=0.81.2
```

### Artifact Download Fails
```bash
# List recent workflow runs
gh run list --workflow build-linux-windows.yml

# Manually download artifacts
gh run download <run-id> --dir ./artifacts
```

### Release Script Hangs
The script has a 60-minute timeout for CI builds. If it hangs:
```bash
# Check CI status
gh run list --workflow build-linux-windows.yml

# Kill the script and retry
Ctrl+C
./scripts/release.sh --skip-binaries  # Skip to upload step
```

## Future Improvements

1. **Add ARM64 Linux/Windows** if user demand increases
2. **Parallel macOS builds** using GitHub Actions (if local machine is slow)
3. **Automatic release notes** from changelog
4. **Smoke tests** for uploaded binaries before publishing

## References

- [GitHub Actions Pricing](https://docs.github.com/en/billing/managing-billing-for-github-actions/about-billing-for-github-actions)
- [cargo-release](https://github.com/crate-ci/cargo-release)
- [GitHub CLI](https://cli.github.com/)
