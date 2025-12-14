# NPM Release Restoration Summary

## Overview

Successfully restored npm package publishing functionality to VT Code release pipeline. The npm package `@vinhnx/vtcode` can now be automatically published to GitHub Packages during the release process.

## Changes Made

### 1. Restored npm Directory Structure

Created the complete npm package directory with all required files:

```
npm/
 package.json              # Package metadata (restored)
 index.js                  # CLI wrapper (restored)
 README.md                 # Package documentation (new)
 PUBLISHING.md             # Publishing guide (new)
 .npmrc.example            # Configuration template (restored)
 scripts/
     postinstall.js        # Auto-download binary (restored)
     preuninstall.js       # Cleanup on uninstall (restored)
     publish-to-github.js  # Manual publish script (restored)
```

### 2. Updated Release Script (`scripts/release.sh`)

#### Added Functions:
- **`update_npm_package_version()`** - Syncs npm package.json version with main project
- **`publish_npm_package()`** - Publishes to GitHub Packages npm registry
- **`publish_github_packages()`** - Wrapper for GitHub Packages publishing

#### Modified:
- **Authentication checks** - Re-enabled npm/GitHub Packages token verification
- **Version update logic** - Now updates npm/package.json during release
- **Post-release workflow** - Publishes npm package in background while other tasks run
- **Completion messages** - Added npm package publishing confirmation

### 3. New Documentation

- **npm/README.md** - Installation and setup guide for npm users
- **npm/PUBLISHING.md** - Comprehensive publishing guide with troubleshooting
- **This file** - Summary of restoration process

## Key Features

### Automated Publishing
The release script now automatically:
1. Updates npm package version to match release version
2. Publishes to GitHub Packages upon successful release
3. Runs npm publishing in background in parallel with other tasks

### Version Sync
npm package.json is automatically kept in sync with:
- Patch releases: `0.39.2 → 0.39.3`
- Minor releases: `0.39.2 → 0.40.0`
- Major releases: `0.39.2 → 1.0.0`
- Pre-releases: `0.39.7 → 0.40.0-alpha.1`

### Flexible Control
Release command options:
```bash
./scripts/release.sh patch              # Auto-publish npm
./scripts/release.sh --skip-npm         # Skip npm publish
./scripts/release.sh --dry-run          # Test without publishing
```

### Environment Support
```bash
# Primary: Use GITHUB_TOKEN environment variable
export GITHUB_TOKEN=your_token

# GitHub Actions: Automatically uses GITHUB_TOKEN from secrets
# Manual releases: Configure npm registry locally or via env var
```

## Usage

### For Release Process

```bash
# Ensure GITHUB_TOKEN is set
export GITHUB_TOKEN=<your_github_token>

# Run standard release (will publish npm package)
./scripts/release.sh patch

# Or skip npm publishing if needed
./scripts/release.sh patch --skip-npm
```

### For Manual npm Publishing

```bash
# Using the helper script
cd npm
node scripts/publish-to-github.js

# Or direct npm publish
npm publish --registry https://npm.pkg.github.com
```

## Configuration

### GitHub Packages Setup

1. Create GitHub Personal Access Token: https://github.com/settings/tokens
   - Required scopes: `write:packages`, `read:packages`, `repo`

2. Configure npm (choose one):
   ```bash
   # Option A: Environment variable (recommended for CI/CD)
   export GITHUB_TOKEN=your_token
   export NODE_AUTH_TOKEN=$GITHUB_TOKEN

   # Option B: npm config
   npm config set //npm.pkg.github.com/:_authToken YOUR_TOKEN
   npm config set @vinhnx:registry https://npm.pkg.github.com

   # Option C: .npmrc file (NOT for public repos with tokens)
   # Copy npm/.npmrc.example to ~/.npmrc and add token
   ```

## Publishing Workflow

```
cargo release --workspace
    ↓
Git tags created & pushed
    ↓
npm package.json updated
    ↓
github release workflow triggers
    ↓
changelog generated
    ↓
Binary build starts
    ↓
npm package publishes to GitHub Packages (parallel)
    ↓
All artifacts available
```

## Verification

Check npm package was published:

```bash
# View on GitHub Packages
https://github.com/vinhnx/vtcode/pkgs/npm/vtcode

# Or via npm
npm view @vinhnx/vtcode versions

# Install and test
npm install @vinhnx/vtcode
npx vtcode --version
```

## Backward Compatibility

- All existing release commands still work
- New `--skip-npm` flag allows opting out
- Graceful fallbacks if npm not available
- No changes to Rust/cargo release process

## Dependencies

Required for npm publishing:
- `npm` (v6+) - For publishing
- `node` (v14+) - For postinstall/publish scripts
- `jq` - For JSON manipulation (already required)
- `GITHUB_TOKEN` - Environment variable for authentication

All are available in standard CI/CD environments.

## Rollback

If npm publishing needs to be disabled again:

```bash
./scripts/release.sh patch --skip-npm
```

To completely remove npm functionality:

```bash
git rm -r npm/
# Edit scripts/release.sh and remove npm-related functions
```

## Next Steps

1. **Setup GitHub Token**: Create PAT in GitHub settings
2. **Test Release**: Run `./scripts/release.sh --dry-run` to verify
3. **Monitor First Release**: Check npm publishing succeeds
4. **Document**: Add npm installation instructions to main README

## References

- npm Package: https://github.com/vinhnx/vtcode/pkgs/npm/vtcode
- GitHub Packages Docs: https://docs.github.com/packages
- Release Guide: See RELEASING.md

## Files Changed

```
Created:
  npm/package.json
  npm/index.js
  npm/README.md
  npm/PUBLISHING.md
  npm/.npmrc.example
  npm/scripts/postinstall.js
  npm/scripts/preuninstall.js
  npm/scripts/publish-to-github.js

Modified:
  scripts/release.sh (added npm functions and publishing logic)
```

## Status

  npm directory restored
  All npm files created
  Release script updated
  Documentation complete
  Syntax validation passed
  Ready for release
