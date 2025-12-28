#!/bin/bash
# general-validator.sh - Example general validation hook
# This script receives JSON input via stdin with hook event data

# Read the JSON input
INPUT_JSON=$(cat)

# Extract tool name and input
TOOL_NAME=$(echo "$INPUT_JSON" | jq -r '.tool_name // empty' 2>/dev/null)

# Log the tool usage for monitoring
echo "Tool usage: $TOOL_NAME at $(date)" >> /tmp/vtcode-tool-usage.log

# Add any general validation logic here
# For this example, we'll just allow everything
echo "General validation passed for tool: $TOOL_NAME"
exit 0