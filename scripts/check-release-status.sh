#!/usr/bin/env bash

# Quick status checker for VT Code v0.58.6 release

REPO="vinhnx/vtcode"
RELEASE_TAG="v0.58.6"
GITHUB_API="https://api.github.com/repos/$REPO/releases/tags/$RELEASE_TAG"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
MAGENTA='\033[0;35m'
NC='\033[0m'

# Get platform
get_platform() {
    case "$(uname -s)-$(uname -m)" in
        Darwin-arm64)
            echo "aarch64-apple-darwin|macOS Apple Silicon|vtcode-v0.58.6-aarch64-apple-darwin.tar.gz"
            ;;
        Darwin-x86_64)
            echo "x86_64-apple-darwin|macOS Intel|vtcode-v0.58.6-x86_64-apple-darwin.tar.gz"
            ;;
        Linux-x86_64)
            echo "x86_64-unknown-linux-gnu|Linux|vtcode-v0.58.6-x86_64-unknown-linux-gnu.tar.gz"
            ;;
        MINGW*-x86_64|MSYS*-x86_64)
            echo "x86_64-pc-windows-msvc|Windows|vtcode-v0.58.6-x86_64-pc-windows-msvc.zip"
            ;;
        *)
            echo "unknown|Unknown|unknown"
            return 1
            ;;
    esac
}

printf '%b\n' "${MAGENTA}════════════════════════════════════════════════════${NC}"
printf '%b\n' "${MAGENTA}  VT Code v0.58.6 Release Status${NC}"
printf '%b\n' "${MAGENTA}════════════════════════════════════════════════════${NC}"
echo ""

# Get platform
PLATFORM_INFO=$(get_platform)
ARCH=$(echo "$PLATFORM_INFO" | cut -d'|' -f1)
DESC=$(echo "$PLATFORM_INFO" | cut -d'|' -f2)
BINARY_NAME=$(echo "$PLATFORM_INFO" | cut -d'|' -f3)
echo "Your Platform: $DESC"
echo "Binary Name:   $BINARY_NAME"
echo ""

# Fetch release info
printf '%b' "${BLUE}ℹ${NC} Checking GitHub API... "
RESPONSE=$(curl -fsSL "$GITHUB_API" 2>/dev/null || echo "")

if [[ -z "$RESPONSE" ]]; then
    printf '%b\n' "${RED}Failed to connect${NC}"
    echo ""
    echo "Cannot reach GitHub API. Check your internet connection."
    exit 1
fi

printf '%b\n' "${GREEN}Done${NC}"
echo ""

# Parse response
RELEASE_STATE=$(echo "$RESPONSE" | grep -o '"draft":[^,]*' | cut -d: -f2 | tr -d ' ')
CREATED_AT=$(echo "$RESPONSE" | grep -o '"created_at":"[^"]*' | cut -d'"' -f4)
PUBLISHED_AT=$(echo "$RESPONSE" | grep -o '"published_at":"[^"]*' | cut -d'"' -f4)

# Count assets
TOTAL_ASSETS=$(echo "$RESPONSE" | grep -o '"name": "vtcode-' | wc -l)
HAS_CHECKSUMS=$(echo "$RESPONSE" | grep -q '"name": "checksums.txt"' && echo "yes" || echo "no")
HAS_YOUR_BINARY=$(echo "$RESPONSE" | grep -q "\"name\": \"$BINARY_NAME\"" && echo "yes" || echo "no")

# Display status
printf '%b\n' "${BLUE}Release Status:${NC}"
if [[ "$RELEASE_STATE" == "true" ]]; then
    printf '%b\n' "  Draft:        ${YELLOW}Yes (still being prepared)${NC}"
else
    printf '%b\n' "  Draft:        ${GREEN}No (published)${NC}"
fi

printf '%b\n' "  Created:      $CREATED_AT"
if [[ -n "$PUBLISHED_AT" && "$PUBLISHED_AT" != "null" ]]; then
    printf '%b\n' "  Published:    $PUBLISHED_AT"
fi
echo ""

printf '%b\n' "${BLUE}Assets:${NC}"
printf '%b\n' "  Total Binaries:    $TOTAL_ASSETS/4"
printf '%b' "  Your Binary:       "
if [[ "$HAS_YOUR_BINARY" == "yes" ]]; then
    printf '%b\n' "${GREEN}✓ Available${NC}"
else
    printf '%b\n' "${YELLOW}⏳ Building...${NC}"
fi

printf '%b' "  Checksums:       "
if [[ "$HAS_CHECKSUMS" == "yes" ]]; then
    printf '%b\n' "${GREEN}✓ Available${NC}"
else
    printf '%b\n' "${YELLOW}⏳ Generating...${NC}"
fi
echo ""

# Overall status
if [[ "$HAS_YOUR_BINARY" == "yes" && "$HAS_CHECKSUMS" == "yes" ]]; then
    printf '%b\n' "${GREEN}✓ READY TO INSTALL${NC}"
    echo ""
    echo "Install VT Code with:"
    echo "  curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash"
    echo ""
    echo "Or auto-wait and install:"
    echo "  ./scripts/wait-for-release.sh -a"
    exit 0
elif [[ "$HAS_YOUR_BINARY" == "yes" ]]; then
    printf '%b\n' "${YELLOW}⏳ ALMOST READY${NC}"
    echo ""
    echo "Binary is built, waiting for checksums.txt to be generated..."
    echo ""
    echo "Monitor progress:"
    echo "  ./scripts/wait-for-release.sh -a"
    exit 1
else
    printf '%b\n' "${YELLOW}⏳ BUILDING${NC}"
    echo ""
    echo "Binaries are still being built. Check back in 5-10 minutes."
    echo ""
    echo "Monitor progress:"
    echo "  ./scripts/wait-for-release.sh -a"
    echo ""
    echo "Or view workflows:"
    echo "  https://github.com/vinhnx/vtcode/actions"
    exit 1
fi
