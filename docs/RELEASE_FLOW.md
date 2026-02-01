# VT Code Complete Release Flow

End-to-end guide for releasing VT Code with automated binary builds and registry updates.

## Quick Summary

```bash
# VT Code repo: Run release (includes macOS binaries)
cd /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode
./scripts/release.sh --patch

# Trigger GitHub Actions for Linux/Windows builds
./scripts/trigger-ci-release.sh v0.74.3

# Wait for CI to complete (~15 min)
gh run list --workflow=release.yml

# Registry repo: Update binaries and rebuild
cd /Users/vinhnguyenxuan/Developer/learn-by-doing/registry
./update-vtcode-binaries.sh v0.74.3
uv run --with jsonschema .github/workflows/build_registry.py
git add . && git commit -m "chore: update vtcode to v0.74.3" && git push
```

## Step-by-Step

### Step 1: Local Release (VT Code Repo)

```bash
cd /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode

# Dry-run first (safe)
./scripts/release.sh --patch --dry-run

# Actual release
./scripts/release.sh --patch
```

**What it does:**
- ✅ Bumps version (0.74.2 → 0.74.3)
- ✅ Builds macOS binaries (x86_64, aarch64)
- ✅ Creates GitHub release with changelog
- ✅ Uploads macOS binaries to GitHub Release
- ✅ Publishes crates to crates.io
- ✅ Updates Homebrew formula

**Output:**
- `v0.74.3` git tag created
- GitHub Release with 2 macOS binaries
- New version on crates.io

### Step 2: Trigger Cross-Platform Builds (CI)

```bash
cd /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode

# Trigger GitHub Actions for Linux & Windows
./scripts/trigger-ci-release.sh v0.74.3

# Monitor progress
gh run list --workflow=release.yml --limit 1
gh run view <run-id> --log

# Or visit GitHub UI
open https://github.com/vinhnx/vtcode/actions/workflows/release.yml
```

**GitHub Actions workflow builds:**
- Linux (x86_64-unknown-linux-gnu)
- Linux (aarch64-unknown-linux-gnu) 
- Windows (x86_64-pc-windows-msvc)
- Windows (aarch64-pc-windows-msvc)

**Timeline:** ~15 minutes for all platforms

### Step 3: Verify Binaries Exist

```bash
# Check GitHub Release assets
gh release view v0.74.3 --repo vinhnx/vtcode

# Or via curl
curl -s https://api.github.com/repos/vinhnx/vtcode/releases/tags/v0.74.3 | jq '.assets[].name'
```

**Expected output:**
```
vtcode-v0.74.3-aarch64-apple-darwin.tar.gz
vtcode-v0.74.3-aarch64-apple-darwin.sha256
vtcode-v0.74.3-aarch64-pc-windows-msvc.tar.gz
vtcode-v0.74.3-aarch64-pc-windows-msvc.sha256
vtcode-v0.74.3-aarch64-unknown-linux-gnu.tar.gz
vtcode-v0.74.3-aarch64-unknown-linux-gnu.sha256
vtcode-v0.74.3-x86_64-apple-darwin.tar.gz
vtcode-v0.74.3-x86_64-apple-darwin.sha256
vtcode-v0.74.3-x86_64-pc-windows-msvc.tar.gz
vtcode-v0.74.3-x86_64-pc-windows-msvc.sha256
vtcode-v0.74.3-x86_64-unknown-linux-gnu.tar.gz
vtcode-v0.74.3-x86_64-unknown-linux-gnu.sha256
```

### Step 4: Update Registry (Registry Repo)

```bash
cd /Users/vinhnguyenxuan/Developer/learn-by-doing/registry

# Update agent.json with all 6 platform binaries
./update-vtcode-binaries.sh v0.74.3

# Verify binaries are referenced
cat vtcode/agent.json | grep -A 1 "archive"

# Validate registry (WITHOUT URL validation first time)
# Once binaries exist on GitHub, remove SKIP_URL_VALIDATION
SKIP_URL_VALIDATION=1 uv run --with jsonschema .github/workflows/build_registry.py

# Or without skipping (once all binaries are available)
uv run --with jsonschema .github/workflows/build_registry.py
```

### Step 5: Commit Registry Changes

```bash
cd /Users/vinhnguyenxuan/Developer/learn-by-doing/registry

git add vtcode/agent.json dist/
git commit -m "chore: update vtcode to v0.74.3 with all platform binaries"
git push origin main

# Verify
git log --oneline -1
```

## Workflow Timeline

