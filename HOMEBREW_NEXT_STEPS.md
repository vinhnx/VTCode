# Homebrew Fix - Next Steps

## What's Been Fixed ✓

- [x] Created missing git tags (v0.56.0, v0.57.0, v0.58.0, v0.58.2)
- [x] Pushed tags to GitHub
- [x] Fixed `.github/workflows/release-on-tag.yml` workflow
- [x] Fixed `scripts/build-and-upload-binaries.sh` script
- [x] Updated `release.toml` configuration
- [x] Created comprehensive documentation

## What Happens Next

When you run the next release:

```bash
./scripts/release.sh --patch
```

The automation will:
1. ✓ Create a git tag automatically
2. ✓ Push the tag to GitHub
3. ✓ Trigger `release-on-tag.yml` workflow automatically
4. ✓ Build binaries for all platforms
5. ✓ Calculate SHA256 checksums
6. ✓ Update `homebrew/vtcode.rb` with new checksums
7. ✓ Commit and push the formula to main branch

## Remaining Task: Create Custom Homebrew Tap

The formula updates are working, but there's **no distribution channel** yet. Users still see the orphaned v0.50.9 on https://formulae.brew.sh/formula/vtcode.

### Quick Setup (20 minutes)

1. **Create the tap repository**:
   ```bash
   # Create new GitHub repo: github.com/vinhnx/homebrew-vtcode
   # Then clone and set up:
   mkdir -p homebrew-vtcode/Formula
   cd homebrew-vtcode
   git init
   ```

2. **Add the formula**:
   ```bash
   # Copy current formula to Formula/vtcode.rb
   cp ../vtcode/homebrew/vtcode.rb Formula/
   ```

3. **Create README**:
   ```bash
   cat > README.md << 'EOF'
   # Homebrew Tap for VT Code
   
   VT Code - Rust-based terminal coding agent with semantic code intelligence
   
   ## Installation
   
   ```bash
   brew tap vinhnx/homebrew-vtcode
   brew install vtcode
   ```
   
   Or install directly:
   
   ```bash
   brew install vinhnx/homebrew-vtcode/vtcode
   ```
   
   ## Update
   
   ```bash
   brew update
   brew upgrade vtcode
   ```
   EOF
   ```

4. **Push to GitHub**:
   ```bash
   git add .
   git commit -m "Initial commit: add vtcode formula"
   git remote add origin https://github.com/vinhnx/homebrew-vtcode.git
   git push -u origin main
   ```

5. **Update the release workflow** in `vtcode` repo:
   
   Add to `.github/workflows/release-on-tag.yml` after the homebrew update step:
   ```yaml
   - name: Push to homebrew tap repository
     run: |
       git clone https://github.com/vinhnx/homebrew-vtcode.git /tmp/homebrew-tap
       cp homebrew/vtcode.rb /tmp/homebrew-tap/Formula/vtcode.rb
       
       cd /tmp/homebrew-tap
       git config user.name "github-actions[bot]"
       git config user.email "github-actions[bot]@users.noreply.github.com"
       
       if ! git diff --quiet Formula/vtcode.rb; then
         git add Formula/vtcode.rb
         git commit -m "chore: update vtcode formula to ${{ github.ref_name }}"
         git push origin main
       fi
     continue-on-error: true
   ```

### Complete Setup (includes automation)

If you want to fully automate the tap:

1. Do the 5 steps above
2. Update the release workflow as shown
3. Create `.github/workflows/publish.yml` in the tap repo for auto-bottle building (optional)

## Testing

### Test the Fix Without Releasing

```bash
# Do a dry run
./scripts/release.sh --patch --dry-run

# Check that it would create a tag
git status
```

### Monitor the Next Real Release

1. Run: `./scripts/release.sh --patch` (or `--minor`, `--major`)
2. Visit GitHub Actions page
3. Check the `release-on-tag.yml` workflow run
4. Verify `homebrew/vtcode.rb` was updated with new checksums
5. Once tap is set up, verify tap repo also got the update

## Installation Instructions for Users

Once the tap is set up:

**Current (broken - shows v0.50.9)**:
```bash
brew install vtcode  # ✗ Gets old v0.50.9 from Homebrew core
```

**After tap is created (working)**:
```bash
# Option 1: Direct install (recommended)
brew install vinhnx/homebrew-vtcode/vtcode

# Option 2: Tap first
brew tap vinhnx/homebrew-vtcode
brew install vtcode

# Option 3: Full name
brew install vinhnx/homebrew-vtcode/vtcode
```

## Files to Know

| File | What it does |
|------|-------------|
| `scripts/release.sh` | Main entry point - run this to release |
| `release.toml` | Cargo-release config - controls tagging & commits |
| `.github/workflows/release-on-tag.yml` | Triggered by tags - updates formula |
| `homebrew/vtcode.rb` | Formula definition - updated automatically |
| `scripts/build-and-upload-binaries.sh` | Builds binaries & calculates checksums |

## Troubleshooting

### If workflow doesn't trigger after pushing a tag

Check GitHub Actions page:
1. Go to Actions tab
2. Look for `release-on-tag.yml` workflow
3. If no run appears, the workflow wasn't triggered
4. Check that the tag name matches `v*` pattern

### If formula doesn't update

1. Check workflow logs on GitHub Actions
2. Look for errors in the Python checksum update step
3. Verify that all `.sha256` files were created
4. Manually check `homebrew/vtcode.rb` was modified

### If users still get old version

They might have the old core formula cached:
```bash
# Clear Homebrew cache and re-tap
brew untap homebrew/core  # Remove old core formula (if installed)
brew tap vinhnx/homebrew-vtcode
brew install vtcode
```

## Timeline

- ✓ **Done**: Fixed all automation
- ⏳ **Next**: Create tap repo (20 min)
- ⏳ **Then**: Test with next release
- ✓ **Result**: Users get latest VT Code via Homebrew

## Questions?

See these files for more details:
- `HOMEBREW_FIX_COMPLETE.md` - What was fixed and how
- `WHY_HOMEBREW_BROKE.md` - Root cause analysis
- `docs/HOMEBREW_TAP_SETUP.md` - Detailed tap setup guide
- `docs/HOMEBREW_RELEASE_GUIDE.md` - Manual release procedures
