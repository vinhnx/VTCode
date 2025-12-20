#!/bin/bash

# VT Code NPM Authentication Setup Script
# This script helps set up npm authentication for publishing

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

print_info() {
    printf '%b\n' "${BLUE}INFO:${NC} $1"
}

print_success() {
    printf '%b\n' "${GREEN}SUCCESS:${NC} $1"
}

print_warning() {
    printf '%b\n' "${YELLOW}WARNING:${NC} $1"
}

print_error() {
    printf '%b\n' "${RED}ERROR:${NC} $1"
}

echo "🚀 VT Code NPM Authentication Setup"
echo "===================================="
echo ""

# Check current authentication status
print_info "Checking current npm authentication..."
echo ""

echo "1. npmjs.com status:"
if npm whoami --registry https://registry.npmjs.org/ >/dev/null 2>&1; then
    NPM_USER=$(npm whoami --registry https://registry.npmjs.org/)
    print_success "Logged in to npmjs.com as: $NPM_USER"
else
    print_warning "Not logged in to npmjs.com"
fi
echo ""

echo "2. GitHub Packages status:"
if npm whoami --registry https://npm.pkg.github.com/ >/dev/null 2>&1; then
    GITHUB_USER=$(npm whoami --registry https://npm.pkg.github.com/)
    print_success "Logged in to GitHub Packages as: $GITHUB_USER"
else
    print_warning "Not logged in to GitHub Packages"
fi
echo ""

# Offer to set up authentication
print_info "Select authentication method:"
echo ""
echo "A) npmjs.com: Trusted Publishing (CI/CD only - requires GitHub repo config)"
echo "B) npmjs.com: NPM_TOKEN environment variable"
echo "C) npmjs.com: npm login (interactive)"
echo "D) GitHub Packages: GITHUB_TOKEN environment variable"
echo "E) GitHub Packages: gh CLI login (interactive)"
echo "F) Skip authentication setup"
echo ""
read -p "Enter your choice (A-F): " choice
echo ""

case $choice in
    A|a)
        print_info "Trusted Publishing setup instructions:"
        echo ""
        echo "1. Go to https://www.npmjs.com/settings/integrations"
        echo "2. Add your GitHub repository (vinhnx/vtcode) as a trusted publisher"
        echo "3. In your GitHub Actions workflow, ensure you have:"
        echo "   permissions:"
        echo "     contents: write"
        echo "     packages: write"
        echo "     id-token: write"
        echo ""
        print_success "No environment variables needed - trusted publishing uses OIDC"
        ;;
    B|b)
        echo "Enter your npm access token (from https://www.npmjs.com/settings/tokens):"
        read -s NPM_TOKEN
        echo ""
        if [[ -z "$NPM_TOKEN" ]]; then
            print_error "No token provided"
            exit 1
        fi
        echo "//registry.npmjs.org/:_authToken=${NPM_TOKEN}" >> ~/.npmrc
        print_success "Added NPM token to ~/.npmrc"
        echo ""
        print_info "You can also use environment variable:"
        echo "export NPM_TOKEN=${NPM_TOKEN:0:10}..."
        ;;
    C|c)
        print_info "Running npm login..."
        npm login
        if npm whoami >/dev/null 2>&1; then
            print_success "Successfully logged in to npm"
        else
            print_error "Login failed"
            exit 1
        fi
        ;;
    D|d)
        echo "Enter your GitHub Personal Access Token (from https://github.com/settings/tokens):"
        read -s GITHUB_TOKEN
        echo ""
        if [[ -z "$GITHUB_TOKEN" ]]; then
            print_error "No token provided"
            exit 1
        fi
        echo "//npm.pkg.github.com/:_authToken=${GITHUB_TOKEN}" >> ~/.npmrc
        print_success "Added GitHub token to ~/.npmrc"
        echo ""
        print_info "You can also use environment variable:"
        echo "export GITHUB_TOKEN=${GITHUB_TOKEN:0:10}..."
        ;;
    E|e)
        print_info "Setting up GitHub CLI authentication..."
        if ! command -v gh &>/dev/null; then
            print_error "GitHub CLI (gh) not found. Install it first: https://cli.github.com/"
            exit 1
        fi
        gh auth login
        TOKEN=$(gh auth token)
        npm config set @vinhnx:registry https://npm.pkg.github.com
        npm config set -- //npm.pkg.github.com/:_authToken=$TOKEN
        print_success "GitHub CLI authentication configured"
        ;;
    F|f)
        print_info "Skipping authentication setup"
        ;;
    *)
        print_error "Invalid choice"
        exit 1
        ;;
esac

echo ""
print_info "Testing authentication..."
echo ""

if npm whoami --registry https://registry.npmjs.org/ >/dev/null 2>&1; then
    print_success "✓ npmjs.com authentication working"
else
    print_warning "✗ npmjs.com authentication not configured"
fi

if npm whoami --registry https://npm.pkg.github.com/ >/dev/null 2>&1; then
    print_success "✓ GitHub Packages authentication working"
else
    print_warning "✗ GitHub Packages authentication not configured"
fi

echo ""
print_success "Setup complete!"
echo ""
print_info "To publish, run:"
echo "  ./scripts/release.sh --patch"
echo ""
print_info "For more help, see: npm/TROUBLESHOOTING.md"
