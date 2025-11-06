# VT Code Distribution Setup Guide

This guide will help you set up distribution for VT Code across multiple package managers and platforms.

## Prerequisites

### 1. Cargo (crates.io) Setup

Follow the official Cargo publishing guide: https://doc.rust-lang.org/cargo/reference/publishing.html

#### Get Your crates.io API Token

1. Go to https://crates.io/me
2. Log in with your account
3. Go to "Account Settings" → "API Tokens"
4. Generate a new token with publishing permissions
5. Copy the token (keep it secure!)

#### Local Cargo Login

```bash
# Login to crates.io (this stores your token locally)
cargo login

# When prompted, paste your API token
# The token will be stored in ~/.cargo/credentials.toml
```

#### GitHub Actions Setup

1. Go to your repository settings
2. Navigate to "Secrets and variables" → "Actions"
3. Add a new repository secret named `CRATES_IO_TOKEN`
4. Paste your crates.io API token as the value

### 4. docs.rs Documentation (Automatic)

**docs.rs** automatically generates and hosts documentation for all Rust crates published to crates.io.

#### What Gets Generated

-   **API Documentation**: Complete Rustdoc documentation for public APIs
-   **Source Code Links**: Direct links to source code on GitHub
-   **Search Functionality**: Full-text search across documentation
-   **Cross-references**: Links between related types and functions

#### URLs for VT Code

-   **Main Package**: https://docs.rs/vtcode
-   **Core Library**: https://docs.rs/vtcode-core
-   **Latest Version**: Automatically updated when new versions are published

#### Badge Integration

Add these badges to your README:

```markdown
[![docs.rs](https://img.shields.io/docsrs/vtcode)](https://docs.rs/vtcode)
[![docs.rs](https://img.shields.io/docsrs/vtcode-core)](https://docs.rs/vtcode-core)
```


4. **Homebrew** - macOS package manager
    - Install: `brew install vtcode`

## Release Process

### Using the Release Script

The updated release script supports multiple distribution channels:

```bash
# Full release (all channels)
./scripts/release.sh --patch

# Release with specific version
./scripts/release.sh 1.0.0

# Skip certain channels
./scripts/release.sh --minor --skip-npm

# Dry run to see what would happen
./scripts/release.sh --patch --dry-run
```

### What the Release Script Does

1. **Validation**: Checks authentication and metadata
2. **Version Update**: Updates version in all package files
3. **Publishing**:
    - Publishes to crates.io (if enabled)
    - Publishes to npm (if enabled)
4. **Git Operations**: Creates and pushes git tag
5. **CI Trigger**: GitHub Actions builds binaries and creates release

### Manual Steps Required

After running the release script, you may need to:

1. **Verify Releases**:
    - Check https://crates.io/crates/vtcode
    - Check https://www.npmjs.com/package/vtcode (if published)
    - Check https://github.com/vinhnx/vtcode/releases

## Troubleshooting

### Cargo Publishing Issues

```bash
# Check if you're logged in

# Verify your token
cat ~/.cargo/credentials.toml

# Test publishing (dry run)
cargo publish --dry-run
```

### npm Publishing Issues

```bash
npm whoami

# Check npm configuration
npm config list

# Test publishing (dry run)
cd npm && npm publish --dry-run
```


-   Ensure `CRATES_IO_TOKEN` secret is set in repository settings
-   Check the Actions tab for workflow run details
-   Verify the release was created successfully

## Security Notes

-   Never commit API tokens to version control
-   Use repository secrets for CI/CD tokens
-   Regularly rotate API tokens
-   Keep your local `~/.cargo/credentials.toml` secure

## Support

If you encounter issues:

1. Check the troubleshooting section above
