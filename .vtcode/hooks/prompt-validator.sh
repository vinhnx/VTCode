#!/bin/bash
# prompt-validator.sh - Example hook to validate user prompts
# This script receives JSON input via stdin with hook event data

# Read the JSON input
INPUT_JSON=$(cat)

# Extract the prompt text
PROMPT=$(echo "$INPUT_JSON" | jq -r '.prompt // empty' 2>/dev/null)

# Define sensitive patterns to block
SENSITIVE_PATTERNS=(
  "password"
  "secret"
  "key"
  "token"
  "api_key"
  "credential"
)

# Check for sensitive information in the prompt
for pattern in "${SENSITIVE_PATTERNS[@]}"; do
  if [[ "$PROMPT" =~ [^a-zA-Z0-9]${pattern}[^a-zA-Z0-9] ]] || [[ "$PROMPT" =~ ^${pattern}[^a-zA-Z0-9] ]] || [[ "$PROMPT" =~ [^a-zA-Z0-9]${pattern}$ ]]; then
    echo "Security violation: Prompt contains sensitive information: $pattern" >&2
    exit 2  # Block the prompt
  fi
done

# Add current time context to the prompt processing
echo "Current time: $(date)"
echo "Prompt validated successfully"

exit 0