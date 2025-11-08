#!/usr/bin/env node

/**
 * Script to publish the package to npmjs.com
 * Usage: node scripts/publish-to-npmjs.js
 *
 * This script will:
 * 1. Check if NPM_TOKEN environment variable is set
 * 2. Verify .npmrc configuration exists
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

function checkPackageJson() {
  const packageJsonPath = path.join(__dirname, '../package.json');
  if (!fs.existsSync(packageJsonPath)) {
    console.error('❌ Error: package.json not found in npm directory');
    process.exit(1);
  }

  const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, 'utf8'));
  console.log(` Package: ${packageJson.name} (v${packageJson.version})`);

  return packageJson;
}

function runPublish() {
  console.log('\n🚀 Starting publish process to npmjs.com...');

  try {
    // Verify npm configuration
    console.log('\n📋 Checking npm configuration...');
    const npmWhoami = execSync('npm whoami', { encoding: 'utf8' }).trim();
    console.log(`👤 Authenticated as: ${npmWhoami}`);

    // Run npm publish to npmjs.com
    console.log('\n📦 Publishing to npmjs.com...');
    const publishOutput = execSync('npm publish', {
      cwd: path.join(__dirname, '..'),
      encoding: 'utf8'
    });

    console.log(' Publish output:');
    console.log(publishOutput);

    console.log('\n🎉 Package published successfully to npmjs.com!');
    console.log(`🔗 View at: https://www.npmjs.com/package/vtcode`);
  } catch (error) {
    console.error('❌ Error during publish:');
    console.error(error.message);
    if (error.stdout) console.error('STDOUT:', error.stdout);
    if (error.stderr) console.error('STDERR:', error.stderr);
    process.exit(1);
  }
}

function main() {
  console.log('📝 Publishing VT Code npm package to npmjs.com');
  console.log('=====================================================');

  checkEnvironment();
  checkNpmrc();
  const packageJson = checkPackageJson();

  console.log('\n📋 Verification complete. Ready to publish:');
  console.log(`   - Package: ${packageJson.name}`);
  console.log(`   - Version: ${packageJson.version}`);
  console.log(`   - Registry: npmjs.com (configured in .npmrc)`);

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
