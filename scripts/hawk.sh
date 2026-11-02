#!/bin/bash
# Hawk — dead public code and visibility analyzer for Rust workspaces
# Usage: ./scripts/hawk.sh [--fix] [--deny] [--json] [--config <path>]
# Docs: https://github.com/astral-sh/hawk

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
HAWK_TOOLCHAIN="1.99.1"
# Hawk links rustc internals, so a build only runs against the exact compiler it
# shipped with. Keep this pinned to the release built for HAWK_TOOLCHAIN.
# 0.1.15's prebuilt archives track rustc 1.99.0, which the workspace MSRV floor
# (1.99.1) refuses, so run the from-source build below until upstream publishes
# a 1.99.1 archive.
HAWK_VERSION="${HAWK_VERSION:-0.1.15}"

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
	-D)
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

# Ensure the pinned toolchain and a hawk build for that exact compiler exist.
if ! rustup toolchain list 2>/dev/null | grep -q "$HAWK_TOOLCHAIN"; then
	echo "Installing Rust $HAWK_TOOLCHAIN (required by hawk)..."
	rustup toolchain install "$HAWK_TOOLCHAIN"
fi

installed_hawk="$(cargo hawk --version 2>/dev/null || true)"
if [[ "$installed_hawk" != *"$HAWK_VERSION"* ]]; then
	cat >&2 <<EOF
cargo-hawk $HAWK_VERSION is required (found: ${installed_hawk:-none}).

Hawk links rustc internals, so only the $HAWK_VERSION build runs against rustc
$HAWK_TOOLCHAIN. Install the prebuilt archive for this platform from

  https://github.com/astral-sh/hawk/releases/download/$HAWK_VERSION

and place cargo-hawk and cargo-hawk-driver together on PATH, or build from source:

  rustup toolchain install $HAWK_TOOLCHAIN --component rustc-dev
  RUSTC_BOOTSTRAP=1 cargo +$HAWK_TOOLCHAIN install --locked cargo-hawk --version $HAWK_VERSION

Set HAWK_VERSION to audit with a different release.
EOF
	exit 1
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
