#!/usr/bin/env node

/**
 * Postinstall script for VT Code npm package
 * Downloads the appropriate binary for the current platform
 */

const https = require('https');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { execSync } = require('child_process');

// Get package version
const packageJson = require('../package.json');
const version = packageJson.version;

// Determine platform and architecture
const platform = os.platform();
const arch = os.arch();

// Map platform/arch to release asset names
const assetMap = {
  'darwin': {
    'arm64': `vtcode-${version}-aarch64-apple-darwin.tar.gz`,
    'x64': `vtcode-${version}-x86_64-apple-darwin.tar.gz`
  },
  'linux': {
    'arm64': `vtcode-${version}-aarch64-unknown-linux-gnu.tar.gz`,
    'x64': `vtcode-${version}-x86_64-unknown-linux-gnu.tar.gz`
  },
  'win32': {
    'x64': `vtcode-${version}-x86_64-pc-windows-msvc.zip`
  }
};

// Get the asset name for current platform
let assetName;
if (assetMap[platform] && assetMap[platform][arch]) {
  assetName = assetMap[platform][arch];
} else {
  console.error(`Unsupported platform/architecture: ${platform}/${arch}`);
  process.exit(1);
}

// GitHub release URL
const downloadUrl = `https://github.com/vinhnx/vtcode/releases/download/v${version}/${assetName}`;
const binDir = path.join(__dirname, '..', 'bin');
const tempPath = path.join(binDir, assetName);

// Ensure bin directory exists
if (!fs.existsSync(binDir)) {
  fs.mkdirSync(binDir, { recursive: true });
}

console.log(`Downloading VT Code v${version} for ${platform}/${arch}...`);

// Download function
function downloadFile(url, dest) {
  return new Promise((resolve, reject) => {
    const file = fs.createWriteStream(dest);
    
    https.get(url, (response) => {
      if (response.statusCode === 302 || response.statusCode === 301) {
        // Handle redirects
        downloadFile(response.headers.location, dest).then(resolve).catch(reject);
        return;
      }
      
      if (response.statusCode !== 200) {
        reject(new Error(`Download failed with status code: ${response.statusCode}`));
        return;
      }
      
      response.pipe(file);
      
      file.on('finish', () => {
        file.close();
        resolve();
      });
      
      file.on('error', (err) => {
        fs.unlink(dest, () => {}); // Delete partial file
        reject(err);
      });
    }).on('error', (err) => {
      fs.unlink(dest, () => {}); // Delete partial file
      reject(err);
    });
  });
}

// Extract function
function extractArchive(archivePath) {
  const binDir = path.join(__dirname, '..', 'bin');
  
  try {
    if (archivePath.endsWith('.tar.gz')) {
      execSync(`tar -xzf "${archivePath}" -C "${binDir}"`, { stdio: 'inherit' });
    } else if (archivePath.endsWith('.zip')) {
      if (platform === 'win32') {
        execSync(`powershell Expand-Archive -Path "${archivePath}" -DestinationPath "${binDir}" -Force`, { stdio: 'inherit' });
      } else {
        execSync(`unzip -o "${archivePath}" -d "${binDir}"`, { stdio: 'inherit' });
      }
    }
    
    // Clean up archive
    fs.unlinkSync(archivePath);
    
    // Find and rename the binary
    const files = fs.readdirSync(binDir);
    const binaryFile = files.find(file => file.startsWith('vtcode') && !file.includes('.') || file.endsWith('.exe'));
    
    if (binaryFile) {
      const oldPath = path.join(binDir, binaryFile);
      let newPath;
      
      switch (platform) {
        case 'darwin':
          newPath = path.join(binDir, arch === 'arm64' ? 'vtcode-macos-arm64' : 'vtcode-macos-x64');
          break;
        case 'linux':
          newPath = path.join(binDir, arch === 'arm64' ? 'vtcode-linux-arm64' : 'vtcode-linux-x64');
          break;
        case 'win32':
          newPath = path.join(binDir, 'vtcode-windows-x64.exe');
          break;
      }
      
      if (newPath) {
        fs.renameSync(oldPath, newPath);
        // Make it executable (not needed on Windows)
        if (platform !== 'win32') {
          fs.chmodSync(newPath, 0o755);
        }
      }
    }
    
    console.log('VT Code installed successfully!');
  } catch (error) {
    console.error('Extraction failed:', error.message);
    process.exit(1);
  }
}

// Main installation process
async function install() {
  try {
    // Download the binary
    await downloadFile(downloadUrl, tempPath);
    console.log('Download complete. Extracting...');
    
    // Extract the binary
    extractArchive(tempPath);
  } catch (error) {
    console.error('Installation failed:', error.message);
    
    // Try to provide alternative installation method
    console.log('\nYou can also install VT Code using one of these methods:');
    console.log('- Using Cargo: cargo install vtcode');
    console.log('- Using Homebrew (macOS): brew install vtcode');
    console.log('- Download binaries directly from: https://github.com/vinhnx/vtcode/releases');
    
    process.exit(1);
  }
}

// Run installation
install();
