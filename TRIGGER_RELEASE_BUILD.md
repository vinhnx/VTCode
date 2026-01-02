# Manually Trigger Release Build

**Issue**: The v0.58.6 release was created successfully, but the build-release.yml workflow didn't automatically trigger.

**Solution**: Manually trigger the build using GitHub's UI or CLI.

---

## Option 1: GitHub Web UI (Easiest)

1. Go to: https://github.com/vinhnx/vtcode/actions
2. Click on **"Build and Release Binaries"** workflow
3. Click **"Run workflow"** dropdown button (top right)
4. Enter version: `v0.58.6`
5. Click **"Run workflow"** button
6. Wait 10-15 minutes for builds to complete

---

## Option 2: GitHub CLI (If Installed)

First, authenticate:
```bash
gh auth login
```

Then trigger the build:
```bash
gh workflow run build-release.yml -f version=v0.58.6 --repo vinhnx/vtcode
```

Check status:
```bash
gh run list --workflow build-release.yml --repo vinhnx/vtcode
```

---

## What Happens After Trigger

Once you trigger the build workflow:

1. **Ubuntu Build** (~5 min)
   - Compiles `x86_64-unknown-linux-gnu`
   
2. **macOS Builds** (~5 min each, parallel)
   - Compiles `x86_64-apple-darwin` (Intel)
   - Compiles `aarch64-apple-darwin` (Apple Silicon) ← Your platform

3. **Windows Build** (~5 min)
   - Compiles `x86_64-pc-windows-msvc`

4. **Upload** (~1 min)
   - All 4 binaries uploaded to GitHub Release

5. **Checksum Workflow Auto-Triggers** (~2 min)
   - Generates `checksums.txt`
   - Uploads to release

**Total Time**: ~15-20 minutes

---

## Check Build Progress

### Via GitHub UI:
1. Go to: https://github.com/vinhnx/vtcode/actions
2. Click on the "Build and Release Binaries" workflow
3. Watch the build progress in real-time
4. Check for errors in any platform build

### Via CLI:
```bash
# List recent runs
gh run list --workflow build-release.yml --repo vinhnx/vtcode --limit 5

# View specific run details
gh run view <run-id> --repo vinhnx/vtcode --log

# Watch logs in real-time
gh run watch <run-id> --repo vinhnx/vtcode
```

---

## After Build Completes

Check release page: https://github.com/vinhnx/vtcode/releases/tag/v0.58.6

You should see:
- ✓ `vtcode-v0.58.6-x86_64-unknown-linux-gnu.tar.gz`
- ✓ `vtcode-v0.58.6-x86_64-apple-darwin.tar.gz`
- ✓ `vtcode-v0.58.6-aarch64-apple-darwin.tar.gz`
- ✓ `vtcode-v0.58.6-x86_64-pc-windows-msvc.zip`
- ✓ `checksums.txt`

Then your installer will work:
```bash
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash
```

---

## Why Didn't It Auto-Trigger?

The build-release.yml is configured to trigger on `release: types: [published]`. However:

1. The release.yml workflow created the release
2. But it might not have had the right permissions or timing
3. Or there might be a GitHub Actions quirk with the trigger

This is why the manual trigger option exists - it allows you to kick off the build manually if needed.

---

## Next Steps

1. **Trigger the build** (use Option 1 or 2 above)
2. **Wait** for it to complete (~15-20 minutes)
3. **Check** the release page for binaries
4. **Run** the installer when ready

---

**TL;DR**: Go to https://github.com/vinhnx/vtcode/actions, click "Build and Release Binaries", enter `v0.58.6`, click "Run workflow". Wait 15-20 minutes. Done!
