#!/bin/bash

# VTCODE - Debug Mode Launch Script
# This script provides fast development builds

set -e

# Suppress macOS malloc warnings by REMOVING the env vars (not setting to 0)
# Setting to 0 triggers "can't turn off malloc stack logging" warnings
unset MallocStackLogging
unset MallocStackLoggingDirectory
unset MALLOCSTACKTOOLSDIR
unset MallocErrorAbort
unset MallocNanoZone

echo "VTCODE - Debug Mode (Fast Build)"
echo "=================================="

# Load .env for local development if present
if [[ -f ".env" ]]; then
  set -a
  # shellcheck disable=SC1091
  source ./.env
  set +a
fi

# Check if API key is set for any supported provider
if [[ -z "$GEMINI_API_KEY" && -z "$GOOGLE_API_KEY" && -z "$OPENAI_API_KEY" && -z "$ANTHROPIC_API_KEY" ]]; then
    echo "Error: API key not found!"
    echo ""
    echo "Set one of these environment variables:"
    echo "  export GEMINI_API_KEY='your_gemini_api_key_here'     # Google Gemini"
    echo "  export GOOGLE_API_KEY='your_google_api_key_here'     # Google Gemini (alias)"
    echo "  export OPENAI_API_KEY='your_openai_api_key_here'     # OpenAI GPT"
    echo "  export ANTHROPIC_API_KEY='your_anthropic_api_key'    # Anthropic Claude"
    echo ""
    echo "Docs:"
    echo "  Gemini:   https://aistudio.google.com/app/apikey"
    echo "  OpenAI:   https://platform.openai.com/api-keys"
    echo "  Anthropic:https://console.anthropic.com/"
    exit 1
fi

# Check if we're in the right directory
if [[ ! -f "Cargo.toml" ]]; then
    echo "Error: Please run this script from the vtcode project root directory"
    exit 1
fi

# Build and run in debug mode (much faster)
echo "Building vtcode in debug mode..."
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

# Build optional args from environment
EXTRA_ARGS=()
if [[ -n "$MODEL" ]]; then
  EXTRA_ARGS+=(--model "$MODEL")
fi
if [[ -n "$PROVIDER" ]]; then
  EXTRA_ARGS+=(--provider "$PROVIDER")
fi
if [[ -n "$WORKSPACE" ]]; then
  EXTRA_ARGS+=(--workspace "$WORKSPACE")
fi

# Run with advanced features enabled by default
# Note: Interactive chat is launched via the TUI without a subcommand
cargo run -- "${EXTRA_ARGS[@]}" --show-file-diffs --debug