```
Time    Action                          Duration    Output
────────────────────────────────────────────────────────────────
T+0     ./release.sh --patch            ~5-10 min   v0.74.3 tag, GitHub Release, crates.io
T+10    ./trigger-ci-release.sh         instant     CI workflow started
T+10    GitHub Actions builds           ~15 min     6 additional binaries
T+25    Verify binaries on GitHub       instant     All 10 assets present
T+25    ./update-vtcode-binaries.sh     instant     agent.json updated
T+25    Registry rebuild                instant     dist/registry.json updated
T+26    git push registry               instant     Registry updated
```

## Automation Breakdown

### Local Release (`./scripts/release.sh`)

**Handles:**
1. Version bump
2. Changelog generation
3. Git tagging
4. macOS binary builds
5. GitHub release creation
6. crates.io publication
7. GitHub binary upload
8. Homebrew formula update

**Prerequisites:**
- `cargo-release` installed
- `gh` CLI authenticated
- Clean git tree

**Options:**
```bash
./scripts/release.sh --patch              # Patch version bump
./scripts/release.sh --minor              # Minor version bump
./scripts/release.sh --major              # Major version bump
./scripts/release.sh --patch --dry-run    # Test without changes
./scripts/release.sh --patch --skip-crates  # Skip crates.io publish
./scripts/release.sh --patch --skip-binaries # Skip binary upload
```

### CI Release (`./scripts/trigger-ci-release.sh`)

**Handles:**
- Triggers `.github/workflows/release.yml`
- Builds all non-macOS platforms
- Auto-uploads to existing GitHub Release

**Example:**
```bash
./scripts/trigger-ci-release.sh v0.74.3
```

### Registry Update (`./update-vtcode-binaries.sh`)

**Handles:**
1. Verifies all binaries exist on GitHub
2. Generates new agent.json with all platforms
3. Updates version number
4. Validates binary URLs

**Example:**
```bash
./update-vtcode-binaries.sh v0.74.3
```

## Troubleshooting

### Release script fails at "gh auth"
```bash
# Login to GitHub
gh auth login

# Refresh scopes if needed
gh auth refresh -h github.com -s workflow
```

### CI workflow fails
```bash
# View workflow logs
gh run view <run-id> --log

# View specific job
gh run view <run-id> --job <job-id>

# Rerun failed jobs
gh run rerun <run-id> --failed
```

### Registry validation fails
```bash
# Skip URL validation temporarily
SKIP_URL_VALIDATION=1 uv run --with jsonschema .github/workflows/build_registry.py

# Full validation once binaries are live
uv run --with jsonschema .github/workflows/build_registry.py
```

### Binary URLs are 404
```bash
# Wait for GitHub Actions to finish
gh run list --workflow=release.yml

# Check Release page
gh release view v0.74.3
```

## Manual Steps (If Automation Fails)

### Create GitHub Release Manually
```bash
gh release create v0.74.3 \
  --title "VT Code v0.74.3" \
  --notes "Release notes here"
```

### Upload Binaries Manually
```bash
gh release upload v0.74.3 \
  target/x86_64-apple-darwin/release/vtcode \
  target/aarch64-apple-darwin/release/vtcode \
  --clobber
```

### Update Homebrew Manually
```bash
cd ~/homebrew-tap/
git pull origin main
# Edit Formula/vtcode.rb with new checksums
git add .
git commit -m "vtcode: update to v0.74.3"
git push origin main
```

## Files Modified During Release

### VT Code Repo
- `Cargo.toml` - version bump
- `CHANGELOG.md` - new entries
- Git tag `v0.74.3` created
- GitHub Release created with binaries

### Registry Repo
- `vtcode/agent.json` - binary URLs updated
- `dist/registry.json` - rebuilt
- `dist/registry-for-jetbrains.json` - rebuilt

### External
- GitHub Release assets (6+ binaries)
- crates.io (new versions)
- docs.rs (rebuilds)
- Homebrew formula

## Next Steps

After successful release:

1. ✅ Announce release in Discord/Twitter
2. ✅ Update download links in README
3. ✅ Monitor docs.rs build completion
4. ✅ Verify Homebrew formula works: `brew install vinhnx/tap/vtcode`

## See Also

- [RELEASE_AUTOMATION.md](./RELEASE_AUTOMATION.md) - Script details
- [.github/workflows/release.yml](../.github/workflows/release.yml) - CI workflow
- [scripts/release.sh](../scripts/release.sh) - Local release script
- [scripts/trigger-ci-release.sh](../scripts/trigger-ci-release.sh) - CI trigger script
