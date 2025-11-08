#!/usr/bin/env node

/**
 * Script to publish the package to npmjs.com (with different package name)
 * Usage: node scripts/publish-to-npmjs.js
 *
 * This script will:
 * 1. Check if NPM_TOKEN environment variable is set
 * 2. Modify package.json to use a different name for npmjs.com
 * 3. Run npm publish to npmjs.com
 */

const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

function checkEnvironment() {
  if (!process.env.NPM_TOKEN) {
    console.error('❌ Error: NPM_TOKEN environment variable is not set');
    console.error('Please set it before running this script:');
    console.error('export NPM_TOKEN=your_npm_access_token_here');
    console.error('');
    console.error('Make sure your npm access token has publish scope');
    process.exit(1);
  }

  console.log(' NPM_TOKEN environment variable is set');
}

function checkNpmrc() {
  const npmrcPath = path.join(__dirname, '../.npmrc');
  if (!fs.existsSync(npmrcPath)) {
    console.error('❌ Error: .npmrc file not found in npm directory');
    console.error('Please create one with the proper npmjs.com configuration');
    console.error('See .npmrc.example for reference');
    process.exit(1);
  }

  const npmrcContent = fs.readFileSync(npmrcPath, 'utf8');
  // Check for valid npmjs.com registry configuration
  const npmjsRegistryPattern = /^\/\/registry\.npmjs\.org\/?:_authToken=/m;
  let npmjsRegistryFound = false;
  
  for (const line of npmrcContent.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (
      trimmed &&
      !trimmed.startsWith('#') &&
      npmjsRegistryPattern.test(trimmed)
    ) {
      npmjsRegistryFound = true;
      break;
    }
  }
  
  if (!npmjsRegistryFound) {
    console.warn('⚠️  Warning: .npmrc file does not contain a valid npmjs.com registry configuration');
    console.warn('Please check that your .npmrc includes: //registry.npmjs.org/:_authToken=YOUR_TOKEN');
  } else {
    console.log(' .npmrc file contains npmjs.com configuration');
  }
}

function runPublish() {
  console.log('\n🚀 Starting publish process to npmjs.com...');

  // Create temporary directory and copy files
  const os = require('os');
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'vtcode-npm-'));
  const sourceDir = path.join(__dirname, '..');
  const files = fs.readdirSync(sourceDir);
  
  for (const file of files) {
    if (file !== 'node_modules') { // Don't copy node_modules
      const src = path.join(sourceDir, file);
      const dest = path.join(tempDir, file);
      if (fs.statSync(src).isDirectory()) {
        const { cp } = require('child_process');
        cp.execSync(`cp -r "${src}" "${dest}"`);
      } else {
        fs.copyFileSync(src, dest);
      }
    }
  }

  try {
    // Read the current package.json and modify the name
    const packageJsonPath = path.join(tempDir, 'package.json');
    let packageJson = JSON.parse(fs.readFileSync(packageJsonPath, 'utf8'));
    
    // Change the package name for npmjs.com (since 'vtcode' is taken)
    packageJson.name = 'vtcode-bin';
    
    // Remove the scoped registry config for npmjs.com publish
    delete packageJson.publishConfig;
    
    // Write the modified package.json
    fs.writeFileSync(packageJsonPath, JSON.stringify(packageJson, null, 2));
    
    console.log(` Package: ${packageJson.name} (v${packageJson.version})`);

    // Verify npm configuration
    console.log('\n📋 Checking npm configuration...');
    const npmWhoami = execSync('npm whoami', { encoding: 'utf8', cwd: tempDir }).trim();
    console.log(`👤 Authenticated as: ${npmWhoami}`);

    // Run npm publish to npmjs.com
    console.log('\n📦 Publishing to npmjs.com...');
    const publishOutput = execSync('npm publish', {
      cwd: tempDir,
      encoding: 'utf8'
    });

    console.log(' Publish output:');
    console.log(publishOutput);

    console.log('\n🎉 Package published successfully to npmjs.com!');
    console.log(`🔗 View at: https://www.npmjs.com/package/vtcode-bin`);
  } catch (error) {
    console.error('❌ Error during publish:');
    console.error(error.message);
    if (error.stdout) console.error('STDOUT:', error.stdout);
    if (error.stderr) console.error('STDERR:', error.stderr);
    process.exit(1);
  } finally {
    // Clean up temporary directory
    const { spawn } = require('child_process');
    const rmProcess = spawn('rm', ['-rf', tempDir]);
    rmProcess.on('close', () => {
      console.log(' Cleaned up temporary files.');
    });
  }
}

function main() {
  console.log('📝 Publishing VT Code npm package to npmjs.com (with different name)');
  console.log('=====================================================');

  checkEnvironment();
  checkNpmrc();

  console.log('\n📋 Ready to publish:');

  // Ask for confirmation
  const readline = require('readline');
  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout
  });

  rl.question('\n⚠️  Do you want to proceed with publishing to npmjs.com? (y/N): ', (answer) => {
    rl.close();

    if (answer.toLowerCase() !== 'y' && answer.toLowerCase() !== 'yes') {
      console.log('Publish cancelled.');
      process.exit(0);
    }

    runPublish();
  });
}

if (require.main === module) {
  main();
}
