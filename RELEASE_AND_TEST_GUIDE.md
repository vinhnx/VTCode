# Release v0.58.6 and Native Installer Test Guide

**Status**: ✅ Release v0.58.6 created and pushed to GitHub  
**Date**: January 2, 2026  
**What Happened**: Full release workflow executed

## What Just Happened

```bash
./scripts/release.sh --patch
```

This command performed the following steps automatically:

✅ **Version Bumped**: 0.58.5 → 0.58.6  
✅ **All Crates Published**: 14 crates published to crates.io  
✅ **Changelog Generated**: `CHANGELOG.md` updated with commit history  
✅ **Git Tag Created**: `v0.58.6` created and pushed  
✅ **All Commits Pushed**: Changes and tags pushed to origin/main  

## GitHub Actions Workflow Status

The following workflows have been automatically triggered:

### 1. **Release Workflow** (`.github/workflows/release.yml`)
- **Trigger**: Tag push (`v0.58.6`)
- **What it does**:
  - Generates changelog using `changelogithub`
  - Creates GitHub Release
  - Publishes npm package
- **Status**: Running (check GitHub Actions)
- **Expected Duration**: 2-5 minutes

### 2. **Build and Release Binaries** (`.github/workflows/build-release.yml`)
- **Trigger**: GitHub Release published
- **What it does**:
  - Builds binaries for all platforms:
    - macOS (Intel x86_64)
    - macOS (Apple Silicon aarch64)
    - Linux (x86_64)
    - Windows (x86_64 MSVC)
  - Uploads binaries to GitHub Release
- **Status**: Will trigger after Release workflow completes
- **Expected Duration**: 10-15 minutes (per platform)

### 3. **Native Installer Checksum** (`.github/workflows/native-installer.yml`)
- **Trigger**: GitHub Release assets uploaded
- **What it does**:
  - Downloads all binary assets
  - Generates SHA256 checksums
  - Creates `checksums.txt`
  - Uploads checksums to release
- **Status**: Will trigger after binaries are uploaded
- **Expected Duration**: 1-2 minutes

## Timeline

```
NOW (v0.58.6 tag pushed to GitHub)
  ↓
Release Workflow starts (2-5 min)
  ├─ Generate changelog
  ├─ Create GitHub Release
  └─ Release published (triggers next workflow)
  ↓
Build Workflow starts (5-15 min per platform)
  ├─ Ubuntu: Build x86_64-unknown-linux-gnu
  ├─ macOS: Build x86_64-apple-darwin
  ├─ macOS: Build aarch64-apple-darwin
  ├─ Windows: Build x86_64-pc-windows-msvc
  └─ Upload all binaries to release
  ↓
Native Installer Workflow starts (1-2 min)
  ├─ Download all assets
  ├─ Generate checksums.txt
  └─ Upload checksums to release
  ↓
✅ COMPLETE - Installer ready to use
```

**Total Expected Time**: 15-30 minutes from now

## Monitoring Progress

### Check GitHub Actions (Recommended)
```
https://github.com/vinhnx/vtcode/actions
```

Look for:
1. "Release" workflow - should be running/complete
2. "Build and Release Binaries" workflow - will start after Release completes
3. "Native Installer - Generate Checksums" workflow - will start after binaries uploaded

### Check Release Page
```
https://github.com/vinhnx/vtcode/releases/tag/v0.58.6
```

Watch for:
- Release created with changelog
- Binaries appearing (one per platform)
- checksums.txt file appearing

### Check Release Assets
Once binaries are uploaded, you should see:
```
vtcode-v0.58.6-x86_64-unknown-linux-gnu.tar.gz
vtcode-v0.58.6-x86_64-apple-darwin.tar.gz
vtcode-v0.58.6-aarch64-apple-darwin.tar.gz
vtcode-v0.58.6-x86_64-pc-windows-msvc.zip
checksums.txt
```

## Testing the Installer

### Wait For Completion

Before testing, wait for all workflows to complete:
- ✅ Release workflow done
- ✅ Build workflow done (all 4 platforms)
- ✅ Checksum workflow done (checksums.txt uploaded)

### Test on macOS (Apple Silicon)

Once binaries are ready:

```bash
# Test the installer script
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash

# Verify installation
vtcode --version

# Should output: vtcode 0.58.6
```

