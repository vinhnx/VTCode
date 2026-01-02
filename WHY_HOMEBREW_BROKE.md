# Why Homebrew Updates Stopped

## Timeline

### Phase 1: Manual Updates (v0.13 → v0.50.9)
- **Period**: Early releases through Dec 20, 2025
- **Method**: Manually running `scripts/release.sh` which updated `homebrew/vtcode.rb`
- **Last manual update**: Commit `7500c402` updated formula to v0.50.9
- **Status**: ✓ Working - Homebrew formula kept in sync

### Phase 2: Automation Attempt (Nov 3, 2025)
- **Date**: Nov 3, 2025
- **Change**: GitHub Actions workflow `release-on-tag.yml` was added with `update-homebrew` job
- **Goal**: Automatically update Homebrew formula when tags are pushed
- **But**: The workflow had bugs:
  - YAML indentation errors in `update-homebrew` job
  - Python string interpolation issues with regex patterns
  - These bugs would cause the workflow to fail silently or partially

### Phase 3: Silent Failure (Dec 21 → Jan 2, 2026)
- **What happened**: 
  - v0.56.0 released (Dec 21) - NO TAG CREATED
  - v0.57.0 released (Dec 22) - NO TAG CREATED
  - v0.58.0, v0.58.1, v0.58.2, v0.58.3 released - NO TAGS
  - Versions bumped in Cargo.toml but no git tags pushed

- **Why no tags?**: The `release.sh` script may have failed or been interrupted at the tag-pushing step

- **Result**: 
  - No `v0.56.0` tag → workflow never triggered
  - No `v0.57.0` tag → workflow never triggered
  - ... and so on
  - Homebrew formula stayed at v0.50.9

### Phase 4: Current (Jan 2, 2026)
- **Discovered issue**: Homebrew formula on formulae.brew.sh still shows v0.50.9
- **Root causes identified**:
  1. **Missing git tags** after v0.55.1 (Dec 29) - this is the PRIMARY cause
  2. **Broken GitHub Actions workflow** - even if tags existed, automation would fail
  3. **No custom Homebrew tap** - Homebrew is stuck with old orphaned formula in core

## The Three Issues

### Issue #1: Missing Git Tags (PRIMARY)
```
v0.50.9: No tag (manually updated in commit)
v0.55.1: ✓ Tag exists (Dec 29)
v0.56.0: ✗ NO TAG despite version bump
v0.57.0: ✗ NO TAG despite version bump
v0.58.0-0.58.3: ✗ NO TAGS despite version bumps
```

**Result**: Release workflow never triggered for v0.56+

### Issue #2: Broken GitHub Actions Workflow
Even if tags were created, the workflow would fail:
- YAML indentation errors
- Python regex string interpolation bugs
- Would silently fail or not update formula

**Status**: ✓ FIXED in this session

### Issue #3: No Custom Homebrew Tap
The current setup assumes pushing to Homebrew/core, which:
- Doesn't exist for VT Code (formula is orphaned)
- Requires Homebrew maintainer approval
- Is slow to update

**Status**: ✓ Documented in `docs/HOMEBREW_TAP_SETUP.md`

## What Was Actually Working

In mid-December when "it was working":
1. Manual updates to `homebrew/vtcode.rb` in git commits
2. Someone (you?) was manually running release and committing the updated formula
3. The formula got updated to v0.50.9 and pushed
4. This appeared on Homebrew, making it seem like automation worked

But it was **manual**, not automated.

## What's Fixed Now

### ✓ Workflow Syntax (Fixed)
- YAML indentation corrected
- Python string interpolation fixed
- Regex patterns improved

### ✗ Missing Git Tags (NOT FIXED)
The release script or process is not creating tags for v0.56+. This needs investigation:
- Does `./scripts/release.sh --patch` create tags?
- Are they being pushed to GitHub?
- Is the CI/CD triggering the workflow?

### ✓ Homebrew Tap Guide (Added)
`docs/HOMEBREW_TAP_SETUP.md` explains how to set up a custom tap

## Next Steps to Complete the Fix

### 1. Verify Tag Creation (URGENT)
```bash
# Test if release script creates tags
./scripts/release.sh --patch --dry-run 2>&1 | grep -i tag

# Verify git tags exist
git tag -l "v0.*" | tail -5
```

### 2. Create Custom Homebrew Tap
```bash
# Create github.com/vinhnx/homebrew-vtcode with Formula/vtcode.rb
# Update release workflow to push to that tap as well
```

### 3. Fix Any Remaining Issues
- Test the next release with `--dry-run`
- Monitor GitHub Actions logs
- Verify Homebrew formula updates

## How It Should Work

Once both issues are fixed:

```
./scripts/release.sh --patch
  ↓
Git tag v0.X.X created
  ↓
Push to GitHub triggers release-on-tag.yml
  ↓
Build binaries
  ↓
Calculate checksums
  ↓
Update homebrew/vtcode.rb AND push to homebrew-vtcode tap
  ↓
Homebrew users get updates automatically
```

## Files Involved

- `scripts/release.sh` - Creates tags and bumps version
- `.github/workflows/release-on-tag.yml` - Triggered by tag push (FIXED)
- `homebrew/vtcode.rb` - Formula file (WORKING)
- `scripts/build-and-upload-binaries.sh` - Handles checksums (FIXED)
- `docs/HOMEBREW_TAP_SETUP.md` - Setup guide (NEW)
