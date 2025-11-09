# Native Installers - Technical Guide

Detailed information about shell and PowerShell installers.

## macOS & Linux (Shell Installer)

### Features
- Auto-detects OS and architecture (Intel, ARM64, ARMv7)
- Downloads latest release from GitHub
- Smart path selection with fallbacks
- Color-coded output with progress
- Verifies installation before completing
- Guides next steps

### Supported Platforms
- macOS 10.15+ (Intel & Apple Silicon)
- Linux x86_64, ARM64, ARMv7
- Windows Subsystem for Linux (WSL)

### Installation
```bash
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash
```

### Manual Download & Run
```bash
curl https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh -o install.sh
chmod +x install.sh
./install.sh
```

### How It Works

1. **Platform Detection**
   - Detects OS (macOS, Linux, Windows)
   - Detects architecture (Intel, ARM64, ARMv7)
   - Maps to release binary name (e.g., `x86_64-apple-darwin`)

2. **Version Fetching**
   - Queries GitHub API for latest release
   - No hardcoded versions

3. **Download & Extract**
   - Downloads binary from GitHub releases
   - Extracts to temporary directory
   - Verifies binary exists

4. **Installation**
   - Tries multiple paths: `/usr/local/bin` → `/opt/local/bin` → `~/.local/bin`
   - Adds to PATH if needed
   - Makes binary executable

5. **Verification**
   - Runs `vtcode --version` to confirm
   - Reports success or failure
   - Shows next steps

### Error Handling

```bash
# Unsupported platform
Error: Unsupported OS: SunOS

# Download failed
Error: Failed to download from https://...

# No write permissions
Error: No suitable installation directory found
```

All errors exit with code 1 for CI/CD integration.

### Environment Variables

None required. Script auto-detects everything.

### CI/CD Usage

```bash
#!/bin/bash
set -e
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash
vtcode --version
```

---

## Windows (PowerShell Installer)

### Features
- Auto-detects Windows architecture
- Smart directory selection (Program Files → LocalAppData)
- .NET ZipFile extraction (compatible with PS 3.0+)
- Automatically configures PATH
- Graceful admin privilege handling
- Supports custom install directory

### Requirements
- PowerShell 3.0+ (built-in on Windows 7+)
- .NET 3.5+ (usually pre-installed)

### Installation
```powershell
irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex
```

### With Custom Install Directory
```powershell
irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex -ArgumentList @{InstallDir="C:\Tools\VTCode"}
```

### Without Cleanup
```powershell
irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex -ArgumentList @{NoCleanup=$true}
```

### Manual Download & Run
```powershell
$uri = "https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1"
Invoke-WebRequest -Uri $uri -OutFile install.ps1
Set-ExecutionPolicy -ExecutionPolicy Bypass -Scope Process
.\install.ps1
```

### How It Works

1. **Architecture Detection**
   - Checks if 64-bit process
   - Detects ARM64 via WMI
   - Maps to `x86_64-pc-windows-msvc`

2. **Privilege Checking**
   - Detects if running as admin
   - Warns if not (uses user directory fallback)

3. **Version Fetching**
   - Queries GitHub API for latest release
   - No hardcoded versions

4. **Download & Extract**
   - Downloads binary ZIP from GitHub releases
   - Uses .NET ZipFile for extraction
   - Verifies binary exists in archive

5. **Installation**
   - Tries Program Files first (requires admin)
   - Falls back to LocalAppData (user-scoped)
   - Kills any running vtcode processes
   - Copies binary to selected directory

6. **PATH Configuration**
   - Checks if directory in PATH
   - Adds if missing (via [Environment]::SetEnvironmentVariable)
   - Updates current session PATH
   - Warns if restart needed

7. **Verification**
   - Runs `vtcode --version` to confirm
   - Reports success or failure

### Error Handling

```powershell
# No suitable directory found
✗ No suitable installation directory found

# Download failed
✗ Failed to download from https://...

# Extraction failed
✗ Failed to extract archive: ...
```

All errors exit with code 1 for CI/CD integration.

### Parameters

```powershell
-InstallDir <string>    # Custom installation directory
-NoCleanup             # Keep temporary files (debugging)
```

### CI/CD Usage

```powershell
$ErrorActionPreference = "Stop"
irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex
vtcode --version
```

