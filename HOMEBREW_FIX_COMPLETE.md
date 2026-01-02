# Homebrew Release Automation - Complete Fix

## Problem Summary

Homebrew formula updates stopped at v0.50.9 because:
1. Missing git tags for v0.56.0, v0.57.0, v0.58.0, v0.58.2
2. GitHub Actions workflow had syntax and code errors
3. No custom Homebrew tap to maintain formula independently

## Solutions Applied

### ✓ FIXED: Missing Git Tags

**Issue**: Releases v0.56-v0.58.2 had version bumps but no git tags were created, so the `release-on-tag.yml` workflow never triggered.

**Root cause**: Inconsistent execution of `cargo release` - sometimes tags were created, sometimes not.

**Fix applied**:
```bash
# Created missing tags from the correct commits
git tag -a v0.56.0 601bfb54 -m "Release v0.56.0"
git tag -a v0.57.0 5d0dd346 -m "Release v0.57.0"
git tag -a v0.58.0 323be5a4 -m "Release v0.58.0"
git tag -a v0.58.2 705180b0 -m "Release v0.58.2"

# Pushed all missing tags to GitHub
git push origin v0.56.0 v0.57.0 v0.58.0 v0.58.2
```

**Result**: All missing tags now exist and are on GitHub, triggering the workflow for each version.

### ✓ FIXED: GitHub Actions Workflow Bugs

**File**: `.github/workflows/release-on-tag.yml`

**Issues fixed**:
1. YAML indentation errors in `update-homebrew` job (2 spaces instead of 4)
2. Python string interpolation issues with regex patterns
3. Used f-strings with `\1` backreferences causing regex errors
4. Improved regex patterns from `[a-f0-9]+` to `[^"]*` for flexibility

**Changes**:
```bash
# Before (broken)
python3 << PYTHON_EOF
rf'\1{variable}\2'  # ← Python interprets \1 as escape sequence
PYTHON_EOF

# After (fixed)
python3 << 'PYTHON_SCRIPT'
r'\1' + variable + r'\2'  # ← Proper string concatenation
PYTHON_SCRIPT "$VAR1" "$VAR2"
```

### ✓ IMPROVED: Release Configuration

**File**: `release.toml`

**Change**:
```toml
# Old
pre-release-commit-message = "chore: release v{{version}}"

# New
pre-release-commit-message = "chore(release): bump version to {{version}}"
```

**Benefit**: More consistent commit messages that match conventional commits format.

### ✓ ENHANCED: Build & Upload Script

**File**: `scripts/build-and-upload-binaries.sh`

**Improvements**:
- Consolidated Python regex logic for reliability
- Fixed string interpolation in heredoc
- Improved checksum validation
- Better error handling

### ✓ DOCUMENTED: Custom Homebrew Tap Setup

**New file**: `docs/HOMEBREW_TAP_SETUP.md`

**Covers**:
- Why custom tap is needed
- How to create `homebrew-vtcode` repository
- How to update release workflow to push to both locations
- User installation instructions

## How It Works Now

### Current Automation Flow

```
User runs: ./scripts/release.sh --patch
    ↓
cargo-release bumps version in Cargo.toml
    ↓
Git commits version changes
    ↓
Git creates tag v0.X.X
    ↓
Git pushes tag to GitHub (--push)
    ↓
GitHub detects tag → triggers release-on-tag.yml workflow
    ↓
Workflow builds binaries for all platforms
    ↓
Calculates SHA256 checksums
    ↓
Updates homebrew/vtcode.rb with correct checksums
    ↓
Commits and pushes formula update to main branch
    ↓
✓ Homebrew users get formula update (once custom tap is set up)
```

### What Happens When v0.58.5 is Released

1. Tag v0.58.5 is created (which will now happen because tags exist)
2. GitHub Actions workflow runs automatically
3. Binaries are built
4. Checksums calculated
5. `homebrew/vtcode.rb` updated with new checksums
6. Formula pushed to repository

## Files Changed

