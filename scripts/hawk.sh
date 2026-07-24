#!/bin/bash
# Hawk — dead public code and visibility analyzer for Rust workspaces
# Usage: ./scripts/hawk.sh [--fix] [--deny] [--json] [--config <path>]
# Docs: https://github.com/astral-sh/hawk

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
HAWK_TOOLCHAIN="1.97.1"

# Default arguments
CONFIG="${HAWK_CONFIG:-$PROJECT_ROOT/hawk.toml}"
FIX=false
DENY=false
JSON=false
EXTRA_ARGS=()

# Parse arguments
while [[ $# -gt 0 ]]; do
	case "$1" in
	--fix)
		FIX=true
		shift
		;;
	--deny)
		DENY=true
		shift
		;;
	--json | --output-format=json)
		JSON=true
		shift
		;;
	--config)
		CONFIG="$2"
		shift 2
		;;
	-D | --deny)
		EXTRA_ARGS+=("$1" "$2")
		shift 2
		;;
	-W | --warn)
		EXTRA_ARGS+=("$1" "$2")
		shift 2
		;;
	-A | --allow)
		EXTRA_ARGS+=("$1" "$2")
		shift 2
		;;
	--help | -h)
		echo "Hawk — dead public code and visibility analyzer"
		echo ""
		echo "Usage: $0 [OPTIONS]"
		echo ""
		echo "Options:"
		echo "  --fix          Apply automatic visibility fixes"
		echo "  --deny         Deny all warnings (exit non-zero on any finding)"
		echo "  --json         Output JSON report to stdout"
		echo "  --config PATH  Path to hawk.toml (default: hawk.toml)"
		echo "  -D LINT        Deny a specific lint"
		echo "  -W LINT        Warn on a specific lint"
		echo "  -A LINT        Allow a specific lint"
		echo "  --help, -h     Show this help"
		echo ""
		echo "Lints: warnings, hawk::dead_public, hawk::unnecessary_public,"
		echo "       hawk::unnecessary_restricted_visibility, hawk::unnecessary_crate_visibility"
		exit 0
		;;
	*)
		EXTRA_ARGS+=("$1")
		shift
		;;
	esac
done

# Ensure toolchain is installed
if ! rustup toolchain list 2>/dev/null | grep -q "$HAWK_TOOLCHAIN"; then
	echo "Installing Rust $HAWK_TOOLCHAIN (required by hawk)..."
	rustup toolchain install "$HAWK_TOOLCHAIN"
fi

# Build arguments
ARGS=("--manifest-path" "$PROJECT_ROOT/Cargo.toml")
ARGS+=("--config" "$CONFIG")

if [ "$FIX" = true ]; then
	ARGS+=("--fix")
fi

if [ "$DENY" = true ]; then
	ARGS+=("-D" "warnings")
fi

if [ "$JSON" = true ]; then
	ARGS+=("--output-format" "json")
fi

ARGS+=("${EXTRA_ARGS[@]}")

echo "==> cargo +$HAWK_TOOLCHAIN hawk check ${ARGS[*]}"
exec cargo "+$HAWK_TOOLCHAIN" hawk check "${ARGS[@]}"
