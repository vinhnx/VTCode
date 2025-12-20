# NPM Publishing Troubleshooting Guide

This guide helps resolve common npm publishing issues for the VT Code project.

## Common Issues

### 1. Missing Binary File Error

**Error:** `npm warn package-json vtcode-bin@0.50.9 No bin file found at bin/vtcode`

**Cause:** The npm package.json specifies a binary entry `bin/vtcode`, but this file doesn't exist at publish time.

**Solution (Already Applied):**
- Created `npm/bin/vtcode` stub file that satisfies npm's validation
- The stub provides helpful error messages if somehow reached
- The release script now auto-creates this stub if missing

### 2. npmjs.com Authentication Error

**Error:** `npm notice Access token expired or revoked. Please try logging in again.`

**Error:** `npm error 404 Not Found - PUT https://registry.npmjs.org/vtcode-bin - Not found`

**Cause:** No valid authentication token for npmjs.com

**Solution Options:**

#### Option A: Use npm login (Local Development)
```bash
npm login
# Enter your npm username, password, and email
# Then test with:
npm whoami
```

#### Option B: Use NPM_TOKEN Environment Variable
```bash
# Get your npm token from https://www.npmjs.com/settings/tokens
export NPM_TOKEN=npm_XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
echo "//registry.npmjs.org/:_authToken=\${NPM_TOKEN}" > ~/.npmrc
```

#### Option C: Trusted Publishing (CI/CD - Recommended)
Configure Trusted Publishing for your npm package:
1. Go to https://www.npmjs.com/settings/integrations
2. Add your GitHub repository as a trusted publisher
3. No token needed - uses OIDC authentication

### 3. GitHub Packages Authentication Error

**Error:** `npm error 401 Unauthorized - PUT https://npm.pkg.github.com/@vinhnx%2fvtcode - unauthenticated`

**Cause:** No valid authentication token for GitHub Packages

**Solution Options:**

#### Option A: Use GITHUB_TOKEN Environment Variable
```bash
# Create a GitHub Personal Access Token with scopes:
# - write:packages
# - read:packages
# - repo
# Get it from: https://github.com/settings/tokens

export GITHUB_TOKEN=github_pat_XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
echo "//npm.pkg.github.com/:_authToken=\${GITHUB_TOKEN}" >> ~/.npmrc
```

#### Option B: Trusted Publishing (CI/CD - Recommended)
GitHub Actions automatically provides `GITHUB_TOKEN` to workflows.
Ensure your workflow has proper permissions:
```yaml
permissions:
  contents: write
  packages: write
```

#### Option C: Use gh CLI (Local Development)
```bash
gh auth login
# Then configure npm to use gh CLI token:
npm config set @vinhnx:registry https://npm.pkg.github.com
npm config set -- //npm.pkg.github.com/:_authToken=$(gh auth token)
```

## Testing Authentication

### Test npmjs.com Authentication
```bash
npm whoami --registry https://registry.npmjs.org/
```

### Test GitHub Packages Authentication
```bash
npm whoami --registry https://npm.pkg.github.com/
```

## Manual Publishing (If Automated Release Fails)

### Publish to npmjs.com
```bash
cd npm
# First, ensure you're logged in:
npm login
# or set NPM_TOKEN

# Publish with different package name
node scripts/publish-to-npmjs.js
```

### Publish to GitHub Packages
```bash
cd npm
# Ensure GitHub token is set:
export GITHUB_TOKEN=your_token_here

# Publish
npm publish --registry https://npm.pkg.github.com
```

## CI/CD Configuration

### GitHub Actions Example
```yaml
- name: Publish to npm
  run: ./scripts/release.sh
  env:
    NPM_TOKEN: ${{ secrets.NPM_TOKEN }}
    GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

### Environment Variables Needed
- `NPM_TOKEN` - npmjs.com access token (if not using trusted publishing)
- `GITHUB_TOKEN` - GitHub token for GitHub Packages (automatic in Actions, or PAT locally)

## Verifying the Fix

After applying fixes, test with:

```bash
# Check bin stub exists
ls -la npm/bin/vtcode

# Test npm authentication
npm whoami

# Test GitHub authentication
npm whoami --registry https://npm.pkg.github.com/

# Run a dry-run publish (preview only)
cd npm
npm publish --dry-run --registry https://registry.npmjs.org/
npm publish --dry-run --registry https://npm.pkg.github.com/
```

## Next Steps

1. Set up authentication using one of the methods above
2. Ensure `npm/bin/vtcode` stub exists (should be automatic now)
3. Re-run the release script

```bash
./scripts/release.sh --patch
```

## Getting Help

If issues persist:

1. Check npm logs: `cat ~/.npm/_logs/*.log`
2. Verify package.json structure in `npm/package.json`
3. Ensure `NPM_TOKEN` and/or `GITHUB_TOKEN` are set
4. For trusted publishing issues, visit:
   - https://docs.npmjs.com/trusted-publishers
   - https://github.blog/changelog/2023-04-27-npm-provenance-public-beta/
