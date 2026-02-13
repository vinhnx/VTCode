# Homebrew Release Guide

## Current Status

**Issue**: Homebrew releases stopped at v0.55.1, but VT Code is now at v0.58.3. Versions v0.56.0 through v0.58.3 are available on GitHub but not on Homebrew.

**Root Cause**: The release workflow doesn't automatically update the Homebrew formula with correct checksums. The formula requires SHA256 checksums for the compiled binaries, which must be generated and updated during the release process.

## Fix Applied

### 1. Updated Homebrew Formula (`homebrew/vtcode.rb`)
- Version updated from 0.50.9 to 0.58.3
- Added placeholder checksums that will be automatically updated during the next release
- Improved formula structure for better maintainability

### 2. Enhanced Build Script (`scripts/build-and-upload-binaries.sh`)
- Added comprehensive error handling for missing checksums
- Improved `update_homebrew_formula()` function with validation
- Better logging of checksums being applied
- Safer sed/Python-based checksum replacement
- Proper git commit and push handling

## How the Release Process Works

### Workflow Diagram

```
release-main.yml triggered
    ↓
./scripts/release.sh
    ↓
cargo-release (version bump, tags, publish to crates.io)
    ↓
git push (commits and tags)
    ↓
release-on-tag.yml triggered by tag push
    ↓
build_binaries() - Compile for all platforms
    ↓
calculate_checksums() - Generate SHA256 for each binary
    ↓
upload_binaries() - Upload to GitHub Releases
    ↓
update_homebrew_formula() - Update formula with new checksums
    ↓
Homebrew picks up the change (syncs periodically)
```

### Step-by-Step Release Procedure

#### 1. Trigger Release

Go to GitHub Actions → "Create Main VT Code Release" → Run workflow

Options:
- `release_type`: patch/minor/major
- `dry_run`: true (test first!)
- Other flags as needed

#### 2. Verify Tag Creation

Release script will:
- Bump version in Cargo.toml
- Run `cargo-release` (handles crate publishing)
- Create git tag (e.g., `0.58.3`)
- Push to remote

#### 3. GitHub Actions Run

The `release-on-tag.yml` workflow activates:

```yaml
on:
  push:
    tags:
      - "[0-9]*"
```

This workflow:
1. Builds binaries for all platforms (macOS Intel/ARM, Linux x64/ARM64)
2. Creates SHA256 checksums for each binary
3. Uploads binaries to the GitHub Release
4. Updates the Homebrew formula
5. Commits and pushes the updated formula

## Troubleshooting

### Problem: Homebrew Formula Not Updating

**Check**: Are binaries uploaded to GitHub Release?
```bash
# Check if release has assets
gh release view 0.58.3 --json assets --jq '.assets | length'
```

**Solution**: 
1. Ensure `release-on-tag.yml` runs successfully
2. Check GitHub Actions logs for the release tag
3. Verify binaries exist in `dist/` directory
4. Check that `.sha256` files are generated

### Problem: Wrong Checksums in Formula

**Cause**: Checksum files weren't read correctly during update

**Solution**:
```bash
# Manually verify checksums
cd dist/
shasum -a 256 vtcode-0.58.3-aarch64-apple-darwin.tar.gz
shasum -a 256 vtcode-0.58.3-x86_64-apple-darwin.tar.gz

# Manually update formula if needed
# Edit homebrew/vtcode.rb and replace sha256 values
```

### Problem: Homebrew Formula Commit Failed

**Cause**: Git push failed or no changes to commit

**Solution**:
```bash
# Check formula was modified
git diff homebrew/vtcode.rb

# If needed, manually push
cd homebrew && git add vtcode.rb
git commit -m "chore: update homebrew formula to 0.58.3"
git push origin main
```

## Manual Update Process

If the automated process fails, manually update Homebrew:

```bash
# 1. Get latest version
VERSION=$(grep '^version = ' Cargo.toml | sed 's/.*"\([^"]*\)".*/\1/')

# 2. Get checksums from GitHub Release
gh release download $VERSION --dir dist --pattern "*.sha256"

# 3. Read checksums
X86_64_SHA=$(cat dist/vtcode-$VERSION-x86_64-apple-darwin.sha256)
ARM64_SHA=$(cat dist/vtcode-$VERSION-aarch64-apple-darwin.sha256)

# 4. Update formula
cat > homebrew/vtcode.rb << EOF
class Vtcode < Formula
  desc "Rust-based terminal coding agent with semantic code intelligence"
  homepage "https://github.com/vinhnx/vtcode"
  license "MIT"
  version "$VERSION"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/vinhnx/vtcode/releases/download/#{version}/vtcode-#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "$ARM64_SHA"
    else
      url "https://github.com/vinhnx/vtcode/releases/download/#{version}/vtcode-#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "$X86_64_SHA"
    end
  end

  # ... rest of formula ...
EOF

# 5. Commit and push
git add homebrew/vtcode.rb
git commit -m "chore: update homebrew formula to $VERSION"
git push origin main
```

## Homebrew Formula Structure

The formula uses Ruby's Homebrew DSL:

```ruby
class Vtcode < Formula
  version "0.58.3"

  on_macos do
    if Hardware::CPU.arm?          # Apple Silicon (M1/M2/M3)
      url "...aarch64-apple-darwin.tar.gz"
      sha256 "..."
    else                            # Intel Mac
      url "...x86_64-apple-darwin.tar.gz"
      sha256 "..."
    end
  end

  on_linux do
    if Hardware::CPU.arm? && Hardware::CPU.is_64_bit?  # ARM64 Linux
      url "...aarch64-unknown-linux-gnu.tar.gz"
      sha256 "..."
    else                                                 # x86_64 Linux
      url "...x86_64-unknown-linux-gnu.tar.gz"
      sha256 "..."
    end
  end
end
```

## Integration with Homebrew Tap

The formula is located in the main repository (`homebrew/vtcode.rb`), not a separate tap.

**Installation**:
```bash
brew install vinhnx/tap/vtcode
```

**Why a tap?** Custom Homebrew packages need to be in a tap (a special repository structure) rather than the main Homebrew core.

See: https://formulae.brew.sh/formula/vtcode

## Next Steps

1. Monitor the next release to ensure Homebrew is automatically updated
2. Test Homebrew installation: `brew install vinhnx/tap/vtcode`
3. Verify version matches: `vtcode --version`
4. If issues persist, improve error handling in build script

## Related Files

- `scripts/release.sh` - Main release orchestration
- `scripts/build-and-upload-binaries.sh` - Binary build and Homebrew update
- `.github/workflows/release-main.yml` - Triggers manual release
- `.github/workflows/release-on-tag.yml` - Automates binary builds on tag
- `homebrew/vtcode.rb` - Homebrew formula
