#!/usr/bin/env node

/**
 * Preuninstall script for VT Code npm package
 * Cleans up downloaded binaries
 */

const fs = require('fs');
const path = require('path');

const binDir = path.join(__dirname, '..', 'bin');

console.log('Cleaning up VT Code binaries...');

// Remove all files in the bin directory
try {
  if (fs.existsSync(binDir)) {
    const files = fs.readdirSync(binDir);
    for (const file of files) {
      const filePath = path.join(binDir, file);
      fs.unlinkSync(filePath);
    }
    fs.rmdirSync(binDir);
  }
  console.log('Cleanup complete.');
} catch (error) {
  console.error('Cleanup failed:', error.message);
}
