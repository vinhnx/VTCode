# Homebrew Release Fix - Verification Summary

## Overview

The Homebrew release automation has been fixed to reliably update the formula with SHA256 checksums for all platforms (macOS x86_64/ARM64 and Linux x86_64/ARM64).

## Issues Fixed

### 1. YAML Indentation in GitHub Actions Workflow
- **File**: `.github/workflows/release-on-tag.yml`
- **Issue**: The `update-homebrew` job had incorrect YAML indentation (2 spaces instead of 4), causing syntax errors
- **Fix**: Corrected indentation to match GitHub Actions workflow requirements

### 2. Python String Interpolation in Regex Replacements
- **Files**: 
  - `scripts/build-and-upload-binaries.sh`
  - `.github/workflows/release-on-tag.yml`
- **Issue**: The Python code was using f-strings with `\1` backreferences, which Python was interpreting as escape sequences, causing `re.PatternError: invalid group reference`
- **Fix**: 
  - Switched to `<<'PYTHON_SCRIPT'` (with quotes) to prevent bash variable interpolation
  - Used `%s` placeholders instead of bash variables
  - Concatenated regex group references properly: `r'\1' + variable + r'\2'` instead of `rf'\1{variable}\2'`

### 3. Regex Pattern for Checksum Matching
- **Issue**: The regex pattern `[a-f0-9]+` only matched hex characters, but some checksums might have other characters
- **Fix**: Changed to `[^"]*` to match any characters within quotes, making it more flexible

### 4. Removed Unnecessary sed Commands
- **Issue**: Multiple `sed` calls with different patterns added complexity and potential for failure
- **Fix**: Consolidated all updates into a single Python script for more reliable execution

## Current Implementation

### Local Release Script (`scripts/build-and-upload-binaries.sh`)

The `update_homebrew_formula()` function:
1. Validates all required checksum files exist
2. Reads checksums from files
3. Uses Python regex to update the formula atomically:
   - Updates version
   - Updates all SHA256 values (macOS and Linux, x86_64 and ARM64)
4. Commits and pushes the updated formula to `main` branch

### GitHub Actions Workflow (`release-on-tag.yml`)

The `update-homebrew` job:
1. Runs after binary build completes
2. Downloads all release assets
3. Calculates SHA256 checksums using `shasum -a 256`
4. Updates the Homebrew formula using identical Python logic
5. Commits and pushes the formula (continues on error to not block other jobs)

## Testing Performed

Verified the Python regex logic with a test script that:
- Created mock formula with old checksums
- Applied the update logic with new test checksums
- Confirmed all 5 replacements (version + 4 checksums) succeeded

**Result**: ✓ All replacements successful

## How the Fix Works on Next Release

When running `./scripts/release.sh --patch`:

1. Version is bumped in Cargo.toml
2. Git tag is created (e.g., `v0.58.4`)
3. GitHub Actions triggered by tag push
4. Binaries built for all platforms
5. **NEW**: Homebrew formula automatically updated:
   - Correct checksums calculated
   - Formula version set to new version
   - All platform-specific SHA256 values replaced
   - Changes committed and pushed to `main`

No manual intervention needed.

## Files Changed

- ✓ `.github/workflows/release-on-tag.yml` - Fixed YAML indentation and Python string handling
- ✓ `scripts/build-and-upload-binaries.sh` - Fixed Python string interpolation
- ✓ `homebrew/vtcode.rb` - Already updated to v0.58.3 as placeholder

## Verification Checklist

Before the next release, you can:

1. **Syntax check**: `ruby -c homebrew/vtcode.rb` 
2. **Dry-run test**: `./scripts/release.sh --patch --dry-run`
3. **Local formula update test**: Run the update logic with mock checksums (as verified above)

## Next Steps

1. Commit these fixes to `main`
2. On next release (`./scripts/release.sh --patch` or `--minor`), the workflow will:
   - Build binaries
   - Calculate checksums
   - Update Homebrew formula
   - Automatically commit and push
3. Monitor GitHub Actions logs to confirm success
4. Wait 24-48 hours for Homebrew to sync the updated formula
5. Verify on https://formulae.brew.sh/formula/vtcode

## References

- Homebrew formula location: `homebrew/vtcode.rb`
- Release script: `scripts/release.sh`
- Build and upload script: `scripts/build-and-upload-binaries.sh`
- GitHub Actions workflows: `.github/workflows/release*.yml`
- Documentation: `docs/HOMEBREW_RELEASE_GUIDE.md`
