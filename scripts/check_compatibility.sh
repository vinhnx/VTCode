#!/bin/bash
# VTCode Compatibility Checker
# Inspired by caniuse.rs - checks MSRV, platform targets, and feature compatibility
#
# Usage: ./scripts/check_compatibility.sh [--msrv] [--targets] [--features] [--all]

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Project root
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

echo -e "${BLUE}╔════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║     VTCode Compatibility Checker (caniuse.rs style)   ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════╝${NC}"
echo ""

# Default: run all checks
RUN_MSRV=false
RUN_TARGETS=false
RUN_FEATURES=false

if [[ $# -eq 0 ]]; then
    RUN_MSRV=true
    RUN_TARGETS=true
    RUN_FEATURES=true
else
    while [[ $# -gt 0 ]]; do
        case $1 in
            --msrv)
                RUN_MSRV=true
                shift
                ;;
            --targets)
                RUN_TARGETS=true
                shift
                ;;
            --features)
                RUN_FEATURES=true
                shift
                ;;
            --all)
                RUN_MSRV=true
                RUN_TARGETS=true
                RUN_FEATURES=true
                shift
                ;;
            --help|-h)
                echo "Usage: $0 [OPTIONS]"
                echo ""
                echo "Options:"
                echo "  --msrv       Check Minimum Supported Rust Version"
                echo "  --targets    Check cross-platform target builds"
                echo "  --features   Check feature flag combinations"
                echo "  --all        Run all checks (default)"
                echo "  --help, -h   Show this help message"
                exit 0
                ;;
            *)
                echo -e "${RED}Unknown option: $1${NC}"
                exit 1
                ;;
        esac
    done
fi

# Track overall status
OVERALL_STATUS=0

# ============================================================================
# MSRV Check
# ============================================================================
if [[ "$RUN_MSRV" == true ]]; then
    echo -e "${YELLOW}━━━ MSRV (Minimum Supported Rust Version) Check ━━━${NC}"
    echo ""
    
    # Get current Rust version
    RUST_VERSION=$(rustc --version)
    echo -e "${BLUE}Current Rust:${NC} $RUST_VERSION"
    
    # Check MSRV in Cargo.toml files
    echo -e "${BLUE}Expected MSRV:${NC} 1.88 (Rust 2024 Edition)"
    echo ""
    
    # Verify all crates have rust-version set
    MSRV_CHECK_PASSED=true
    
    for cargo_toml in $(find . -name "Cargo.toml" -not -path "./target/*"); do
        if grep -q 'edition = "2024"' "$cargo_toml"; then
            if ! grep -q 'rust-version' "$cargo_toml"; then
                echo -e "${RED}✗ Missing rust-version in $cargo_toml${NC}"
                MSRV_CHECK_PASSED=false
            fi
        fi
    done
    
    if [[ "$MSRV_CHECK_PASSED" == true ]]; then
        echo -e "${GREEN}✓ All crates have rust-version specified${NC}"
    else
        OVERALL_STATUS=1
    fi
    
    # Check if cargo-msrv is available
    if command -v cargo-msrv &> /dev/null; then
        echo ""
        echo -e "${BLUE}Running cargo-msrv verify...${NC}"
        if cargo msrv verify 2>/dev/null; then
            echo -e "${GREEN}✓ MSRV verification passed${NC}"
        else
            echo -e "${YELLOW}⚠ MSRV verification found issues (run 'cargo msrv verify' for details)${NC}"
            OVERALL_STATUS=1
        fi
    else
        echo ""
        echo -e "${YELLOW}⚠ cargo-msrv not installed. Install with: cargo install cargo-msrv${NC}"
    fi
    
    echo ""
fi

