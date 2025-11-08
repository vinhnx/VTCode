# npm Publishing Guide for VT Code

## Overview

The npm package `@vinhnx/vtcode` is published to GitHub Packages and provides a convenient way to install VT Code binaries through npm.

## Setup Requirements

### 1. GitHub Personal Access Token (PAT)

Create a GitHub PAT with the following scopes:
- `write:packages` - Publish packages
- `read:packages` - Download packages  
- `repo` - Link to repositories

See: https://github.com/settings/tokens

### 2. Configure NPM Registry

Add your GitHub token to your npm configuration:

```bash
npm config set //npm.pkg.github.com/:_authToken YOUR_GITHUB_TOKEN
npm config set @vinhnx:registry https://npm.pkg.github.com
```

Or use environment variable during publish:

```bash
export NODE_AUTH_TOKEN=YOUR_GITHUB_TOKEN
```

### 3. Create .npmrc (Optional)

Copy `.npmrc.example` to your home directory and add your token:

```bash
cp npm/.npmrc.example ~/.npmrc
# Edit ~/.npmrc and add your GitHub token
```

## Release Process

The main release script (`scripts/release.sh`) automatically:

1. **Updates npm version**: Syncs npm package.json version with the main project
2. **Runs cargo release**: Publishes crates to crates.io and creates git tags
3. **Publishes to GitHub Packages**: Automatically publishes the npm package

### Usage

```bash
# Standard release (patch version bump)
./scripts/release.sh

# Minor version bump
./scripts/release.sh --minor

# Major version bump  
./scripts/release.sh --major

# Pre-release (alpha/beta/rc)
./scripts/release.sh --pre-release
./scripts/release.sh --pre-release-suffix beta.1

# Dry run (shows what would happen)
./scripts/release.sh --dry-run

# Skip npm publishing
./scripts/release.sh --skip-npm
```

### Environment Variables

```bash
# Required for npm publishing to GitHub Packages
export GITHUB_TOKEN=your_token_here

# Recommended for headless/CI environments
export NODE_AUTH_TOKEN=$GITHUB_TOKEN
```

## Manual Publishing

If the automatic publish fails, you can manually publish:

```bash
cd npm
npm publish --registry https://npm.pkg.github.com
```

Or use the helper script:

```bash
cd npm
node scripts/publish-to-github.js
```

## Verification

After publishing, verify the package:

```bash
# View package on GitHub Packages
https://github.com/vinhnx/vtcode/pkgs/npm/vtcode

# View detailed version info
npm view @vinhnx/vtcode

# Install and test locally
npm install @vinhnx/vtcode
vtcode --version
```

## Package Contents

The published npm package includes:

- `index.js` - CLI wrapper that downloads and executes the binary
- `package.json` - Package metadata
- `scripts/postinstall.js` - Auto-downloads platform-specific binary
- `scripts/preuninstall.js` - Cleanup on uninstall
- `scripts/publish-to-github.js` - Manual publish script

## Troubleshooting

### 401 Unauthorized

**Cause**: GitHub token not properly configured or expired

**Solution**:
```bash
npm config set //npm.pkg.github.com/:_authToken $GITHUB_TOKEN
npm cache clean --force
npm publish --registry https://npm.pkg.github.com
```

### Binary Download Fails Post-Install

**Cause**: Release binary not yet available when package is installed

**Solution**: 
- Ensure GitHub release exists with binaries: https://github.com/vinhnx/vtcode/releases
- Wait a few minutes after release for all binaries to finish building
- Try reinstalling: `npm install @vinhnx/vtcode`

### Permission Denied During Postinstall

**Cause**: Insufficient permissions to create bin directory

**Solution**:
```bash
# Reinstall with sudo (not recommended)
sudo npm install @vinhnx/vtcode -g

# Or fix npm permissions
mkdir ~/.npm-global
npm config set prefix '~/.npm-global'
export PATH=~/.npm-global/bin:$PATH
```

### Wrong Binary Downloaded

**Cause**: Platform/architecture detection issue

**Solution**:
```bash
# Check your platform/arch
node -e "console.log(process.platform, process.arch)"

# Manually download correct binary from releases
# https://github.com/vinhnx/vtcode/releases
# Copy to: node_modules/@vinhnx/vtcode/bin/
```

## CI/CD Integration

### GitHub Actions

The release workflow (`.github/workflows/release.yml`) automatically:

1. Detects version tags (v0.39.x)
2. Generates changelog
3. Creates GitHub release
4. Triggers the release script which publishes npm package

### Manual Trigger

```bash
git tag v0.40.0
git push origin v0.40.0
# GitHub Actions will handle the rest
```

## Security Considerations

1. **Never commit tokens** - Always use environment variables or .gitignore
2. **Token scope** - Use minimal scopes required (avoid admin access)
3. **Rotate regularly** - Regenerate tokens periodically
4. **Package name** - The package now publishes to both npmjs.org and GitHub Packages using the name "vtcode"

## References

- [GitHub Packages npm Registry](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-npm-registry)
- [Creating Personal Access Tokens](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens)
- [npm Publishing Guide](https://docs.npmjs.com/packages-and-modules/contributing-packages-to-the-registry)
