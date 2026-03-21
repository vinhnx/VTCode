#!/bin/bash

# VTCODE - Debug Mode Launch Script
# This script provides fast development builds

set -eo pipefail

restore_terminal_state() {
  if [[ -t 1 ]]; then
    # Best-effort restore for raw/alternate-screen/mouse modes in case vtcode aborts.
    printf '\r\033[K\033[?1049l\033[?2004l\033[?1004l\033[?1006l\033[?1015l\033[?1003l\033[?1002l\033[?1000l\033[<1u\033[?25h' > /dev/tty 2>/dev/null || true
    stty sane < /dev/tty > /dev/tty 2>/dev/null || true
  fi
}

trap restore_terminal_state EXIT INT TERM

# Suppress macOS malloc warnings by REMOVING the env vars (not setting to 0)
# Setting to 0 triggers "can't turn off malloc stack logging" warnings
unset MallocStackLogging
unset MallocStackLoggingDirectory
unset MALLOCSTACKTOOLSDIR
unset MallocErrorAbort
unset MallocNanoZone

# Load .env for local development if present
if [[ -f ".env" ]]; then
  set -a
  # shellcheck disable=SC1091
  source ./.env
  set +a
fi

# Keep debug-run session artifacts in the legacy ~/.vtcode/sessions location
# so harness logs, session archives, and debug logs land in one place.
export VT_SESSION_DIR="${VT_SESSION_DIR:-$HOME/.vtcode/sessions}"

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
echo "Building VT Code in debug mode..."

# Try building with sccache first, fall back to regular cargo if sccache fails
# Show build progress with cargo's default terminal output
echo ""
if ! cargo build 2>&1 | tee /tmp/vtcode_build.log; then
    BUILD_ERROR=$(cat /tmp/vtcode_build.log)
    if [[ "$BUILD_ERROR" == *"sccache: error: Operation not permitted"* ]]; then
        echo "sccache permission error detected. Retrying without sccache..."
        export RUSTC_WRAPPER=""
        cargo build 2>&1 | tee /tmp/vtcode_build.log
        RESULT=$?
        if [ $RESULT -ne 0 ]; then
            echo "Build failed even without sccache. Check errors above."
            exit $RESULT
        fi
    else
        echo "Build failed with non-sccache error:"
        echo "$BUILD_ERROR"
        exit 1
    fi
else
    echo "Build completed successfully."
fi

echo ""
echo "Debug build complete!"
echo ""

HOST_TARGET="$(rustc -vV | sed -n 's/^host: //p')"
if [[ -n "$HOST_TARGET" ]]; then
    if [[ "${VTCODE_GHOSTTY_VT_AUTO_SETUP:-0}" == "1" && ! -f "dist/ghostty-vt/$HOST_TARGET/include/ghostty/vt.h" ]]; then
        echo "Bootstrapping Ghostty VT dev assets for $HOST_TARGET..."
        if ! bash scripts/setup-ghostty-vt-dev.sh "$HOST_TARGET"; then
            echo "Ghostty VT bootstrap failed. Falling back to legacy_vt100 snapshots."
        fi
    fi

    bash scripts/stage-ghostty-vt-assets.sh "$HOST_TARGET" "target/debug" >/dev/null 2>&1 || true
    if [[ -x "target/debug/ghostty-vt/ghostty_vt_host" || -x "target/debug/ghostty-vt/ghostty_vt_host.exe" ]]; then
        export VTCODE_GHOSTTY_VT_DIR="$(pwd)/target/debug/ghostty-vt"
        echo "Ghostty VT sidecar staged in target/debug/ghostty-vt"
    else
        unset VTCODE_GHOSTTY_VT_DIR
        echo "Ghostty VT sidecar unavailable. PTY snapshots will use legacy_vt100."
    fi
    echo ""
fi

echo "Starting vtcode chat with advanced features..."
echo "  - Async file operations enabled for better performance"
echo "  - Real-time file diffs enabled in chat"
echo "  - Type your coding questions and requests"
echo "  - Press Ctrl+C to exit"
echo "  - The agent has access to file operations and coding tools"
echo "  - Set VTCODE_GHOSTTY_VT_AUTO_SETUP=1 to download/build local Ghostty VT sidecar assets"
echo ""
echo "Tip: Use './scripts/rrf.sh' for fast optimized runs (release-fast profile)"
echo "      Or add 'alias rrf=\"$(pwd)/scripts/rrf.sh\"' to your shell config for convenience"
echo "      Or add '$(pwd)/bin' to your PATH and use 'rrf' from anywhere in the project"
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
# Increase stack floor for spawned threads in debug runs to reduce overflow risk
export RUST_MIN_STACK="${RUST_MIN_STACK:-16777216}"
if [[ "${RUSTC_WRAPPER:-}" == "" ]]; then
    # If RUSTC_WRAPPER was set to empty string, run with it still empty
    RUSTC_WRAPPER="" cargo run -- "${EXTRA_ARGS[@]}" --show-file-diffs --debug
    # Clean up the environment variable after running
    unset RUSTC_WRAPPER
else
    # Otherwise run normally
    cargo run -- "${EXTRA_ARGS[@]}" --show-file-diffs --debug
fi