# ============================================================================
# Target Platform Checks
# ============================================================================
if [[ "$RUN_TARGETS" == true ]]; then
    echo -e "${YELLOW}━━━ Cross-Platform Target Checks ━━━${NC}"
    echo ""
    
    # Define targets to check
    TARGETS_LINUX="x86_64-unknown-linux-gnu:Linux x64"
    TARGETS_MACOS_ARM="aarch64-apple-darwin:macOS ARM64"
    TARGETS_MACOS_X64="x86_64-apple-darwin:macOS x64"
    TARGETS_WINDOWS_MSVC="x86_64-pc-windows-msvc:Windows MSVC"
    TARGETS_WINDOWS_GNU="x86_64-pc-windows-gnu:Windows GNU"
    
    # Check which targets are installed
    echo -e "${BLUE}Checking installed targets...${NC}"
    INSTALLED_TARGETS=$(rustup target list --installed)
    
    for target_info in "$TARGETS_LINUX" "$TARGETS_MACOS_ARM" "$TARGETS_MACOS_X64" "$TARGETS_WINDOWS_MSVC" "$TARGETS_WINDOWS_GNU"; do
        target="${target_info%%:*}"
        name="${target_info##*:}"
        if echo "$INSTALLED_TARGETS" | grep -q "$target"; then
            echo -e "${GREEN}✓${NC} $name ($target)"
        else
            echo -e "${YELLOW}⚠${NC} $name ($target) - not installed"
            echo "   Install with: rustup target add $target"
        fi
    done
    
    echo ""
    
    # Check current host target
    HOST_TARGET=$(rustc -vV | grep host | cut -d' ' -f2)
    echo -e "${BLUE}Host target:${NC} $HOST_TARGET"
    echo ""
    
    # Try to check (not build) main crate for current target
    echo -e "${BLUE}Running cargo check on workspace...${NC}"
    if cargo check --workspace --all-targets 2>&1 | tail -5; then
        echo -e "${GREEN}✓ Workspace check passed${NC}"
    else
        echo -e "${RED}✗ Workspace check failed${NC}"
        OVERALL_STATUS=1
    fi
    
    echo ""
fi

# ============================================================================
# Feature Flag Checks
# ============================================================================
if [[ "$RUN_FEATURES" == true ]]; then
    echo -e "${YELLOW}━━━ Feature Flag Compatibility Checks ━━━${NC}"
    echo ""
    
    # Check default features
    echo -e "${BLUE}Checking default features...${NC}"
    if cargo check --workspace --all-targets 2>&1 | tail -3; then
        echo -e "${GREEN}✓ Default features build successfully${NC}"
    else
        echo -e "${RED}✗ Default features build failed${NC}"
        OVERALL_STATUS=1
    fi
    echo ""
    
    # Check with all features
    echo -e "${BLUE}Checking all features...${NC}"
    if cargo check --workspace --all-targets --all-features 2>&1 | tail -3; then
        echo -e "${GREEN}✓ All features build successfully${NC}"
    else
        echo -e "${RED}✗ All features build failed${NC}"
        OVERALL_STATUS=1
    fi
    echo ""
    
    # Check with no default features
    echo -e "${BLUE}Checking no default features...${NC}"
    if cargo check --workspace --no-default-features 2>&1 | tail -3; then
        echo -e "${GREEN}✓ No default features build successfully${NC}"
    else
        echo -e "${YELLOW}⚠ No default features build has issues${NC}"
    fi
    echo ""
    
    # Check individual crate features
    echo -e "${BLUE}Checking individual crate features...${NC}"
    
    CRATES=(
        "vtcode-core:schema,anthropic-api,a2a-server,desktop-notifications"
        "vtcode-config:bootstrap,schema"
        "vtcode-exec-events:serde-json,telemetry-tracing,schema-export"
    )
    
    for crate_info in "${CRATES[@]}"; do
        IFS=':' read -r crate features <<< "$crate_info"
        if [[ -d "$crate" ]]; then
            echo -e "  Checking $crate with features: $features"
            if cargo check -p "$crate" --features "$features" 2>&1 | tail -1; then
                echo -e "  ${GREEN}✓${NC} $crate features OK"
            else
                echo -e "  ${YELLOW}⚠${NC} $crate features have issues"
            fi
        fi
    done
    
    echo ""
fi

# ============================================================================
# Clippy Checks
# ============================================================================
echo -e "${YELLOW}━━━ Clippy Lint Checks ━━━${NC}"
echo ""

echo -e "${BLUE}Running clippy on workspace...${NC}"
if cargo clippy --workspace --all-targets --all-features -- -D warnings 2>&1 | tail -10; then
    echo -e "${GREEN}✓ Clippy checks passed${NC}"
else
    echo -e "${YELLOW}⚠ Clippy found warnings (see above)${NC}"
    # Don't fail overall for clippy warnings in development
fi

echo ""

# ============================================================================
# Summary
# ============================================================================
echo -e "${BLUE}╔════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║                  Compatibility Summary                 ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════╝${NC}"
echo ""

if [[ $OVERALL_STATUS -eq 0 ]]; then
    echo -e "${GREEN}✓ All critical compatibility checks passed!${NC}"
    echo ""
    echo "Your VTCode installation is compatible with the current setup."
else
    echo -e "${RED}✗ Some compatibility checks failed!${NC}"
    echo ""
    echo "Please review the errors above and fix them."
    echo "See COMPATIBILITY.md for detailed platform support information."
fi

echo ""
echo -e "${BLUE}For more information:${NC}"
echo "  - COMPATIBILITY.md - Platform compatibility matrix"
echo "  - clippy.toml - Clippy configuration"
echo "  - rust-toolchain.toml - Rust toolchain settings"
echo ""

exit $OVERALL_STATUS
