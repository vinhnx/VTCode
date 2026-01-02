# Release v0.58.6 Monitoring Guide

**Status**: Binaries building (check status below)  
**Release**: v0.58.6 - macOS Apple Silicon  
**Expected Time to Ready**: 10-20 minutes from now

---

## Quick Start - Auto-Install When Ready

The easiest way - just run this and walk away. It will automatically install VT Code when binaries are ready:

```bash
./scripts/wait-for-release.sh -a
```

That's it. The script will:
1. Poll GitHub API every 15 seconds
2. Detect when binaries are available
3. Automatically run the installer
4. You'll have VT Code installed in 5-10 minutes

---

## Check Release Status Right Now

See if binaries are ready and when they'll be available:

```bash
./scripts/check-release-status.sh
```

**Output Examples**:

**Status 1 - Still Building** (right now)
```
Your Platform: macOS Apple Silicon
Binary Name:   vtcode-v0.58.6-aarch64-apple-darwin.tar.gz

Assets:
  Total Binaries:     0/4
  Your Binary:        ⏳ Building...
  Checksums:         ⏳ Generating...

⏳ BUILDING
Binaries are still being built. Check back in 5-10 minutes.
```

**Status 2 - Binary Ready, Checksums Pending** (middle of build)
```
Your Platform: macOS Apple Silicon
Binary Name:   vtcode-v0.58.6-aarch64-apple-darwin.tar.gz

Assets:
  Total Binaries:     3/4
  Your Binary:        ✓ Available
  Checksums:         ⏳ Generating...

⏳ ALMOST READY
Binary is built, waiting for checksums.txt to be generated...
```

**Status 3 - Ready to Install** (after ~20 minutes)
```
Your Platform: macOS Apple Silicon
Binary Name:   vtcode-v0.58.6-aarch64-apple-darwin.tar.gz

Assets:
  Total Binaries:     4/4
  Your Binary:        ✓ Available
  Checksums:         ✓ Available

✓ READY TO INSTALL
Install VT Code with:
  curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash
```

---

## Monitoring Options

### Option 1: Auto-Wait & Install (Recommended)
**Best for**: You want to walk away and come back to an installed VT Code

```bash
./scripts/wait-for-release.sh -a
```

The script will:
- Poll every 15 seconds by default
- Auto-install when ready
- Show progress in real-time
- Exit when complete

---

### Option 2: Manual Polling
**Best for**: You want to see progress and decide when to install

```bash
# Check status now
./scripts/check-release-status.sh

# Check again in 5 minutes
./scripts/check-release-status.sh

# Keep checking until ready...
./scripts/check-release-status.sh
```

When it shows `✓ READY TO INSTALL`, run:
```bash
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash
```

---

### Option 3: Background Monitor
**Best for**: Run once and forget about it

```bash
# Start monitoring in background
nohup ./scripts/wait-for-release.sh -a > ~/vtcode-install.log 2>&1 &

# Check log anytime
tail -f ~/vtcode-install.log

# Or just wait for it to be done
# When done, you'll have vtcode installed!
```

---

### Option 4: GitHub Actions (Manual)
**Best for**: Technical users who want to see all workflows

Visit: https://github.com/vinhnx/vtcode/actions

Look for:
1. **Release** workflow - Creates GitHub Release
2. **Build and Release Binaries** workflow - Builds for 4 platforms
3. **Native Installer - Generate Checksums** workflow - Creates checksums.txt

When all are green ✓, binaries are ready.

---

## Expected Timeline

```
NOW (Build in progress)
  └─ GitHub Actions building binaries for 4 platforms
  └─ Check with: ./scripts/check-release-status.sh

5-10 MINUTES
  └─ First binaries uploaded (you may see 1-2 available)
  └─ Status: 1-2/4 binaries ready

10-20 MINUTES
  └─ All binaries uploaded
  └─ Checksum workflow starts
  └─ Status: 4/4 binaries ready, waiting for checksums

20-25 MINUTES
  └─ checksums.txt uploaded
  └─ Status: ✓ READY TO INSTALL
  └─ If using -a flag: Auto-install begins

DONE (25 minutes total)
  └─ VT Code installed
  └─ Verify: vtcode --version
```

