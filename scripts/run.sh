#!/bin/bash

# VTCODE - Simple Launch Script
# This script provides the easiest way to run vtcode

set -eo pipefail

restore_terminal_state() {
  if [[ -t 1 ]]; then
    # Best-effort restore for raw/alternate-screen/mouse modes in case vtcode aborts.
    printf '\r\033[K\033[?1049l\033[?2004l\033[?1004l\033[?1006l\033[?1015l\033[?1003l\033[?1002l\033[?1000l\033[<1u\033[?25h' > /dev/tty 2>/dev/null || true
    stty sane < /dev/tty > /dev/tty 2>/dev/null || true
  fi
}

trap restore_terminal_state EXIT INT TERM

echo "VTCODE - Research-preview Rust Coding Agent"
echo "=================================================="

# Check if API key is set
if [[ -z "${GEMINI_API_KEY:-}" && -z "${GOOGLE_API_KEY:-}" ]]; then
    echo "Error: API key not found!"
    echo ""
    echo "Please set one of these environment variables:"
    echo "  export GEMINI_API_KEY='your_gemini_api_key_here'"
    echo "  export GOOGLE_API_KEY='your_google_api_key_here'"
    echo ""
    echo "Get your API key from: https://aistudio.google.com/app/apikey"
    exit 1
fi

# Check if we're in the right directory
if [[ ! -f "Cargo.toml" ]]; then
    echo "Error: Please run this script from the vtcode project root directory"
    exit 1
fi

# Increase stack floor for spawned threads to reduce overflow risk.
export RUST_MIN_STACK="${RUST_MIN_STACK:-16777216}"

# Check if user wants debug build
if [[ "${1:-}" == "debug" ]]; then
    echo "Building vtcode in debug mode for faster compilation..."
    echo "Tip: Use './run.sh' for release builds in production"
    echo ""
    cargo build
    echo ""
    echo "Debug build complete!"
    echo ""
    echo "Starting vtcode chat with advanced features..."
    echo "  - Async file operations enabled for better performance"
    echo "  - Real-time file diffs enabled in chat"
    echo "  - Type your coding questions and requests"
    echo "  - Press Ctrl+C to exit"
    echo "  - The agent has access to file operations and coding tools"
    echo ""
    cargo run -- --show-file-diffs chat
else
    echo "Building vtcode in release mode (this may take a few minutes)..."
    echo "Tip: Use './run.sh debug' for faster builds during development"
    echo ""
    cargo build --release
    echo ""
    echo "Build complete!"
    echo ""
    echo "Starting vtcode chat with advanced features..."
    echo "  - Async file operations enabled for better performance"
    echo "  - Real-time file diffs enabled in chat"
    echo "  - Type your coding questions and requests"
    echo "  - Press Ctrl+C to exit"
    echo "  - The agent has access to file operations and coding tools"
    echo ""
    cargo run --release -- --show-file-diffs chat
fi