### Test on Linux (if available)

```bash
# Test the installer script
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash

# Verify installation
vtcode --version
```

### Test on Windows (if available)

```powershell
# Test the installer script
irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex

# Verify installation
vtcode --version
```

### Test Checksum Verification

Once checksums.txt is available:

```bash
# Download checksums
cd /tmp
curl -fsSL https://github.com/vinhnx/vtcode/releases/download/v0.58.6/checksums.txt -o checksums.txt

# Download binary
curl -fsSL https://github.com/vinhnx/vtcode/releases/download/v0.58.6/vtcode-v0.58.6-aarch64-apple-darwin.tar.gz -o vtcode.tar.gz

# Verify checksum
sha256sum -c checksums.txt --ignore-missing

# Should output: vtcode-v0.58.6-aarch64-apple-darwin.tar.gz: OK
```

## Troubleshooting

### Build workflow stuck or failed?
- Check GitHub Actions logs: https://github.com/vinhnx/vtcode/actions
- Look for the "Build and Release Binaries" workflow
- Click on the failed job to see error messages

### Installer returns 404?
- The binaries might not be uploaded yet
- Wait for the "Build and Release Binaries" workflow to complete
- Check the release page for uploaded assets

### Checksum verification fails?
- Ensure checksums.txt has been uploaded
- Check that the binary filename matches exactly in checksums.txt
- Ensure you're using the correct platform binary

### Release page is empty?
- The Release workflow might still be running
- Wait 5 minutes and refresh
- Check GitHub Actions for errors

## What to Do Next

### After Workflows Complete (15-30 min)

1. **Verify Release Page**
   - Visit: https://github.com/vinhnx/vtcode/releases/tag/v0.58.6
   - Confirm all assets are present

2. **Test Installer** (optional, test one platform)
   ```bash
   curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash
   ```

3. **Verify Version**
   ```bash
   vtcode --version  # Should show: vtcode 0.58.6
   ```

4. **Update Documentation** (if needed)
   - README already points to native installer
   - Installation guide already updated
   - No further action needed

### Optional: Create Additional Releases

For future releases, simply run:

```bash
# Patch version (0.58.6 → 0.58.7)
./scripts/release.sh --patch

# Minor version (0.58.6 → 0.59.0)
./scripts/release.sh --minor

# Major version (0.58.6 → 1.0.0)
./scripts/release.sh --major
```

## Release Checklist

- [x] Version bumped to 0.58.6
- [x] All crates published to crates.io
- [x] Changelog updated
- [x] Git tag created (v0.58.6)
- [x] Changes pushed to origin/main
- [x] GitHub Actions workflows triggered
- [ ] Release workflow completed
- [ ] Build workflow completed (all 4 platforms)
- [ ] Checksum workflow completed
- [ ] Binaries verified on release page
- [ ] Installer tested and working
- [ ] Version check passes (vtcode --version shows 0.58.6)

## Commands for Future Reference

### Check GitHub Release
```bash
# View release info
gh release view v0.58.6

# Download release assets
gh release download v0.58.6 -D ~/Downloads
```

### Verify Installer on All Platforms
```bash
# macOS/Linux
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash

# Windows PowerShell
irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex
```

### Test Update Check
```bash
# Once auto-updater is integrated:
vtcode --check-for-updates
# or
vtcode update
```

## Key Files

- **Release script**: `./scripts/release.sh`
- **Installer script**: `./scripts/install.sh` (macOS/Linux)
- **Installer script**: `./scripts/install.ps1` (Windows)
- **Build workflow**: `.github/workflows/build-release.yml`
- **Release workflow**: `.github/workflows/release.yml`
- **Checksum workflow**: `.github/workflows/native-installer.yml`
- **Auto-updater module**: `src/updater.rs`
- **Documentation**: `docs/NATIVE_INSTALLER.md`

## Summary

✅ **Release v0.58.6 created and pushed**  
✅ **All crates.io publications complete**  
✅ **GitHub Actions workflows triggered**  

**Next Step**: Wait for GitHub Actions to complete (15-30 minutes), then test the installer.

**Status**: Waiting for GitHub Actions → Build Binaries → Generate Checksums → Ready for Production

---

**Last Updated**: January 2, 2026  
**Release**: v0.58.6  
**Installer**: Ready (once workflows complete)
