# VT Code Native Installer (Windows PowerShell)
# Downloads and installs the latest VT Code binary from GitHub Releases

param(
    [string]$InstallDir = "$env:USERPROFILE\.local\bin",
    [switch]$Help
)

$ErrorActionPreference = "Stop"

# Configuration
$Repo = "vinhnx/vtcode"
$BinName = "vtcode.exe"
$GitHubAPI = "https://api.github.com/repos/$Repo/releases/latest"
$GitHubReleases = "https://github.com/$Repo/releases/download"

# Logging functions
function Write-Info { Write-Host "INFO: $args" -ForegroundColor Cyan }
function Write-Success { Write-Host "✓ $args" -ForegroundColor Green }
function Write-Error { Write-Host "✗ $args" -ForegroundColor Red }
function Write-Warning { Write-Host "⚠ $args" -ForegroundColor Yellow }

# Show help
function Show-Help {
    Write-Host @"
VT Code Native Installer (Windows PowerShell)

Usage: .\install.ps1 [options]

Options:
  -InstallDir <path>   Installation directory (default: $env:USERPROFILE\.local\bin)
  -Help                Show this help message

Examples:
  .\install.ps1                                         # Install to user directory
  .\install.ps1 -InstallDir "C:\Program Files\vtcode" # Install to Program Files

Note: Installing to Program Files may require administrator privileges.
"@
    exit 0
}

if ($Help) {
    Show-Help
}

# Ensure installation directory exists
if (-not (Test-Path $InstallDir)) {
    Write-Info "Creating installation directory: $InstallDir"
    New-Item -ItemType Directory -Path $InstallDir -Force > $null
}

# Fetch latest release from GitHub API
function Get-LatestRelease {
    Write-Info "Fetching latest VT Code release from GitHub..."
    
    try {
        $response = Invoke-RestMethod -Uri $GitHubAPI -ErrorAction Stop
        return $response
    }
    catch {
        Write-Error "Failed to fetch release info from GitHub API"
        Write-Error "Error: $_"
        exit 1
    }
}

# Get platform-specific download URL
function Get-DownloadUrl {
    param([object]$Release)
    
    $ReleaseTag = $Release.tag_name
    Write-Info "Latest version: $ReleaseTag"
    
    # Windows always uses x86_64-pc-windows-msvc
    $Platform = "x86_64-pc-windows-msvc"
    $Filename = "vtcode-$ReleaseTag-$Platform.zip"
    $DownloadUrl = "$GitHubReleases/$ReleaseTag/$Filename"
    
    return @{
        Url = $DownloadUrl
        Tag = $ReleaseTag
        Filename = $Filename
    }
}

# Download binary with progress
function Download-Binary {
    param(
        [string]$Url,
        [string]$OutputPath
    )
    
    Write-Info "Downloading binary from GitHub..."
    Write-Info "URL: $Url"
    
    try {
        # Use ProgressPreference to show download progress
        $ProgressPreference = 'Continue'
        Invoke-WebRequest -Uri $Url -OutFile $OutputPath -ErrorAction Stop
        Write-Success "Downloaded successfully"
    }
    catch {
        Write-Error "Failed to download binary"
        Write-Error "Error: $_"
        exit 1
    }
}

# Verify checksum if available
function Verify-Checksum {
    param(
        [string]$BinaryPath,
        [string]$ReleaseTag
    )
    
    Write-Info "Verifying binary integrity..."
    
    $Filename = (Get-Item $BinaryPath).Name
    $TempChecksumFile = Join-Path $env:TEMP "vtcode-checksums.txt"
    $ExpectedChecksum = ""

    # Try to download checksums.txt first
    $ChecksumsUrl = "$GitHubReleases/$ReleaseTag/checksums.txt"
    try {
        Invoke-WebRequest -Uri $ChecksumsUrl -OutFile $TempChecksumFile -ErrorAction SilentlyContinue
        if (Test-Path $TempChecksumFile) {
            $ExpectedChecksum = (Get-Content $TempChecksumFile | Select-String $Filename | ForEach-Object { $_ -split '\s+' } | Select-Object -First 1).Trim()
        }
    }
    catch {
        # Continue to try individual file
    }

    # If not found in checksums.txt, try individual .sha256 file
    if ([string]::IsNullOrEmpty($ExpectedChecksum)) {
        $ShaUrl = "$GitHubReleases/$ReleaseTag/$Filename.sha256"
        try {
            Invoke-WebRequest -Uri $ShaUrl -OutFile $TempChecksumFile -ErrorAction SilentlyContinue
            if (Test-Path $TempChecksumFile) {
                $ExpectedChecksum = (Get-Content $TempChecksumFile | Select-Object -First 1).Trim()
            }
        }
        catch {
            # Both failed
        }
    }
    
    if ([string]::IsNullOrEmpty($ExpectedChecksum)) {
        Write-Warning "Checksum not found for $Filename, skipping verification"
        if (Test-Path $TempChecksumFile) { Remove-Item -Path $TempChecksumFile -Force }
        return
    }
    
    # Compute actual checksum
    $ActualChecksum = (Get-FileHash -Path $BinaryPath -Algorithm SHA256).Hash
    
    # PowerShell Get-FileHash returns uppercase, while shasum usually returns lowercase
    if ($ActualChecksum.ToLower() -ne $ExpectedChecksum.ToLower()) {
        Write-Error "Checksum mismatch for $Filename!"
        Write-Error "Expected: $ExpectedChecksum"
        Write-Error "Got:      $ActualChecksum"
        if (Test-Path $TempChecksumFile) { Remove-Item -Path $TempChecksumFile -Force }
        exit 1
    }
    
    Write-Success "Checksum verified: $ExpectedChecksum"
    
    # Clean up temporary file
    if (Test-Path $TempChecksumFile) { Remove-Item -Path $TempChecksumFile -Force }
}

