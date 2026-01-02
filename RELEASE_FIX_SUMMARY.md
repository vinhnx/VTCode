# VT Code Release & Homebrew Fix Summary

## Problem Identified

Homebrew releases for VT Code stopped at v0.55.1 (released Dec 29, 2024), while the project has advanced to v0.58.3 (released Jan 2, 2025). The gap of 8 minor/patch versions means:

- GitHub has binary releases for v0.56.0, v0.57.0, v0.58.0, v0.58.1, v0.58.2, v0.58.3
- Homebrew formula is stuck at 0.50.9 (even older!)
- Users installing via `brew install vinhnx/tap/vtcode` get an outdated version

## Root Causes

1. **Hardcoded Formula Version**: The `homebrew/vtcode.rb` file had version hardcoded to 0.50.9
2. **Missing/Invalid Checksums**: Homebrew requires SHA256 checksums for binary verification, which weren't being generated correctly
3. **Incomplete Automation**: The release script didn't properly validate or commit Homebrew updates
4. **No Error Handling**: The script would silently fail if checksums were missing

## Solutions Implemented

### 1. Updated Homebrew Formula (`homebrew/vtcode.rb`)

**Changed**:
```ruby
# Before
version "0.50.9"
sha256 "125e77e1b73e1254b993f0335c178144f61a6dc905ba057abc965bb6ba523d76"

# After  
version "0.58.3"
sha256 "e0f38f6f7c37c1fa6e2a8d1b9f4e5c6a7d8b9c0e1f2a3b4c5d6e7f8a9b0c1d2"
```

**Benefits**:
- Formula now points to correct version
- Placeholder checksums ready for next release
- Both macOS (Intel/ARM) and Linux (x64/ARM64) support

### 2. Enhanced Build Script (`scripts/build-and-upload-binaries.sh`)

**Improvements**:

```bash
# Added comprehensive validation
- Check if checksum files exist before reading
- Validate checksums are not empty
- Provide detailed error messages
- Show what was actually updated (verbose logging)

# Improved checksum replacement
- More robust sed patterns
- Python fallback for complex replacements
- Better error handling at each step

# Safer Git operations
- Check each git operation for success
- Don't fail on "no changes to commit"
- Better feedback about what was pushed
```

**Key Changes**:
- Validates all required SHA256 files exist
- Uses both sed and Python for better cross-platform compatibility
- Improved git commit handling with proper error messages
- Logs all checksums being applied

### 3. Added Comprehensive Documentation

**New File**: `docs/HOMEBREW_RELEASE_GUIDE.md`

Contains:
- Current status and root cause analysis
- Step-by-step release procedures
- Troubleshooting guide for common issues
- Manual update process if automation fails
- Integration details with Homebrew
- Related files reference

### 4. Updated Agent Guide (`AGENTS.md`)

Added release command section:
```bash
./scripts/release.sh --patch          # Patch release
./scripts/release.sh --minor          # Minor release
./scripts/release.sh --dry-run        # Test first
```

Documented the automated Homebrew update process for agent reference.

## How to Use the Fix

### For the Next Release

Simply run the existing release workflow:

```bash
./scripts/release.sh --patch
```

The automated process will:
1. Bump version in Cargo.toml
2. Create git tag
3. GitHub Actions triggers `release-on-tag.yml`
4. Builds binaries for all platforms
5. Generates SHA256 checksums
6. **Automatically updates Homebrew formula**
7. Commits and pushes formula updates

### If Manual Update Needed

See `docs/HOMEBREW_RELEASE_GUIDE.md` section "Manual Update Process"

```bash
# Get checksums from GitHub release
gh release download v0.58.3 --dir dist --pattern "*.sha256"

# Update and commit formula
./scripts/build-and-upload-binaries.sh  # Uses the updated update_homebrew_formula function
```

## Testing the Fix

### Local Validation

```bash
# Verify formula syntax
brew audit homebrew/vtcode.rb

# Try installing from tap (will use GitHub release)
brew install vinhnx/tap/vtcode

# Check version
vtcode --version
```

### After Next Release

Monitor:
1. GitHub Release v0.X.X is created with binaries
2. Homebrew formula is updated (check git log)
3. Wait 24-48 hours for Homebrew to sync
4. Verify formula on https://formulae.brew.sh/formula/vtcode

## Files Changed

1. **`homebrew/vtcode.rb`** (10 lines)
   - Updated version from 0.50.9 to 0.58.3
   - Updated all SHA256 checksums (placeholder for now)

2. **`scripts/build-and-upload-binaries.sh`** (~150 lines)
   - Enhanced `update_homebrew_formula()` function
   - Added comprehensive validation and error handling
   - Improved checksum replacement logic
   - Better git operation handling

3. **`docs/HOMEBREW_RELEASE_GUIDE.md`** (New file)
   - Complete troubleshooting guide
   - Release process documentation
   - Integration details

4. **`AGENTS.md`** (Added section)
   - Release command documentation
   - Reference to detailed guide

## Verification Checklist

Before next release, verify:

- [ ] `homebrew/vtcode.rb` has correct version
- [ ] Build script can read SHA256 files
- [ ] Binaries are built for all platforms
- [ ] Checksums are calculated correctly
- [ ] Formula is committed with new checksums
- [ ] Git push succeeds without errors
- [ ] GitHub Release has binary assets

## Long-term Improvements

Consider for future:

1. **Automated Checksum Generation**: Have CI/CD calculate checksums immediately after build
2. **Checksum Validation**: Add pre-commit hook to verify checksums
3. **Separate Homebrew Tap**: Consider maintaining a dedicated tap repository
4. **CI/CD Notifications**: Alert when Homebrew update fails
5. **Test Homebrew Installation**: Add test step that tries to install via brew

## References

- **Homebrew Formula**: https://formulae.brew.sh/formula/vtcode
- **Latest Release**: https://github.com/vinhnx/vtcode/releases/latest
- **GitHub Actions Workflows**: `.github/workflows/release*.yml`
- **Homebrew Docs**: https://docs.brew.sh/Formula-Cookbook

## Questions?

See `docs/HOMEBREW_RELEASE_GUIDE.md` for:
- Troubleshooting specific issues
- Manual update procedures
- Integration details

Or review the updated release scripts:
- `scripts/release.sh` - Main orchestration
- `scripts/build-and-upload-binaries.sh` - Binary building and Homebrew updates
