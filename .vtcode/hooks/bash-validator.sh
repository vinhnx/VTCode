#!/bin/bash
# bash-validator.sh - Example hook script to validate bash commands
# This script receives JSON input via stdin with hook event data

# Read the JSON input
INPUT_JSON=$(cat)

# Extract the command being executed
COMMAND=$(echo "$INPUT_JSON" | jq -r '.tool_input.command // empty' 2>/dev/null)

# Define validation rules
INVALID_PATTERNS=(
  "rm -rf /"
  "rm -rf .*\\$HOME"
  "chmod.*777"
)

# Check for invalid patterns
for pattern in "${INVALID_PATTERNS[@]}"; do
  if [[ "$COMMAND" =~ $pattern ]]; then
    echo "Error: Command contains potentially dangerous pattern: $pattern" >&2
    exit 2  # Block the command
  fi
done

# Check for potentially unsafe commands
UNSAFE_COMMANDS=(
  "dd"
  "mkfs"
  "format"
)

for unsafe_cmd in "${UNSAFE_COMMANDS[@]}"; do
  if [[ "$COMMAND" =~ (^|\\s)$unsafe_cmd\\s ]]; then
    echo "Warning: Potentially unsafe command detected: $unsafe_cmd" >&2
    # For this example, we'll allow but log - in practice you might want to block
    echo "Command allowed but logged for review: $COMMAND"
  fi
done

# If we get here, the command is allowed
echo "Bash command validated successfully"
exit 0