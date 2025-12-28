#!/bin/bash
# VT Code Advanced Status Line Script
# Receives JSON input from vtcode via stdin and outputs a formatted status line
# Supports ANSI color codes for enhanced visual display

# Read JSON input from stdin
input=$(cat)

# Extract values using jq
MODEL_DISPLAY=$(echo "$input" | jq -r '.model.display_name // .model.id // "unknown"')
CURRENT_DIR=$(echo "$input" | jq -r '.workspace.current_dir // ""')
REASONING=$(echo "$input" | jq -r '.runtime.reasoning_effort // ""')
VERSION=$(echo "$input" | jq -r '.version // "unknown"')

# Extract git information if available
GIT_BRANCH=$(echo "$input" | jq -r '.git.branch // ""')
GIT_DIRTY=$(echo "$input" | jq -r '.git.dirty // false')

# Show git branch if in a git repo
GIT_STATUS=""
if [ -n "$GIT_BRANCH" ] && [ "$GIT_BRANCH" != "null" ]; then
    if [ "$GIT_DIRTY" = "true" ]; then
        GIT_STATUS=" on $GIT_BRANCH *"
    else
        GIT_STATUS=" on $GIT_BRANCH"
    fi
fi

# Format reasoning effort if present with color coding
REASONING_DISPLAY=""
if [ -n "$REASONING" ] && [ "$REASONING" != "null" ]; then
    case "$REASONING" in
        "high")
            REASONING_DISPLAY=" | thinking: \033[38;5;208m$REASONING\033[0m"  # Orange
            ;;
        "medium")
            REASONING_DISPLAY=" | thinking: \033[33m$REASONING\033[0m"  # Yellow
            ;;
        "low")
            REASONING_DISPLAY=" | thinking: \033[32m$REASONING\033[0m"  # Green
            ;;
        *)
            REASONING_DISPLAY=" | thinking: $REASONING"
            ;;
    esac
fi

# Extract context information if available
CONTEXT_UTIL=$(echo "$input" | jq -r '.context.utilization_percent // 0')
CONTEXT_TOKENS=$(echo "$input" | jq -r '.context.total_tokens // 0')

# Format context information if available with color coding based on utilization
CONTEXT_DISPLAY=""
if [ "$CONTEXT_UTIL" != "0" ] && [ "$CONTEXT_UTIL" != "null" ]; then
    # Color code based on context utilization
    if (( $(echo "$CONTEXT_UTIL > 80" | bc -l) )); then
        CONTEXT_COLOR="\033[31m"  # Red for high utilization
    elif (( $(echo "$CONTEXT_UTIL > 50" | bc -l) )); then
        CONTEXT_COLOR="\033[33m"  # Yellow for medium utilization
    else
        CONTEXT_COLOR="\033[32m"  # Green for low utilization
    fi
    CONTEXT_DISPLAY=" | ctx: ${CONTEXT_COLOR}${CONTEXT_UTIL}%\033[0m (\033[36m${CONTEXT_TOKENS}\033[0m tokens)"
fi

# Build the status line with colors
DIR_NAME=$(basename "$CURRENT_DIR")
echo -e "\033[38;5;111m[$MODEL_DISPLAY]\033[0m in \033[38;5;153m$DIR_NAME\033[0m$GIT_STATUS$REASONING_DISPLAY$CONTEXT_DISPLAY | \033[38;5;245mv$VERSION\033[0m"