#!/bin/bash
# VT Code Custom Status Line Script
# Receives JSON input from vtcode via stdin and outputs a formatted status line

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

# Format reasoning effort if present
REASONING_DISPLAY=""
if [ -n "$REASONING" ] && [ "$REASONING" != "null" ]; then
    REASONING_DISPLAY=" | thinking: $REASONING"
fi

# Extract context information if available
CONTEXT_UTIL=$(echo "$input" | jq -r '.context.utilization_percent // 0')
CONTEXT_TOKENS=$(echo "$input" | jq -r '.context.total_tokens // 0')

# Format context information if available
CONTEXT_DISPLAY=""
if [ "$CONTEXT_UTIL" != "0" ] && [ "$CONTEXT_UTIL" != "null" ]; then
    CONTEXT_DISPLAY=" | ctx: ${CONTEXT_UTIL}% (${CONTEXT_TOKENS} tokens)"
fi

# Build the status line
DIR_NAME=$(basename "$CURRENT_DIR")
echo -e "[$MODEL_DISPLAY] in $DIR_NAME$GIT_STATUS$REASONING_DISPLAY$CONTEXT_DISPLAY | v$VERSION"