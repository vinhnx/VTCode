#!/usr/bin/env bash

# Quick status checker for VT Code release assets

# Source common utilities
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

REPO="vinhnx/vtcode"

RELEASE_TAG=$(get_current_version)

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -t|--tag)
            RELEASE_TAG=$2
            shift 2
            ;;
        *)
            shift
            ;;
    esac
done

GITHUB_API="https://api.github.com/repos/$REPO/releases/tags/$RELEASE_TAG"

# Get platform
get_platform() {
    case "$(uname -s)-$(uname -m)" in
        Darwin-arm64)
            echo "aarch64-apple-darwin|macOS Apple Silicon|vtcode-$RELEASE_TAG-aarch64-apple-darwin.tar.gz"
            ;;
        Darwin-x86_64)
            echo "x86_64-apple-darwin|macOS Intel|vtcode-$RELEASE_TAG-x86_64-apple-darwin.tar.gz"
            ;;
        Linux-x86_64)
            echo "x86_64-unknown-linux-musl|Linux|vtcode-$RELEASE_TAG-x86_64-unknown-linux-musl.tar.gz"
            ;;
        MINGW*-x86_64|MSYS*-x86_64)
            echo "x86_64-pc-windows-msvc|Windows|vtcode-$RELEASE_TAG-x86_64-pc-windows-msvc.zip"
            ;;
        *)
            echo "unknown|Unknown|unknown"
            return 1
            ;;
    esac
}

printf '%b\n' "${MAGENTA}════════════════════════════════════════════════════${NC}"
printf '%b\n' "${MAGENTA}  VT Code $RELEASE_TAG Release Status${NC}"
printf '%b\n' "${MAGENTA}════════════════════════════════════════════════════${NC}"
echo ""

# Get platform
PLATFORM_INFO=$(get_platform)
ARCH=$(echo "$PLATFORM_INFO" | cut -d'|' -f1)
DESC=$(echo "$PLATFORM_INFO" | cut -d'|' -f2)
BINARY_NAME=$(echo "$PLATFORM_INFO" | cut -d'|' -f3)
FALLBACK_BINARY=""
if [[ "$ARCH" == "x86_64-unknown-linux-musl" ]]; then
    FALLBACK_BINARY="vtcode-$RELEASE_TAG-x86_64-unknown-linux-gnu.tar.gz"
fi
echo "Your Platform: $DESC"
echo "Binary Name:   $BINARY_NAME"
if [[ -n "$FALLBACK_BINARY" ]]; then
    echo "Fallback:      $FALLBACK_BINARY"
fi
echo ""

# Fetch release info
printf '%b' "${BLUE}ℹ${NC} Checking GitHub API... "
RESPONSE=$(curl -fsSL "$GITHUB_API" 2>/dev/null || echo "")

if [[ -z "$RESPONSE" ]]; then
    printf '%b\n' "${RED}Failed to connect${NC}"
    echo ""
    echo "Cannot reach GitHub API or release $RELEASE_TAG not found."
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
if echo "$RESPONSE" | grep -q "\"name\": \"$BINARY_NAME\""; then
    HAS_YOUR_BINARY="yes"
elif [[ -n "$FALLBACK_BINARY" ]] && echo "$RESPONSE" | grep -q "\"name\": \"$FALLBACK_BINARY\""; then
    HAS_YOUR_BINARY="yes"
else
    HAS_YOUR_BINARY="no"
fi

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
printf '%b\n' "  Total Binaries:    $TOTAL_ASSETS/5"
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
    exit 0
else
    printf '%b\n' "${YELLOW}⏳ BUILDING${NC}"
    echo ""
    echo "Binaries are still being built. Check back in 5-10 minutes."
    echo ""
    echo "Monitor progress:"
    echo "  ./scripts/wait-for-release.sh -t $RELEASE_TAG"
    exit 1
fi