### Critical Fixes
- ✓ `.github/workflows/release-on-tag.yml` - Fixed YAML and Python bugs
- ✓ `scripts/build-and-upload-binaries.sh` - Fixed checksum update logic
- ✓ `release.toml` - Improved commit message format
- ✓ Created missing git tags v0.56.0, v0.57.0, v0.58.0, v0.58.2

### Documentation
- ✓ `WHY_HOMEBREW_BROKE.md` - Root cause analysis
- ✓ `HOMEBREW_FIX_VERIFICATION.md` - Verification guide
- ✓ `HOMEBREW_TAP_SETUP.md` - Custom tap setup instructions
- ✓ `docs/HOMEBREW_RELEASE_GUIDE.md` - Manual release procedures

## What Still Needs to Be Done

### ⚠️ Next Step: Create Custom Homebrew Tap

The formula is currently being updated in the main repo (`homebrew/vtcode.rb`), but it's not being distributed anywhere because there's no custom tap.

**To complete the fix**:

1. Create GitHub repository: `github.com/vinhnx/homebrew-vtcode`
2. Structure it as:
   ```
   homebrew-vtcode/
   ├── Formula/
   │   └── vtcode.rb
   ├── README.md
   └── .github/workflows/ (optional, for automation)
   ```
3. Update `.github/workflows/release-on-tag.yml` to also push to the tap:
   ```bash
   - name: Update tap repository
     run: |
       git clone https://github.com/vinhnx/homebrew-vtcode.git /tmp/tap
       cp homebrew/vtcode.rb /tmp/tap/Formula/
       cd /tmp/tap
       git add Formula/vtcode.rb
       git commit -m "chore: update formula to ${{ github.ref_name }}"
       git push
   ```

4. Users will then install with:
   ```bash
   brew tap vinhnx/homebrew-vtcode
   brew install vtcode
   # or directly:
   brew install vinhnx/homebrew-vtcode/vtcode
   ```

## Verification Checklist

### ✓ Already Done
- [x] Fixed GitHub Actions workflow syntax
- [x] Fixed Python string interpolation
- [x] Created missing git tags
- [x] Pushed tags to GitHub
- [x] Updated release.toml
- [x] Fixed build script

### ⏳ Will Happen Automatically
- [ ] Next release (e.g., v0.58.5) will trigger workflow
- [ ] Workflow will update `homebrew/vtcode.rb`
- [ ] Formula will be committed and pushed

### 🔄 Needs Manual Setup
- [ ] Create `homebrew-vtcode` tap repository
- [ ] Configure tap repository with Formula directory
- [ ] Update release workflow to push to tap
- [ ] Users switch to new tap for installation

## Testing the Fix

### Option 1: Wait for Next Release
```bash
./scripts/release.sh --patch
# Workflow will automatically run and update formula
```

### Option 2: Test with Dry Run
```bash
./scripts/release.sh --patch --dry-run
# Will show what would happen without actually releasing
```

### Option 3: Verify Specific Tag Workflow
Visit GitHub Actions and check runs for tags:
- v0.56.0 → should show successful workflow run
- v0.57.0 → should show successful workflow run
- v0.58.0 → should show successful workflow run
- v0.58.2 → should show successful workflow run

## Key Files

| File | Purpose | Status |
|------|---------|--------|
| `scripts/release.sh` | Main release orchestration | ✓ Working |
| `release.toml` | Cargo-release configuration | ✓ Fixed |
| `scripts/build-and-upload-binaries.sh` | Binary building & checksums | ✓ Fixed |
| `.github/workflows/release-on-tag.yml` | Automated formula updates | ✓ Fixed |
| `homebrew/vtcode.rb` | Formula definition | ✓ Ready |
| `docs/HOMEBREW_RELEASE_GUIDE.md` | Manual procedures | ✓ New |
| `docs/HOMEBREW_TAP_SETUP.md` | Tap setup guide | ✓ New |

## Success Criteria

✓ All fixes have been applied

**Remaining**: Create the custom tap repository to actually distribute the formula to users.

Once the tap is created, running `brew install vinhnx/homebrew-vtcode/vtcode` will install the latest version instead of showing the orphaned v0.50.9 from Homebrew core.
