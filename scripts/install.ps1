# VT Code Installer for Windows
# Usage: irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex

param(
    [string]$InstallDir = "",
    [switch]$NoCleanup = $false
)

$ErrorActionPreference = "Stop"
$ProgressPreference = 'SilentlyContinue'

# Logging
function Write-Log { Write-Host "➜ $args" -ForegroundColor Cyan }
function Write-Success { Write-Host "✓ $args" -ForegroundColor Green }
function Write-Error-Msg { Write-Host "✗ $args" -ForegroundColor Red }

# Get version
function Get-Version {
    Write-Log "Checking latest version..."
    try {
        $response = Invoke-RestMethod -Uri "https://api.github.com/repos/vinhnx/vtcode/releases/latest" -ErrorAction Stop
        $version = $response.tag_name -replace "^v", ""
        Write-Success "Latest: v$version"
        return $version
    } catch {
        Write-Error-Msg "Failed to fetch version: $_"
        exit 1
    }
}

# Download and extract
function Get-Binary {
    param([string]$Version)
    
    $url = "https://github.com/vinhnx/vtcode/releases/download/v${Version}/vtcode-v${Version}-x86_64-pc-windows-msvc.zip"
    $tempDir = [System.IO.Path]::GetTempPath()
    $archive = Join-Path $tempDir "vtcode-$Version.zip"
    
    Write-Log "Downloading..."
    try {
        Invoke-WebRequest -Uri $url -OutFile $archive -ErrorAction Stop
    } catch {
        Write-Error-Msg "Download failed: $_"
        exit 1
    }
    
    Write-Log "Extracting..."
    $extractDir = Join-Path $tempDir "vtcode-extract-$(Get-Random)"
    New-Item -ItemType Directory -Path $extractDir -Force | Out-Null
    
    try {
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        [System.IO.Compression.ZipFile]::ExtractToDirectory($archive, $extractDir)
    } catch {
        Write-Error-Msg "Extract failed: $_"
        exit 1
    }
    
    $binary = Join-Path $extractDir "vtcode.exe"
    if (-not (Test-Path $binary)) {
        Write-Error-Msg "Binary not found in archive"
        exit 1
    }
    
    Write-Success "Downloaded"
    
    return @{
        Binary = $binary
        TempDir = $extractDir
        Archive = $archive
    }
}

# Find install directory
function Get-InstallPath {
    if ($InstallDir) {
        if (-not (Test-Path $InstallDir)) {
            New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
        }
        return $InstallDir
    }
    
    # Try Program Files
    $progFiles = [Environment]::GetFolderPath("ProgramFiles")
    $vtDir = Join-Path $progFiles "VTCode"
    
    if (-not (Test-Path $vtDir)) {
        try {
            New-Item -ItemType Directory -Path $vtDir -Force | Out-Null
            Write-Log "Created: $vtDir"
            return $vtDir
        } catch {
            # Fall through to LocalAppData
        }
    }
    
    # Fall back to LocalAppData
    $localApp = [Environment]::GetFolderPath("LocalApplicationData")
    $vtDir = Join-Path $localApp "VTCode"
    New-Item -ItemType Directory -Path $vtDir -Force | Out-Null
    return $vtDir
}

# Install
function Install-Binary {
    param($BinInfo)
    
    $installPath = Get-InstallPath
    Write-Log "Installing to $installPath..."
    
    # Stop running processes
    Get-Process vtcode -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 300
    
    $target = Join-Path $installPath "vtcode.exe"
    Copy-Item $BinInfo.Binary $target -Force
    
    Write-Success "Installed"
    
    # Add to PATH
    $userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
    if ($userPath -notlike "*$installPath*") {
        Write-Log "Adding to PATH..."
        [Environment]::SetEnvironmentVariable("PATH", "$installPath;$userPath", "User")
        $env:PATH = "$installPath;$env:PATH"
        Write-Success "Added to PATH"
    }
    
    # Cleanup
    if (-not $NoCleanup) {
        Remove-Item $BinInfo.TempDir -Recurse -Force -ErrorAction SilentlyContinue
        Remove-Item $BinInfo.Archive -Force -ErrorAction SilentlyContinue
    }
    
    return $target
}

# Verify
function Verify {
    param([string]$Binary)
    
    Write-Log "Verifying..."
    try {
        $version = & $Binary --version 2>&1
        if ($LASTEXITCODE -eq 0) {
            Write-Success "VT Code installed!"
            return
        }
    } catch {}
    
    Write-Error-Msg "Verification failed"
    exit 1
}

# Main
Write-Host ""
Write-Host "VT Code Installer" -ForegroundColor Cyan
Write-Host "=================" -ForegroundColor Cyan
Write-Host ""

$version = Get-Version
$binInfo = Get-Binary -Version $version
$binary = Install-Binary $binInfo
Verify -Binary $binary

Write-Host ""
Write-Host "Quick start:" -ForegroundColor Cyan
Write-Host '  $env:OPENAI_API_KEY = "sk-..."'
Write-Host "  vtcode"
Write-Host ""
