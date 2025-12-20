#!/bin/bash

# Verification script for release.sh and build-and-upload-binaries.sh updates
# This script verifies that the enhanced GitHub account checks and upload functionality work correctly

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Counters
TESTS_PASSED=0
TESTS_FAILED=0

print_header() {
    echo ""
    echo "=========================================="
    echo "$1"
    echo "=========================================="
    echo ""
}

print_success() {
    echo -e "${GREEN}✓${NC} $1"
    TESTS_PASSED=$((TESTS_PASSED + 1))
}

print_error() {
    echo -e "${RED}✗${NC} $1"
    TESTS_FAILED=$((TESTS_FAILED + 1))
}

print_info() {
    echo -e "${BLUE}ℹ${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}⚠${NC} $1"
}

# Test 1: Check if scripts exist
test_scripts_exist() {
    print_header "Test 1: Checking if required scripts exist"
    
    if [[ -f "scripts/release.sh" ]]; then
        print_success "scripts/release.sh exists"
    else
        print_error "scripts/release.sh not found"
    fi
    
    if [[ -f "scripts/build-and-upload-binaries.sh" ]]; then
        print_success "scripts/build-and-upload-binaries.sh exists"
    else
        print_error "scripts/build-and-upload-binaries.sh not found"
    fi
    
    if [[ -f "npm/bin/vtcode" ]]; then
        print_success "npm/bin/vtcode stub exists"
    else
        print_error "npm/bin/vtcode stub not found"
    fi
}

# Test 2: Check GitHub CLI
test_github_cli() {
    print_header "Test 2: Checking GitHub CLI"
    
    if command -v gh &> /dev/null; then
        print_success "GitHub CLI (gh) is installed"
        
        if gh auth status >/dev/null 2>&1; then
            print_success "GitHub CLI is authenticated"
            
            # Check if we're logged in to vinhnx and it's active
            if gh auth status 2>&1 | grep -q "Logged in to github.com account vinhnx"; then
                if gh auth status 2>&1 | grep -A 5 "account vinhnx" | grep -q "Active account: true"; then
                    print_success "Correct GitHub account active (vinhnx)"
                else
                    print_error "GitHub account 'vinhnx' is logged in but not active"
                    print_info "Run: gh auth switch --hostname github.com --user vinhnx"
                fi
            else
                print_error "Not logged in to GitHub account: vinhnx"
                print_info "Run: gh auth login --hostname github.com"
            fi
        else
            print_error "GitHub CLI is not authenticated. Run: gh auth login"
        fi
    else
        print_error "GitHub CLI (gh) is not installed"
    fi
}

# Test 3: Check npm package structure
test_npm_package() {
    print_header "Test 3: Checking npm package structure"
    
    if [[ -f "npm/package.json" ]]; then
        print_success "npm/package.json exists"
        
        # Check if package.json has bin field
        if jq -e '.bin' npm/package.json >/dev/null 2>&1; then
            print_success "package.json has 'bin' field"
            
            local bin_path=$(jq -r '.bin.vtcode' npm/package.json 2>/dev/null || echo "")
            if [[ "$bin_path" == "bin/vtcode" ]]; then
                print_success "package.json bin field points to bin/vtcode"
            else
                print_error "package.json bin field incorrect: $bin_path"
            fi
        else
            print_error "package.json missing 'bin' field"
        fi
        
        # Check if files array includes bin/
        if jq -e '.files | index("bin/")' npm/package.json >/dev/null 2>&1; then
            print_success "package.json files array includes bin/"
        else
            print_error "package.json files array doesn't include bin/"
        fi
    else
        print_error "npm/package.json not found"
    fi
    
    # Check if bin directory exists
    if [[ -d "npm/bin" ]]; then
        print_success "npm/bin directory exists"
    else
        print_error "npm/bin directory not found"
    fi
    
    # Check if bin/vtcode exists and is executable
    if [[ -f "npm/bin/vtcode" ]]; then
        print_success "npm/bin/vtcode file exists"
        
        if [[ -x "npm/bin/vtcode" ]]; then
            print_success "npm/bin/vtcode is executable"
        else
            print_error "npm/bin/vtcode is not executable"
        fi
        
        # Check file size (should be ~1KB)
        local size=$(wc -c < "npm/bin/vtcode")
        if [[ $size -gt 100 && $size -lt 2000 ]]; then
            print_success "npm/bin/vtcode has reasonable size ($size bytes)"
        else
            print_warning "npm/bin/vtcode has unexpected size ($size bytes)"
        fi
    else
        print_error "npm/bin/vtcode file not found"
    fi
}

# Test 4: Test npm package validation
test_npm_validation() {
    print_header "Test 4: Testing npm package validation"
    
    cd npm
    
    if npm pack --dry-run 2>&1 | grep -qi "warning\|error"; then
        print_warning "npm pack produced warnings/errors"
        npm pack --dry-run 2>&1 | grep -i "warning\|error"
    else
        print_success "npm pack validation passed (no warnings/errors)"
    fi
    
    # Check if bin/vtcode is included in package
    if npm pack --dry-run 2>&1 | grep -q "bin/vtcode"; then
        print_success "bin/vtcode is included in npm package"
    else
        print_error "bin/vtcode not found in npm package contents"
    fi
    
    cd ..
}

