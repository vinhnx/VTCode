#!/usr/bin/env node

/**
 * VT Code - npm package entry point
 * This serves as the main entry point for the npm package.
 * It delegates to the Rust binary which is downloaded during postinstall.
 */

const { spawn } = require('child_process');
const path = require('path');
const os = require('os');

// Determine the platform-specific binary path
const platform = os.platform();
const arch = os.arch();

let binaryName;
switch (platform) {
  case 'darwin':
    binaryName = arch === 'arm64' ? 'vtcode-macos-arm64' : 'vtcode-macos-x64';
    break;
  case 'linux':
    binaryName = arch === 'arm64' ? 'vtcode-linux-arm64' : 'vtcode-linux-x64';
    break;
  case 'win32':
    binaryName = 'vtcode-windows-x64.exe';
    break;
  default:
    console.error(`Unsupported platform: ${platform}`);
    process.exit(1);
}

const binaryPath = path.join(__dirname, 'bin', binaryName);

// Check if the binary exists
const fs = require('fs');
if (!fs.existsSync(binaryPath)) {
  console.error('VT Code binary not found. Please run npm install again.');
  console.error('If the problem persists, please install using cargo: cargo install vtcode');
  process.exit(1);
}

// Execute the binary with all passed arguments
const args = process.argv.slice(2);
const child = spawn(binaryPath, args, {
  stdio: 'inherit'
});

child.on('error', (err) => {
  console.error('Failed to start VT Code:', err.message);
  process.exit(1);
});

child.on('exit', (code) => {
  process.exit(code || 0);
});
