# Homebrew Release Process for VT Code

## Prerequisites

1. Create a GitHub repository for your Homebrew tap:
   - Repository name: `homebrew-tap` (under your GitHub username, e.g., `vinhnx/homebrew-tap`)
   - This repository should be public

2. Clone the tap repository:
   ```bash
   git clone https://github.com/vinhnx/homebrew-tap.git
   ```

3. Copy the formula file to your tap repository:
   ```bash
   cp /path/to/vtcode/homebrew/vtcode.rb /path/to/homebrew-tap/
   ```

## Releasing a New Version

After creating a GitHub release for VT Code:

1. Use the automated update script:
   ```bash
   ./scripts/update-homebrew-formula.sh <version> vinhnx/homebrew-tap
   ```

   This script will:
   - Download the macOS binaries from the GitHub release
   - Calculate SHA256 hashes for both Intel and ARM versions
   - Update the formula file with the new version and hashes
   - Commit and push the changes to your tap repository

2. Alternatively, update manually:
   - Update the version in `vtcode.rb`
   - Download the binaries and calculate their SHA256 hashes
   - Update the SHA256 values in the formula
   - Commit and push the changes

## Installation for Users

Users can install VT Code using your Homebrew tap:

```bash
brew install vtcode
```

## Formula Structure

The Homebrew formula (`vtcode.rb`) supports both Intel and ARM Macs:
- Intel Macs: Downloads `vtcode-v<version>-x86_64-apple-darwin.tar.gz`
- ARM Macs: Downloads `vtcode-v<version>-aarch64-apple-darwin.tar.gz`

## Testing

Test the formula locally before pushing:
```bash
brew install --build-from-source vtcode.rb
brew test vtcode
```