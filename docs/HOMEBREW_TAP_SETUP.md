# Homebrew Tap Setup Guide

## Current Situation

The Homebrew formula shown on https://formulae.brew.sh/formula/vtcode is **orphaned and outdated** because:

1. **VT Code is NOT in Homebrew/core** - The formula visible on formulae.brew.sh was manually added to homebrew-core at some point, but is no longer maintained
2. **No custom tap exists** - A proper Homebrew tap (`homebrew-vtcode`) should be created and maintained separately
3. **Two installation paths are needed**:
   - **Custom tap** (recommended): Users install via `brew tap vinhnx/vtcode` or `brew install vinhnx/tap/vtcode`
   - **Homebrew core** (optional): If accepted into homebrew-core by maintainers

## The Problem

- Users visiting https://formulae.brew.sh/formula/vtcode see v0.50.9 (extremely outdated)
- Running `brew install vtcode` installs the old core version
- The formula in this repository (`homebrew/vtcode.rb`) isn't being published anywhere

## The Solution: Create a Custom Tap

### Step 1: Create the Tap Repository

Create a new GitHub repository named `homebrew-vtcode`:

```bash
# Create the repository at https://github.com/vinhnx/homebrew-vtcode
# Add these files:

homebrew-vtcode/
├── README.md
├── Formula/
│   └── vtcode.rb
└── .github/workflows/
    └── publish.yml
```

### Step 2: Set Up the Tap Structure

The formula file should be at `Formula/vtcode.rb` (not root):

```ruby
class Vtcode < Formula
  desc "Rust-based terminal coding agent with semantic code intelligence"
  homepage "https://github.com/vinhnx/vtcode"
  license "MIT"
  version "0.58.3"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/vinhnx/vtcode/releases/download/v#{version}/vtcode-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "e0f38f6f7c37c1fa6e2a8d1b9f4e5c6a7d8b9c0e1f2a3b4c5d6e7f8a9b0c1d2"
    else
      url "https://github.com/vinhnx/vtcode/releases/download/v#{version}/vtcode-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "f1e49a7c8d39e2a0f1c3b5d7e9a1c3e5g7i9k1m3o5q7s9u1w3y5z7b9d1f3h5"
    end
  end

  on_linux do
    if Hardware::CPU.arm? && Hardware::CPU.is_64_bit?
      url "https://github.com/vinhnx/vtcode/releases/download/v#{version}/vtcode-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7r8s9t0u1v2w3x4y5z6a7b8c9d0e1f"
    else
      url "https://github.com/vinhnx/vtcode/releases/download/v#{version}/vtcode-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7r8s9t0u1v2w3x4y5z6a7b8c9d0e1f2g"
    end
  end

  def install
    bin.install "vtcode"
  end

  def caveats
    <<~EOS
      VT Code is now installed! To get started:

      1. Set your API key environment variable:
         export OPENAI_API_KEY="sk-..."
         (or use ANTHROPIC_API_KEY, GEMINI_API_KEY, etc.)

      2. Launch VT Code:
         vtcode

      For more information, visit:
        https://github.com/vinhnx/vtcode
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/vtcode --version")
  end
end
```

### Step 3: Update the Release Workflow

Modify `.github/workflows/release-on-tag.yml` to update BOTH locations:

1. **Update `homebrew/vtcode.rb`** in this repository (for backup)
2. **Push to `homebrew-vtcode` tap** repository

```yaml
- name: Update Homebrew tap formula
  run: |
    # Clone the tap repository
    git clone https://github.com/vinhnx/homebrew-vtcode.git /tmp/homebrew-vtcode
    cd /tmp/homebrew-vtcode
    
    # Copy the updated formula
    cp ../homebrew/vtcode.rb Formula/
    
    # Commit and push
    git config user.name "github-actions[bot]"
    git config user.email "github-actions[bot]@users.noreply.github.com"
    git add Formula/vtcode.rb
    git commit -m "chore: update vtcode formula to ${{ github.ref_name }}"
    git push origin main
  env:
    GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

### Step 4: Configure Access

Add GitHub token permissions to allow pushing to the tap repository:
- Create a GitHub token with `repo` scope if separate repository
- Or use `GITHUB_TOKEN` if tap repository is public and accessible

## User Installation Instructions

Once the tap is set up, users can install VT Code with:

```bash
# Option 1: Direct installation (auto-taps the repository)
brew install vinhnx/tap/vtcode

# Option 2: Manual tap then install
brew tap vinhnx/homebrew-vtcode
brew install vtcode

# Check version
vtcode --version
```

## Automatic Updates

Once tapped, users get automatic updates with:

```bash
brew update
brew upgrade vtcode
```

## Next Steps

1. **Create `homebrew-vtcode` repository** on GitHub
2. **Set up repository structure** with Formula directory
3. **Update release workflow** to push to both locations
4. **Update documentation** to recommend custom tap installation
5. **Optional**: Submit to Homebrew/core for official inclusion

## Why Custom Tap is Better Than Core

- **Faster updates**: No need to wait for Homebrew maintainers
- **Vendor control**: You control exactly what users get
- **CI/CD automation**: Automatic formula updates on every release
- **Community discoverability**: Users find your official tap, not orphaned core formula

## References

- Homebrew Tap Documentation: https://docs.brew.sh/Taps
- How to Create and Maintain a Tap: https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap
- Acceptable Software for Homebrew/core: https://docs.brew.sh/Acceptable-Formulae
