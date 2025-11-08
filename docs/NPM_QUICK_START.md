# NPM Package Quick Start Guide

## Installation

Install VT Code from GitHub Packages npm registry:

```bash
npm install @vinhnx/vtcode
```

Global installation (recommended for CLI):

```bash
npm install -g @vinhnx/vtcode
```

## First Time Setup

### Step 1: Create GitHub Personal Access Token

1. Go to https://github.com/settings/tokens
2. Click "Generate new token"
3. Select scopes:
   - ✓ `write:packages`
   - ✓ `read:packages`
   - ✓ `repo`
4. Copy the token (you won't see it again!)

### Step 2: Configure npm

Choose ONE method:

#### Method A: Environment Variable (Recommended)
```bash
export GITHUB_TOKEN=<your_token>
export NODE_AUTH_TOKEN=$GITHUB_TOKEN
```

Add to your shell rc file (`.bashrc`, `.zshrc`, etc.):
```bash
echo 'export GITHUB_TOKEN=<your_token>' >> ~/.bashrc
echo 'export NODE_AUTH_TOKEN=$GITHUB_TOKEN' >> ~/.bashrc
source ~/.bashrc
```

#### Method B: npm Config
```bash
npm config set //npm.pkg.github.com/:_authToken <your_token>
npm config set @vinhnx:registry https://npm.pkg.github.com
```

#### Method C: .npmrc File
```bash
cat > ~/.npmrc << EOF
//npm.pkg.github.com/:_authToken=<your_token>
@vinhnx:registry=https://npm.pkg.github.com
EOF
```

### Step 3: Install the Package

```bash
npm install @vinhnx/vtcode
# or for global CLI
npm install -g @vinhnx/vtcode
```

The package will automatically download the correct binary for your system during installation.

## Usage

### As a Command-Line Tool

```bash
# If installed globally
vtcode ask "What does this code do?"
vtcode plan "Create a REST API"
vtcode implement --prompt "Add error handling"

# If installed locally
npx vtcode ask "Your question here"
```

### In Node.js/JavaScript

```javascript
const { spawn } = require('child_process');
const path = require('path');

// Get the vtcode binary path
const vtcodePath = path.join(__dirname, 'node_modules', '@vinhnx/vtcode', 'bin');

// Use it in your application
const child = spawn('vtcode', ['ask', 'Your query'], {
  stdio: 'inherit'
});

child.on('exit', (code) => {
  console.log(`vtcode exited with code ${code}`);
});
```

## Supported Platforms

| OS | Architecture | Status |
|---|---|---|
| macOS | arm64 | ✓ Supported |
| macOS | x64 | ✓ Supported |
| Linux | x64 | ✓ Supported |
| Linux | arm64 | ✓ Supported |
| Windows | x64 | ✓ Supported |

## Troubleshooting

### "npm ERR! 404 Not Found"

**Problem**: Can't find the package

**Solution**:
1. Ensure you configured npm correctly (see Step 2 above)
2. Verify your GitHub token is valid
3. Clear npm cache: `npm cache clean --force`
4. Try again: `npm install @vinhnx/vtcode`

### "Binary not found" after installation

**Problem**: Package installed but binary download failed

**Solution**:
```bash
# Reinstall to retry the download
npm uninstall @vinhnx/vtcode
npm install @vinhnx/vtcode

# Or manually download from releases
# https://github.com/vinhnx/vtcode/releases
```

### "Permission denied" error

**Problem**: Can't write to global npm directory

**Solution**:
```bash
# Option 1: Use a different directory for npm
mkdir ~/.npm-global
npm config set prefix '~/.npm-global'
export PATH=~/.npm-global/bin:$PATH

# Option 2: Fix npm permissions
sudo chown -R $(whoami) /usr/local/lib/node_modules
```

### Postinstall script failed

**Problem**: Binary download during installation failed

**Solution**:
1. Check your internet connection
2. Verify the release exists: https://github.com/vinhnx/vtcode/releases
3. Check available disk space
4. Try reinstalling with verbose output:
   ```bash
   npm install @vinhnx/vtcode --verbose
   ```

## Version Management

### Check Installed Version

```bash
# Via the CLI
vtcode --version
npx vtcode --version

# Via npm
npm list @vinhnx/vtcode
npm view @vinhnx/vtcode@latest
```

### Update to Latest

```bash
npm update @vinhnx/vtcode
# or
npm install @vinhnx/vtcode@latest
```

### Install Specific Version

```bash
npm install @vinhnx/vtcode@0.42.12
npm install @vinhnx/vtcode@0.42.x  # Latest 0.42.x
```

### View All Versions

```bash
npm view @vinhnx/vtcode versions
```

## CI/CD Integration

### GitHub Actions

```yaml
name: VTCode Integration

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      
      - uses: actions/setup-node@v3
        with:
          node-version: lts/*
          registry-url: https://npm.pkg.github.com
      
      - run: npm install @vinhnx/vtcode
        env:
          NODE_AUTH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
      
      - run: npx vtcode ask "Analyze this code"
```

### GitLab CI

```yaml
test:
  image: node:18
  script:
    - npm install @vinhnx/vtcode
    - npx vtcode --version
  only:
    - main
  variables:
    NODE_AUTH_TOKEN: $CI_JOB_TOKEN
```

### Local Development

```bash
# Install from GitHub Packages during development
npm install @vinhnx/vtcode

# Use in scripts
npx vtcode ask "Your query"

# Or add to package.json scripts
{
  "scripts": {
    "analyze": "vtcode ask 'Analyze this code'"
  }
}
```

## Uninstall

### Global Uninstall

```bash
npm uninstall -g @vinhnx/vtcode
```

The preuninstall script will automatically clean up downloaded binaries.

### Local Uninstall

```bash
npm uninstall @vinhnx/vtcode
```

## Package Contents

The npm package includes:

- **index.js**: Entry point that finds and executes the binary
- **bin/**: Platform-specific binaries (downloaded on install)
- **scripts/postinstall.js**: Auto-downloads correct binary for your OS
- **scripts/preuninstall.js**: Cleans up binaries on uninstall
- **scripts/publish-to-github.js**: Manual publishing script

## Development

### For Package Maintainers

See `npm/PUBLISHING.md` for detailed publishing documentation.

### For Contributors

To test changes to the npm package:

```bash
# Link for local testing
cd npm
npm link
vtcode --version

# Unlink when done
npm unlink
```

## Additional Resources

- **Package on GitHub**: https://github.com/vinhnx/vtcode/pkgs/npm/vtcode
- **GitHub Issues**: https://github.com/vinhnx/vtcode/issues
- **Documentation**: https://github.com/vinhnx/vtcode#documentation
- **Main Repository**: https://github.com/vinhnx/vtcode

## Getting Help

### Check Status

```bash
# Verify package is installed
npm list @vinhnx/vtcode

# Check if binary is available
which vtcode
npx vtcode --version

# View package info
npm info @vinhnx/vtcode
```

### Report Issues

1. Check existing issues: https://github.com/vinhnx/vtcode/issues
2. Provide system info:
   ```bash
   node --version
   npm --version
   uname -a  # or 'systeminfo' on Windows
   ```
3. Attach installation logs:
   ```bash
   npm install @vinhnx/vtcode --verbose 2>&1 | tee install.log
   ```

## Next Steps

1. Configure your GitHub token
2. Install the package
3. Test with `vtcode --version`
4. Check out the documentation: https://github.com/vinhnx/vtcode

Happy coding with VT Code!
