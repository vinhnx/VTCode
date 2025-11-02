# VTCode Extension Release Script

Automated release script for the VTCode VSCode extension that handles version bumping, building, packaging, and publishing.

## Quick Start

```bash
# Patch release (0.1.1 -> 0.1.2)
./release.sh patch

# Minor release (0.1.1 -> 0.2.0)
./release.sh minor

# Major release (0.1.1 -> 1.0.0)
./release.sh major
```

## What It Does

1. ✅ Checks all required dependencies (node, npm, git, jq, vsce, ovsx)
2. 📦 Bumps version in package.json
3. 📝 Updates CHANGELOG.md with new version and date
4. 🔨 Builds the extension (npm run bundle)
5. 📦 Packages the extension (.vsix file)
6. 💾 Commits changes to git
7. 🏷️ Creates git tag with format: `vscode-v{version}`
8. ⬆️ Pushes to GitHub (with confirmation)
9. 🚀 Publishes to VSCode Marketplace (with confirmation)
10. 🌐 Publishes to Open VSX Registry (with confirmation)
11. 🧹 Cleans up old .vsix files

## Tag Naming Convention

The extension uses **`vscode-v{version}`** format to avoid conflicts with the main VTCode binary:

-   **Main VTCode CLI**: `v0.39.0`, `v0.39.1`, `v0.39.2`
-   **VSCode Extension**: `vscode-v0.1.0`, `vscode-v0.1.1`, `vscode-v0.1.2`

## Prerequisites

### Required Tools

-   `node` and `npm`
-   `git`
-   `jq` (JSON processor)
-   `@vscode/vsce` (installed automatically if missing)
-   `ovsx` (installed automatically if missing)

### Publishing Credentials

**VSCode Marketplace:**

-   Personal Access Token from https://marketplace.visualstudio.com/manage
-   Login with: `vsce login nguyenxuanvinh`

**Open VSX Registry:**

-   Account at https://open-vsx.org/
-   Personal Access Token (requested during publish)

## Interactive Prompts

The script will ask for confirmation before:

-   Pushing to GitHub
-   Publishing to VSCode Marketplace
-   Publishing to Open VSX Registry

You can skip any of these steps if needed.

## Manual Override

If you need to perform any step manually, you can still use the individual commands:

```bash
# Build only
npm run bundle

# Package only
npm run package

# Publish to VSCode Marketplace only
vsce publish

# Publish to Open VSX only
ovsx publish vtcode-companion-{version}.vsix
```

## Troubleshooting

**Missing jq:**

```bash
# macOS
brew install jq

# Ubuntu/Debian
sudo apt-get install jq
```

**Permission denied:**

```bash
chmod +x release.sh
```

**Tag already exists:**
The script will automatically delete and recreate local tags if they exist.

## Files Modified

-   `package.json` - Version number
-   `CHANGELOG.md` - New version entry
-   Git commits and tags created

## Output

After a successful release:

-   Local `.vsix` file: `vtcode-companion-{version}.vsix`
-   Git tag: `vscode-v{version}`
-   Marketplace: https://marketplace.visualstudio.com/items?itemName=nguyenxuanvinh.vtcode-companion
-   Open VSX: https://open-vsx.org/extension/nguyenxuanvinh/vtcode-companion
