# Quick Start: VT Code Native Installer

## For Users

### Install VT Code (One Command)

**macOS & Linux:**
```bash
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash
```

**Windows (PowerShell):**
```powershell
irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex
```

### Verify Installation
```bash
vtcode --version
vtcode ask "hello world"
```

### Update to New Version
```bash
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash
```

### Uninstall
```bash
rm ~/.local/bin/vtcode
```

---

## For Developers

### Test Installer Locally

```bash
# Test bash installer help
bash scripts/install.sh --help

# Test with custom directory (don't install globally)
INSTALL_DIR=/tmp/vtcode-test ./scripts/install.sh
```

### Verify Code Changes

```bash
# Check installer syntax
bash -n scripts/install.sh

# Verify dependencies
command -v curl
command -v tar
command -v sha256sum
```

### Trigger Binary Build & Checksum Generation

```bash
# Create a release (automatically builds binaries)
./scripts/release.sh --patch --dry-run

# Verify the workflow files exist
ls .github/workflows/build-release.yml
ls .github/workflows/native-installer.yml
```

### Integrate Auto-Updater (Optional)

Add to `src/main.rs`:

```rust
mod updater;
use crate::updater::Updater;

// Check for updates on startup
let updater = Updater::new(env!("CARGO_PKG_VERSION"))?;
if let Ok(Some(update)) = updater.check_for_updates().await {
    eprintln!("📦 New version available: {}", update.version);
}
Updater::record_update_check()?;
```

Then test:
```bash
cargo build
./target/debug/vtcode --version
```

---

## Troubleshooting

| Problem | Solution |
|---------|----------|
| "Command not found: vtcode" | Add `~/.local/bin` to PATH: `export PATH="$HOME/.local/bin:$PATH"` |
| "Permission denied" | Try: `curl ... \| bash -s -- --dir ~/bin` |
| "Checksum mismatch" | Internet issue - try again in a few minutes |
| PowerShell script won't run | Use: `pwsh -ExecutionPolicy Bypass -Command "..."`|
| Download is slow | Using GitHub CDN - should be fast. Check internet. |

---

## System Requirements

- **macOS**: 10.15+ (Intel or Apple Silicon)
- **Linux**: Ubuntu 20.04+, Debian 10+
- **Windows**: 10/11 with PowerShell or WSL
- **Tools**: curl, tar, sha256sum (usually pre-installed)
- **Space**: ~50MB for binary
- **Internet**: Required for download and auth

---

## What Gets Installed

```
~/.local/bin/
  └── vtcode          # Single self-contained binary
```

No additional files or directories created by installer (except cache).

---

## Advanced Options

### Custom Installation Directory

```bash
# macOS/Linux
curl -fsSL ... | bash -s -- --dir /custom/path

# Windows
$params = @{ InstallDir = "C:\custom\path" }
irm ... | iex
```

### Check for Updates Manually

Once integrated (see UPDATER_INTEGRATION.md):

```bash
vtcode update
```

### View Version Info

```bash
vtcode --version    # Current version
vtcode --help       # Usage information
```

---

## File Reference

| File | Purpose |
|------|---------|
| `scripts/install.sh` | macOS/Linux installer |
| `scripts/install.ps1` | Windows PowerShell installer |
| `src/updater.rs` | Auto-updater module |
| `docs/NATIVE_INSTALLER.md` | Full user guide |
| `docs/DISTRIBUTION_STRATEGY.md` | Architecture details |
| `docs/UPDATER_INTEGRATION.md` | Programmer's guide |
| `.github/workflows/build-release.yml` | Binary build workflow |
| `.github/workflows/native-installer.yml` | Checksum generation |

---

## Links

- **Main Docs**: `docs/NATIVE_INSTALLER.md`
- **Architecture**: `docs/DISTRIBUTION_STRATEGY.md`
- **Developer Guide**: `docs/UPDATER_INTEGRATION.md`
- **Full Summary**: `NATIVE_INSTALLER_SUMMARY.md`
- **GitHub Releases**: https://github.com/vinhnx/vtcode/releases
- **Installation Guide**: `README.md#installation`
