#!/usr/bin/env node

/**
 * Script to publish the package to GitHub Packages
 * Usage: node scripts/publish-to-github.js
 *
 * This script will:
 * 1. Check if GITHUB_TOKEN environment variable is set
 * 2. Verify .npmrc configuration exists
 * 3. Run npm publish
 */

const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

function checkEnvironment() {
  if (!process.env.GITHUB_TOKEN) {
    console.error('❌ Error: GITHUB_TOKEN environment variable is not set');
    console.error('Please set it before running this script:');
    console.error('export GITHUB_TOKEN=your_github_personal_access_token_here');
    console.error('');
    console.error('Make sure your GitHub personal access token has the required scopes:');
    console.error('  - write:packages (to publish packages)');
    console.error('  - read:packages (to download packages)');
    console.error('  - repo (to link packages to your repositories)');
    process.exit(1);
  }

  console.log(' GITHUB_TOKEN environment variable is set');
}

function checkNpmrc() {
  const npmrcPath = path.join(__dirname, '../.npmrc');
  if (!fs.existsSync(npmrcPath)) {
    console.error('❌ Error: .npmrc file not found in npm directory');
    console.error('Please create one with the proper GitHub Packages configuration');
    console.error('See .npmrc.example for reference');
    process.exit(1);
  }

  const npmrcContent = fs.readFileSync(npmrcPath, 'utf8');
  if (!npmrcContent.includes('npm.pkg.github.com')) {
    console.warn('⚠️  Warning: .npmrc file does not contain GitHub Packages registry configuration');
    console.warn('Please check that your .npmrc includes: @vinhnx:registry=https://npm.pkg.github.com');
  } else {
    console.log(' .npmrc file contains GitHub Packages configuration');
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
  console.log('\n🚀 Starting publish process...');

  try {
    // Verify npm configuration
    console.log('\n📋 Checking npm configuration...');
    const npmWhoami = execSync('npm whoami', { encoding: 'utf8' }).trim();
    console.log(`👤 Authenticated as: ${npmWhoami}`);

    // Run npm publish
    console.log('\n📦 Publishing to GitHub Packages...');
    const publishOutput = execSync('npm publish', {
      cwd: path.join(__dirname, '..'),
      encoding: 'utf8'
    });

    console.log(' Publish output:');
    console.log(publishOutput);

    console.log('\n🎉 Package published successfully to GitHub Packages!');
    console.log(`🔗 View at: https://github.com/vinhnx/vtcode/pkgs/npm/vtcode`);
  } catch (error) {
    console.error('❌ Error during publish:');
    console.error(error.message);
    if (error.stdout) console.error('STDOUT:', error.stdout);
    if (error.stderr) console.error('STDERR:', error.stderr);
    process.exit(1);
  }
}

function main() {
  console.log('📝 Publishing VT Code npm package to GitHub Packages');
  console.log('=====================================================');

  checkEnvironment();
  checkNpmrc();
  const packageJson = checkPackageJson();

  console.log('\n📋 Verification complete. Ready to publish:');
  console.log(`   - Package: ${packageJson.name}`);
  console.log(`   - Version: ${packageJson.version}`);
  console.log(`   - Registry: GitHub Packages (configured in .npmrc)`);

  // Ask for confirmation
  const readline = require('readline');
  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout
  });

  rl.question('\n⚠️  Do you want to proceed with publishing? (y/N): ', (answer) => {
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