# Extract binary from zip
function Extract-Binary {
    param(
        [string]$ZipPath,
        [string]$OutputDir
    )
    
    Write-Info "Extracting binary..."
    
    try {
        Expand-Archive -Path $ZipPath -DestinationPath $OutputDir -Force -ErrorAction Stop
    }
    catch {
        Write-Error "Failed to extract binary"
        Write-Error "Error: $_"
        exit 1
    }
    
    # Find the binary
    $BinaryPath = Get-ChildItem -Path $OutputDir -Name "$BinName" -Recurse | Select-Object -First 1
    
    if ($null -eq $BinaryPath) {
        Write-Error "Binary not found in archive"
        exit 1
    }
    
    return Join-Path $OutputDir $BinaryPath
}

# Install binary
function Install-Binary {
    param(
        [string]$Source,
        [string]$Target
    )
    
    Write-Info "Installing to $Target..."
    
    try {
        Copy-Item -Path $Source -Destination $Target -Force -ErrorAction Stop
        Write-Success "Binary installed to $Target"
    }
    catch {
        Write-Error "Failed to install binary to $Target"
        Write-Error "Error: $_"
        Write-Info "You may need to run PowerShell as Administrator"
        exit 1
    }
}

# Check if installation directory is in PATH
function Check-Path {
    param([string]$InstallPath)
    
    $PathDirs = $env:PATH -split ';'
    return $PathDirs -contains $InstallPath
}

# Add installation directory to PATH
function Add-ToPath {
    param([string]$InstallPath)
    
    Write-Warning "Installation directory is not in PATH"
    Write-Info "Add the following to your user environment variables:"
    Write-Host ""
    Write-Host "  Path: $InstallPath"
    Write-Host ""
    Write-Info "To add it manually:"
    Write-Host '  1. Press Win+X, select "System"'
    Write-Host '  2. Click "Advanced system settings"'
    Write-Host '  3. Click "Environment Variables"'
    Write-Host '  4. Under "User variables", select "Path" and click "Edit"'
    Write-Host "  5. Click "New" and add: $InstallPath"
    Write-Host ""
}

# Cleanup temporary files
function Cleanup {
    Get-ChildItem -Path $env:TEMP -Name "vtcode-*" -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
}

# Main installation flow
function Main {
    Write-Host "VT Code Native Installer (Windows)" -ForegroundColor Magenta
    Write-Host ""
    
    # Create temporary directory for downloads
    $TempDir = New-Item -ItemType Directory -Path "$env:TEMP\vtcode-install-$(Get-Random)" -Force
    
    try {
        # Fetch latest release
        $Release = Get-LatestRelease
        
        # Get download URL
        $DownloadInfo = Get-DownloadUrl $Release
        
        # Download binary
        $ArchivePath = Join-Path $TempDir "vtcode-binary.zip"
        Download-Binary -Url $DownloadInfo.Url -OutputPath $ArchivePath
        
        # Verify checksum
        Verify-Checksum -BinaryPath $ArchivePath -ReleaseTag $DownloadInfo.Tag
        
        # Extract binary
        $ExtractDir = Join-Path $TempDir "extract"
        New-Item -ItemType Directory -Path $ExtractDir -Force > $null
        $BinaryPath = Extract-Binary -ZipPath $ArchivePath -OutputDir $ExtractDir
        
        # Install binary
        $TargetPath = Join-Path $InstallDir $BinName
        Install-Binary -Source $BinaryPath -Target $TargetPath
        
        # Check if in PATH
        if (-not (Check-Path $InstallDir)) {
            Add-ToPath $InstallDir
        }
        
        Write-Host ""
        Write-Success "Installation complete!"
        Write-Info "VT Code is ready to use"
        Write-Host ""
        
        # Test installation
        try {
            $Version = & $TargetPath --version
            Write-Success "Version check passed: $Version"
        }
        catch {
            Write-Warning "Could not verify installation, but binary appears to be installed"
        }
        
        Write-Host ""
        Write-Info "To get started, run: vtcode ask 'hello world'"
    }
    finally {
        # Cleanup
        Remove-Item -Path $TempDir -Recurse -Force -ErrorAction SilentlyContinue
        Cleanup
    }
}

Main
