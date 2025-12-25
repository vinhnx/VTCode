# VTCode Release Guide

This document explains the release process for different components of the VTCode project.

## Components Overview

The VTCode project has **two separate release processes** with independent versioning:

1. **Main VTCode Binary** (Rust CLI) - Tags: `v0.39.x`
2. **VSCode Extension** - Tags: `vscode-v0.1.x`

---

## 1. Main VTCode Binary Release

The main VTCode Rust binary and related components.

### Current Version

Check current version:

```bash
cargo read-manifest | jq -r .version
```

### Release Command

```bash
# From repository root
./scripts/release.sh [patch|minor|major]

# Examples:
./scripts/release.sh patch    # 0.39.2 -> 0.39.3
./scripts/release.sh minor    # 0.39.2 -> 0.40.0
./scripts/release.sh major    # 0.39.2 -> 1.0.0
```

### Prerequisites

-   `cargo`, `rustc`, and `rustup`
-   [`cross`](https://github.com/cross-rs/cross) — `cargo install cross` (recommended for fast, reproducible macOS cross-builds)
-   Homebrew `openssl@3` (`brew install openssl@3`) for macOS builds

> The `scripts/release.sh` flow automatically installs `cross` when missing (unless `VTCODE_SKIP_AUTO_CROSS=1`). Set that environment variable if you prefer to manage installation yourself.

### Cross-compilation Configuration

VTCode includes optimized cross-compilation configuration in `Cross.toml` for building binaries across multiple platforms. See [docs/cross-compilation.md](docs/cross-compilation.md) for details about the configuration.

### What It Does

-   Bumps version in `Cargo.toml` and related files
-   Updates `CHANGELOG.md` from conventional commits
-   Builds and tests the project
-   Creates git tag: `v{version}` (e.g., `v0.39.3`)
-   Publishes to crates.io

-   Publishes to GitHub Packages (optional)
-   Builds and uploads platform-specific binaries
-   Updates Homebrew formula (optional)
-   Updates Zed extension checksums

### Options

```bash
./scripts/release.sh --help
```

---

## 2. VSCode Extension Release

The Visual Studio Code extension with marketplace publishing.

### Current Version

Check current version:

```bash
cd vscode-extension
jq -r .version package.json
```

### Release Command

```bash
# From vscode-extension directory
cd vscode-extension
./release.sh [patch|minor|major]

# Or from repository root
cd vscode-extension && ./release.sh [patch|minor|major]

# Or using cargo scripts
```

### What It Does

-   Bumps version in `package.json`
-   Updates `CHANGELOG.md` with new version and date
-   Builds the extension
-   Packages the extension (`.vsix` file)
-   Commits changes to git
-   Creates git tag: `vscode-v{version}` (e.g., `vscode-v0.1.2`)
-   Pushes to GitHub (with confirmation)
-   Publishes to VSCode Marketplace (with confirmation)
-   Publishes to Open VSX Registry (with confirmation)
-   Cleans up old `.vsix` files

### Prerequisites

The extension release script requires:

-   `node` and other build tools
-   `git`
-   `jq` (JSON processor)
-   `@vscode/vsce` (auto-installed if missing)
-   `ovsx` (auto-installed if missing)

**Publishing Credentials:**

-   VSCode Marketplace: Personal Access Token from https://marketplace.visualstudio.com/manage
-   Open VSX: Account at https://open-vsx.org/ with PAT

### More Details

See [`vscode-extension/RELEASE.md`](vscode-extension/RELEASE.md) for detailed documentation.

---

## Version Tagging Convention

To avoid conflicts in the same repository:

| Component        | Tag Format                        | Example         | Current                  |
| ---------------- | --------------------------------- | --------------- | ------------------------ |
| Main Binary      | `v{major}.{minor}.{patch}`        | `v0.39.2`       | Latest main release      |
| VSCode Extension | `vscode-v{major}.{minor}.{patch}` | `vscode-v0.1.1` | Latest extension release |

### Why Separate Versioning?

1. **Different Release Cycles**: The VSCode extension may need updates independently of the CLI
2. **Clear Separation**: Users and CI/CD can distinguish between binary and extension releases
3. **Avoid Tag Conflicts**: Both components can exist in the same repository without version collisions
4. **Marketplace Independence**: Extension versions follow marketplace conventions

---

## Quick Reference

### Release Main Binary

```bash
./scripts/release.sh patch
```

### Release VSCode Extension

```bash
cd vscode-extension && ./release.sh patch
```

### Check All Tags

```bash
# Main binary tags
git tag -l "v*" | grep -v "vscode"

# VSCode extension tags
git tag -l "vscode-v*"
```

### Create GitHub Releases

After tagging:

1. **Main Binary**: Automated via GitHub Actions after tag push
2. **VSCode Extension**: Manual at https://github.com/vinhnx/vtcode/releases/new
    - Select tag: `vscode-v{version}`
    - Add changelog from `vscode-extension/CHANGELOG.md`
    - Attach `.vsix` file for manual installation

---

## Troubleshooting

### Main Binary Release Issues

See main `README.md` and `scripts/release.sh --help`

### VSCode Extension Release Issues

See `vscode-extension/RELEASE.md` and `vscode-extension/DEVELOPMENT.md`

### Tag Conflicts

If you accidentally create a conflicting tag:

```bash
# Delete local tag
git tag -d <tag-name>

# Delete remote tag
git push origin :refs/tags/<tag-name>
```

---

## Distribution Channels

### Main VTCode Binary

-   **crates.io**: https://crates.io/crates/vtcode

-   **GitHub Packages**: https://github.com/vinhnx/vtcode/packages
-   **Homebrew**: `brew install vinhnx/tap/vtcode`
-   **GitHub Releases**: https://github.com/vinhnx/vtcode/releases

### VSCode Extension

-   **VSCode Marketplace**: https://marketplace.visualstudio.com/items?itemName=nguyenxuanvinh.vtcode-companion
-   **Open VSX Registry**: https://open-vsx.org/extension/nguyenxuanvinh/vtcode-companion
-   **Direct Download**: `.vsix` files from GitHub Releases

---

## CI/CD Integration

### Main Binary

Releases are automated via GitHub Actions (`.github/workflows/release.yml`)

### VSCode Extension

Currently manual release process. Future: Consider GitHub Actions for automated publishing.

---

For more information:

-   Main Project: [README.md](README.md)
-   VSCode Extension: [vscode-extension/README.md](vscode-extension/README.md)
-   Development: [vscode-extension/DEVELOPMENT.md](vscode-extension/DEVELOPMENT.md)