---

## Script Reference

### wait-for-release.sh
Polls GitHub API and waits for binaries.

```bash
# Basic usage (polls every 5 seconds, max 30 min wait)
./scripts/wait-for-release.sh

# Poll every 10 seconds
./scripts/wait-for-release.sh -i 10

# Poll every 30 seconds, max 1 hour wait
./scripts/wait-for-release.sh -i 30 -m 3600

# Auto-install when ready (recommended)
./scripts/wait-for-release.sh -a

# Auto-install, poll every 20 seconds
./scripts/wait-for-release.sh -i 20 -a
```

### check-release-status.sh
Quick one-time status check.

```bash
# Check status now
./scripts/check-release-status.sh

# Check every 30 seconds in a loop
while true; do ./scripts/check-release-status.sh && break; sleep 30; done
```

---

## Recommended Approach

1. **Right now**: Run status check
   ```bash
   ./scripts/check-release-status.sh
   ```

2. **See it's building**: Start auto-wait
   ```bash
   ./scripts/wait-for-release.sh -a
   ```

3. **Walk away**: The script will notify when ready and auto-install

4. **Come back**: VT Code is installed
   ```bash
   vtcode --version  # Verify it worked
   ```

**Total time**: ~25 minutes, mostly waiting for GitHub Actions

---

## Troubleshooting

### Script says "Cannot reach GitHub API"
- Check your internet connection
- GitHub might be down (check https://status.github.com)
- Try again in a minute

### Binaries stuck at 3/4
- One platform build might be failing
- Check GitHub Actions for errors
- You can still use your platform binary once it's ready

### Checksum workflow never starts
- Wait longer - build workflows complete sequentially
- Check GitHub Actions page for status

### Auto-install fails
- Binary might be corrupted
- Delete the downloaded file and try again
- Check /tmp/ for partial downloads: `ls -la /tmp/vtcode-*`

### Just run the installer manually
Once binaries are available (Status 3), just run:
```bash
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash
```

---

## Verification

Once installation completes, verify it worked:

```bash
# Check version
vtcode --version

# Should output:
# vtcode 0.58.6
# Authors: vinhnx <vinhnx@users.noreply.github.com>
# ...
```

If that works, you're done! VT Code is ready to use.

---

## What's Happening Behind the Scenes

1. **Release Created** (v0.58.6 tag pushed)
   - Status: ✅ Done

2. **Release Workflow** (Creates GitHub Release)
   - Status: Running
   - Duration: ~5 min
   - What: Makes empty GitHub Release, publishes npm package

3. **Build Workflow** (Builds binaries)
   - Status: Queued (starts after Release completes)
   - Duration: ~10-15 min
   - What: Builds for 4 platforms in parallel:
     - Ubuntu: x86_64-unknown-linux-gnu
     - macOS: x86_64-apple-darwin
     - macOS: aarch64-apple-darwin ← Your platform
     - Windows: x86_64-pc-windows-msvc

4. **Checksum Workflow** (Generates SHA256)
   - Status: Queued (starts after binaries uploaded)
   - Duration: ~1-2 min
   - What: Downloads all binaries, generates checksums.txt

5. **Production Ready** (Installer can download)
   - Status: Pending
   - When: All workflows complete
   - What: Your installer can now fetch and verify binaries

---

## Need Help?

- **Script not working?** Check syntax: `bash -n scripts/wait-for-release.sh`
- **Want to see all options?** `./scripts/wait-for-release.sh -h`
- **Still stuck?** Check GitHub Actions: https://github.com/vinhnx/vtcode/actions
- **Network issue?** Check GitHub status: https://status.github.com

---

## Summary

✅ **Best Option**: Run `./scripts/wait-for-release.sh -a` and wait 25 minutes  
✅ **Alternative**: Manually check status with `./scripts/check-release-status.sh`  
✅ **Expected Time**: 20-30 minutes total  
✅ **Effort**: Once you start the auto-wait script, nothing else to do  

---

**Next Step**: Start the monitor!

```bash
./scripts/wait-for-release.sh -a
```

Come back in ~25 minutes and you'll have VT Code installed.
