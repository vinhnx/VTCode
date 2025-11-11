#!/bin/bash

# VTCODE - Simple Launch Script
# This script provides the easiest way to run vtcode

set -e

echo "VTCODE - Research-preview Rust Coding Agent"
echo "=================================================="

# Check if API key is set
if [[ -z "$GEMINI_API_KEY" && -z "$GOOGLE_API_KEY" ]]; then
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

# Check if user wants debug build
if [[ "$1" == "debug" ]]; then
    echo "Building vtcode in debug mode for faster compilation..."
    echo "Tip: Use './run.sh' for release builds in production"
    echo ""
    echo "Using debug build for faster compilation..."
    cargo build
else
    echo "Building vtcode in release mode (this may take a few minutes)..."
    echo "Tip: Use './run.sh debug' for faster builds during development"
    echo ""
fi
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
    cargo run --release --  --show-file-diffs chat
fi