---

## Homebrew Formula

### Setup (One-Time)

VT Code Homebrew formula is available at `homebrew/vtcode.rb`.

To make it available via `brew install vtcode`, you need a Homebrew tap:

```bash
# Option 1: Personal tap (recommended)
mkdir -p ~/homebrew-vtcode
cp homebrew/vtcode.rb ~/homebrew-vtcode/Formula/
cd ~/homebrew-vtcode
git init
git add .
git commit -m "Add VT Code formula"
git remote add origin https://github.com/YOUR_USER/homebrew-vtcode
git push -u origin main

# Then users can install with:
brew tap YOUR_USER/vtcode
brew install vtcode
```

Or use the formula directly:

```bash
brew install --formula ~/path/to/vtcode.rb
```

### Updating for New Releases

Edit `homebrew/vtcode.rb`:

```ruby
version "X.Y.Z"

on_macos do
  if Hardware::CPU.arm?
    sha256 "NEW_SHA_HERE"  # Run: shasum -a 256 vtcode-vX.Y.Z-aarch64-apple-darwin.tar.gz
  else
    sha256 "NEW_SHA_HERE"  # Run: shasum -a 256 vtcode-vX.Y.Z-x86_64-apple-darwin.tar.gz
  end
end

on_linux do
  if Hardware::CPU.arm? && Hardware::CPU.is_64_bit?
    sha256 "NEW_SHA_HERE"  # ARM64
  else
    sha256 "NEW_SHA_HERE"  # x86_64
  end
end
```

Then commit and push to your tap repository.

---

## Comparison

| Feature | Shell (macOS/Linux) | PowerShell (Windows) | Homebrew |
|---------|---------------------|---------------------|----------|
| Platform detection | ✓ Auto | ✓ Auto | ✓ Auto |
| Version management | ✗ Latest only | ✗ Latest only | ✓ Version tracking |
| PATH configuration | ✓ Auto | ✓ Auto | ✓ Auto |
| Easy uninstall | ✓ One command | ✓ One command | ✓ `brew uninstall` |
| Admin required | ✗ Optional | ✗ Optional (fallback) | ✗ No |
| Custom path | ✗ No | ✓ Yes | ✗ No |
| Simple | ✓ Yes | Moderate | ✓ Yes |

---

## Security

All installers:
- ✓ HTTPS-only downloads
- ✓ Use official GitHub releases
- ✓ Verify binary exists after extraction
- ✓ Display clear error messages
- ✓ Never ask for root/admin unless necessary
- ✓ Clean up temporary files

No:
- ✗ Custom code execution
- ✗ Hardcoded URLs (except GitHub)
- ✗ Hidden operations
- ✗ Telemetry

---

## Debugging

### Verbose Output

Add `set -x` to shell script to see all commands:

```bash
bash -x <(curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh)
```

### Keep Temp Files

```powershell
irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex -ArgumentList @{NoCleanup=$true}
```

### Check Downloaded Binary

```bash
# macOS
file /usr/local/bin/vtcode
ldd /usr/local/bin/vtcode

# Linux
file /usr/local/bin/vtcode
ldd /usr/local/bin/vtcode
```

---

## Known Issues

### macOS M1/M2: Command not found

If `vtcode` isn't found after installation:
```bash
# Check if it's in PATH
which vtcode
echo $PATH

# Add to PATH if needed
export PATH="/usr/local/bin:$PATH"

# Make permanent (add to ~/.zshrc)
echo 'export PATH="/usr/local/bin:$PATH"' >> ~/.zshrc
```

### Linux: Permission denied

```bash
# Make executable
chmod +x /usr/local/bin/vtcode

# Or check ownership
ls -la /usr/local/bin/vtcode
sudo chown $USER:$USER /usr/local/bin/vtcode
```

### Windows: File locked during install

The installer kills any running `vtcode` processes:
```powershell
# Or manually:
Get-Process vtcode -ErrorAction SilentlyContinue | Stop-Process -Force
```

---

## File Locations

In the repository:
- `scripts/install.sh` - Shell installer
- `scripts/install.ps1` - PowerShell installer
- `homebrew/vtcode.rb` - Homebrew formula

Available at (via GitHub raw):
- https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh
- https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1