# Test 5: Check release script for key improvements
test_release_script() {
    print_header "Test 5: Checking release.sh enhancements"
    
    # Check for GitHub account verification
    if grep -q "expected_account.*vinhnx" scripts/release.sh; then
        print_success "release.sh has GitHub account verification"
    else
        print_error "release.sh missing GitHub account verification"
    fi
    
    # Check for release existence verification
    if grep -q "Verifying GitHub release" scripts/release.sh; then
        print_success "release.sh has release existence verification"
    else
        print_error "release.sh missing release existence verification"
    fi
    
    # Check for retry logic
    if grep -q "retry_count" scripts/release.sh; then
        print_success "release.sh has retry logic for release verification"
    else
        print_error "release.sh missing retry logic"
    fi
    
    # Check for npm bin stub creation
    if grep -q "stub not found, creating" scripts/release.sh; then
        print_success "release.sh has automatic npm bin stub creation"
    else
        print_error "release.sh missing automatic npm bin stub creation"
    fi
}

# Test 6: Check build script for key improvements
test_build_script() {
    print_header "Test 6: Checking build-and-upload-binaries.sh enhancements"
    
    # Check for GitHub account verification
    if grep -q "expected_account.*vinhnx" scripts/build-and-upload-binaries.sh; then
        print_success "build-and-upload-binaries.sh has GitHub account verification"
    else
        print_error "build-and-upload-binaries.sh missing GitHub account verification"
    fi
    
    # Check for batch upload
    if grep -q "files_to_upload.*sha256" scripts/build-and-upload-binaries.sh; then
        print_success "build-and-upload-binaries.sh uploads checksum files"
    else
        print_error "build-and-upload-binaries.sh missing checksum file upload"
    fi
    
    # Check for upload verification
    if grep -q "Verifying uploaded assets" scripts/build-and-upload-binaries.sh; then
        print_success "build-and-upload-binaries.sh verifies upload success"
    else
        print_error "build-and-upload-binaries.sh missing upload verification"
    fi
    
    # Check for release existence check
    if grep -q "Verify the release exists" scripts/build-and-upload-binaries.sh; then
        print_success "build-and-upload-binaries.sh checks release existence"
    else
        print_error "build-and-upload-binaries.sh missing release existence check"
    fi
}

# Test 7: Test GitHub release access
test_github_release() {
    print_header "Test 7: Testing GitHub release access"
    
    # Check if we can view releases
    if gh release list --limit 1 >/dev/null 2>&1; then
        print_success "Can list GitHub releases"
        
        # Show latest release
        local latest=$(gh release list --limit 1 --json tagName -q '.[0].tagName' 2>/dev/null || echo "unknown")
        print_info "Latest release: $latest"
        
        # Check if latest release has assets
        if [[ "$latest" != "unknown" ]]; then
            local asset_count=$(gh release view "$latest" --json assets -q '.assets | length' 2>/dev/null || echo "0")
            print_info "Assets in latest release: $asset_count"
            
            if [[ $asset_count -gt 0 ]]; then
                print_success "Latest release has assets"
            else
                print_warning "Latest release has no assets"
            fi
        fi
    else
        print_error "Cannot access GitHub releases"
    fi
}

# Test 8: Simulate account switch
test_account_switch() {
    print_header "Test 8: Testing account switch command"
    
    print_info "To switch to the correct account, run:"
    echo ""
    echo "  gh auth switch --hostname github.com --user vinhnx"
    echo ""
    print_info "If that doesn't work, re-authenticate:"
    echo ""
    echo "  gh auth login --hostname github.com"
    echo ""
    
    # Show current account
    if command -v gh &> /dev/null && gh auth status >/dev/null 2>&1; then
        local account=$(gh auth status --hostname github.com 2>&1 | grep "Active account:" | awk '{print $3}' || echo "unknown")
        print_info "Current account: $account"
    fi
}

# Run all tests
run_all_tests() {
    print_header "VERIFYING RELEASE SCRIPT SETUP"
    print_info "This script verifies that all release script enhancements are properly configured"
    echo ""
    
    test_scripts_exist
    test_github_cli
    test_npm_package
    test_npm_validation
    test_release_script
    test_build_script
    test_github_release
    test_account_switch
    
    # Summary
    print_header "TEST SUMMARY"
    echo "Tests passed: $TESTS_PASSED"
    echo -e "Tests failed: ${RED}$TESTS_FAILED${NC}"
    echo ""
    
    if [[ $TESTS_FAILED -eq 0 ]]; then
        print_success "All verification tests passed!"
        echo ""
        print_info "The release scripts are properly configured and ready to use."
        exit 0
    else
        print_error "Some verification tests failed."
        echo ""
        print_info "Please review the errors above and fix any issues before running a release."
        print_info "Common fixes:"
        echo "  - Switch to correct GitHub account: gh auth switch --hostname github.com --user vinhnx"
        echo "  - Ensure npm/bin/vtcode exists and is executable"
        echo "  - Check that GitHub CLI is installed and authenticated"
        exit 1
    fi
}

# Show help
if [[ "$1" == "--help" || "$1" == "-h" ]]; then
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Verify that release script enhancements are properly configured"
    echo ""
    echo "Options:"
    echo "  --help, -h    Show this help message"
    echo ""
    echo "This script checks:"
    echo "  ✓ Required scripts exist"
    echo "  ✓ GitHub CLI is installed and using correct account (vinhnx)"
    echo "  ✓ npm package structure is valid"
    echo "  ✓ npm package passes validation (no warnings)"
    echo "  ✓ release.sh has all enhancements"
    echo "  ✓ build-and-upload-binaries.sh has all enhancements"
    echo "  ✓ GitHub release access works"
    exit 0
fi

# Run tests
run_all_tests
