# VT Code Distribution Setup

This document outlines the complete distribution setup for VT Code across multiple package managers and platforms.

## Distribution Channels

### Current Release Status (v0.11.1)

-   **Status: Complete** – Published crate: [vtcode on crates.io](https://crates.io/crates/vtcode)
-   **Status: Complete** – Documentation refreshed: [docs.rs/vtcode 0.11.1](https://docs.rs/vtcode/0.11.1) and [docs.rs/vtcode-core 0.11.1](https://docs.rs/vtcode-core/0.11.1)
-   **Status: Complete** – Release tag: [GitHub v0.11.1](https://github.com/vinhnx/vtcode/releases/tag/v0.11.1) with binaries for macOS, Linux, and Windows
-   **Status: Monitoring** – CI tracking: [GitHub Actions dashboard](https://github.com/vinhnx/vtcode/actions) (docs.rs propagation typically needs 10-30 minutes)

### 1. Cargo (crates.io)

-   **Primary Rust package repository**
-   **Location**: `https://crates.io/crates/vtcode`
-   **Workflow**: `.github/workflows/publish-crates.yml`
-   **Metadata**: Added to `Cargo.toml` and `vtcode-core/Cargo.toml`

### 2. npm (Cross-platform)

-   **Package**: `vtcode` (when published)
-   **Installation**: `npm install -g vtcode`
-   **Structure**: `npm/` directory with postinstall script
-   **Location**: `https://www.npmjs.com/package/vtcode`
-   **Workflow**: Uses postinstall script to download platform-specific binaries from GitHub Releases

### 3. GitHub Releases

-   **Binaries**: Pre-built for multiple platforms
-   **Workflow**: `.github/workflows/build-release.yml`
-   **Platforms**: Linux x64, macOS x64/ARM64, Windows x64

## File Structure

```
vtcode/
├── Cargo.toml                    # Main crate metadata
├── vtcode-core/
│   └── Cargo.toml               # Core library metadata
├── npm/
│   ├── package.json            # npm package config
│   ├── README.md               # npm package README
│   ├── index.js               # Main entry point
│   ├── bin/
│   │   └── vtcode            # Executable wrapper
│   └── scripts/
│       ├── postinstall.js     # Binary download script
│       └── preuninstall.js    # Cleanup script
├── .github/workflows/
│   ├── publish-crates.yml     # Cargo publishing
│   ├── build-release.yml      # Binary builds
│   └── release.yml           # Release creation
└── scripts/
    ├── release.sh            # Release management
    └── test-distribution.sh  # Distribution validation
```

## Release Process

1. **Create Release**: Use `./scripts/release.sh` to bump version and create git tag
2. **Build Binaries**: GitHub Actions automatically builds binaries for all platforms
3. **Publish to Cargo**: Automatically publishes to crates.io
4. **Publish to npm**: Automatically publishes to npm registry

## Validation

Run `./scripts/test-distribution.sh` to validate the entire setup before releasing.

## Secrets Required

-   `CRATES_IO_TOKEN`: For publishing to crates.io
-   `NPM_TOKEN`: For publishing to npm registry (optional, can use npm login)
-   `GITHUB_TOKEN`: Automatically provided by GitHub Actions

## Next Steps

1. Create a test release to validate the pipeline
2. Set up npm publishing (requires npm account)
3. Update documentation with final installation URLs
