# VT Code Native Installers - Implementation Complete

## Summary

Implemented native installers and comprehensive documentation for VT Code across macOS, Linux, and Windows.

## What Was Done

### 1. Native Installers (Simplified)

#### Shell Script (`scripts/install.sh`) - 113 lines
- Auto-detects: OS (macOS/Linux), Architecture (Intel/ARM64/ARMv7)
- Downloads latest release from GitHub
- Intelligent path selection: `/usr/local/bin` → `/opt/local/bin` → `~/.local/bin`
- Verifies installation before completing
- Usage: `curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash`

#### PowerShell Script (`scripts/install.ps1`) - 170 lines
- Auto-detects: Windows architecture
- Smart directory selection: Program Files → LocalAppData
- Auto-configures PATH environment variable
- Stops any running processes gracefully
- Usage: `irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex`

#### Homebrew Formula (`homebrew/vtcode.rb`)
- Supports macOS and Linux
- Auto platform detection via Homebrew
- Usage: `brew install vtcode`

### 2. Documentation (Hierarchical & Non-Redundant)

#### Installation Guide (`docs/installation/README.md`) - Single Source of Truth
- Quick start commands for all 5 methods
- Installation method comparison table
- After-installation guide
- Troubleshooting section
- Uninstall instructions
- Installation paths and PATH configuration

#### Native Installers Technical Guide (`docs/installation/NATIVE_INSTALLERS.md`)
- Deep dive into each installer
- Platform detection logic
- Error handling strategies
- Debugging tips
- Homebrew setup instructions
- Security features
- Known issues & solutions

#### Quick Reference (`docs/installation/QUICK_REFERENCE.md`)
- One-liner commands only
- API keys quick list
- Uninstall one-liners
- Links to full docs

#### Developer Guide (`docs/installation/DEVELOPERS.md`)
- For maintainers and contributors
- Platform detection explanation
- Release binary setup
- CI/CD workflow template
- Testing procedures
- Update procedures
- Troubleshooting for developers

### 3. README.md Updates

**Before:** Confusing "Quickstart" with scattered installation info
**After:** Clear "Installation" and "Usage" sections

- Installation section: Native installers first, package managers as alternatives
- Usage section: Concise API key + launch example
- Removed redundant provider list (links to guide instead)
- Updated docs index with installation links

## Installation Commands

```bash
# macOS & Linux
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash

# Windows (PowerShell)
irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex

# Homebrew
brew install vtcode

# Cargo
cargo install vtcode

# npm
npm install -g @vinhnx/vtcode
```

## Recent Fixes

### v0.43.2+ - Installer CDN Caching Fix
- Fixed stdout pollution in shell installer by redirecting all logging to stderr
- Resolves "No such file or directory" errors from log messages interfering with variable capture
- If installation fails, use GitHub API endpoint for always-fresh script:
  ```bash
  curl -fsSL "https://api.github.com/repos/vinhnx/vtcode/contents/scripts/install.sh?ref=main" | jq -r '.content' | base64 -d | bash
  ```

## File Structure

```
docs/installation/
├── README.md              (main guide, all methods)
├── NATIVE_INSTALLERS.md   (technical details)
├── QUICK_REFERENCE.md     (one-liners)
└── DEVELOPERS.md          (for maintainers)

scripts/
├── install.sh             (macOS/Linux installer)
└── install.ps1            (Windows installer)

homebrew/
└── vtcode.rb              (Homebrew formula)

README.md                   (updated)
```

## Key Improvements

✓ **Simplicity:** Reduced scripts from 400+ lines to ~280 total
✓ **No Redundancy:** Single source of truth for users
✓ **Clear Hierarchy:** README → Installation Guide → Details → Developer Docs
✓ **No Broken URLs:** All scripts use GitHub raw content URLs
✓ **Works Out of Box:** No custom domain setup required
✓ **Professional:** Clean code, clear docs, obvious structure

## Documentation Paths

### For Users
1. Want to install? → `docs/installation/README.md`
2. Need quick commands? → `docs/installation/QUICK_REFERENCE.md`
3. Want detailed info? → `docs/installation/NATIVE_INSTALLERS.md`

### For Developers
1. Need to maintain installers? → `docs/installation/DEVELOPERS.md`
2. Need technical details? → `docs/installation/NATIVE_INSTALLERS.md`

### For the README
- Installation section: Top of document, clear and concise
- Links to full guide in "See Installation Guide" note

## Verification Checklist

Before releasing:
- [ ] Test `install.sh` on macOS Intel
- [ ] Test `install.sh` on macOS ARM64
- [ ] Test `install.sh` on Linux x86_64
- [ ] Test `install.sh` on Linux ARM64
- [ ] Test `install.ps1` on Windows 10/11
- [ ] Test `brew install vtcode`
- [ ] Test `cargo install vtcode`
- [ ] Test `npm install -g @vinhnx/vtcode`
- [ ] Verify all doc links work
- [ ] Check README displays correctly on GitHub

## Next Steps

1. **Create GitHub Release**
   - Build binaries for all platforms
   - Upload to releases with correct names
   - Copy SHA256 hashes

2. **Update Homebrew Formula**
   - Add correct SHAs to `homebrew/vtcode.rb`
   - Create tap at `github.com/vinhnx/homebrew-vtcode` (optional)

3. **Test Installers**
   - Use checklist above
   - Test on real systems or CI/CD

4. **Update GitHub Actions**
   - Ensure CI/CD creates binaries for all platforms
   - Auto-upload to releases

5. **Monitor & Maintain**
   - Watch GitHub Issues for installation problems
   - Keep documentation in sync with releases
   - Update installers only if necessary

## Features

✓ Platform auto-detection (OS and CPU architecture)
✓ Latest version fetching (no hardcoded versions)
✓ Smart installation paths with fallbacks
✓ Automatic PATH configuration
✓ Post-installation verification
✓ Clear error messages and guidance
✓ Works offline after download (except version check)
✓ No admin required (unless choosing Program Files on Windows)
✓ Clean uninstall (no config files left behind)

## Security

All installers:
- Use HTTPS only
- Download from official GitHub releases
- Verify binary exists after extraction
- Display all operations clearly
- No hidden execution
- Clean up temporary files
- Exit with proper error codes

## Support

- **Issues:** https://github.com/vinhnx/vtcode/issues
- **Docs:** https://github.com/vinhnx/vtcode/tree/main/docs
- **Installation Help:** https://github.com/vinhnx/vtcode/docs/installation
