# VT Code - npm Package

This directory contains the npm package configuration for publishing VT Code to GitHub Packages.

## Package Details

- **Package Name**: `@vinhnx/vtcode`
- **Registry**: GitHub Packages
- **Repository**: https://github.com/vinhnx/vtcode

## Installation

The package is published to GitHub Packages and can be installed with:

```bash
npm install @vinhnx/vtcode
```

Or as a global CLI tool:

```bash
npm install -g @vinhnx/vtcode
```

## Features

- **Platform Support**: macOS, Linux, Windows
- **Architecture Support**: x64, arm64
- **Postinstall**: Automatically downloads the correct binary for your platform
- **CLI Integration**: Available as `vtcode` command after installation

## How It Works

1. **Installation**: When you install the package, the `postinstall` script runs
2. **Binary Download**: Downloads the appropriate platform-specific binary from GitHub releases
3. **Binary Setup**: Extracts and configures the binary for your system
4. **CLI Ready**: The `vtcode` command is available immediately

## Publishing

The release script (`scripts/release.sh`) automatically:

1. Updates the npm package version
2. Publishes to GitHub Packages during the release process
3. Uploads with proper authentication

For manual publishing, see `scripts/publish-to-github.js`.

## Configuration

### GitHub Packages Authentication

Create a `.npmrc` file in your home directory:

```
//npm.pkg.github.com/:_authToken=YOUR_GITHUB_TOKEN
@vinhnx:registry=https://npm.pkg.github.com
```

Or use the environment variable:

```bash
export NODE_AUTH_TOKEN=YOUR_GITHUB_TOKEN
```

See `.npmrc.example` for detailed setup instructions.

## Files

- `package.json` - Package metadata and version
- `index.js` - Entry point and CLI wrapper
- `scripts/postinstall.js` - Downloads and sets up the binary
- `scripts/preuninstall.js` - Cleanup on uninstall
- `scripts/publish-to-github.js` - Manual publish script
- `.npmrc.example` - Authentication configuration template

## Troubleshooting

### Binary Download Fails

If the postinstall script fails to download the binary:

1. Check your internet connection
2. Verify the release exists: https://github.com/vinhnx/vtcode/releases
3. Try manual installation:
   ```bash
   cargo install vtcode
   ```

### Permission Issues

On macOS/Linux, ensure the binary has execute permissions:

```bash
chmod +x ./bin/vtcode-*
```

### Windows Issues

For Windows, ensure you have:
- PowerShell 5.0+ (for Expand-Archive)
- Or `7-Zip` installed

## Development

When updating the release script, ensure:

1. Version in `package.json` matches the main release version
2. `.npmrc` is configured for GitHub Packages
3. `GITHUB_TOKEN` environment variable is set during CI/CD
4. Test postinstall script locally before release

## Support

For issues or questions:

- GitHub Issues: https://github.com/vinhnx/vtcode/issues
- Documentation: https://github.com/vinhnx/vtcode/docs
