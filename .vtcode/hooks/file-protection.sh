#!/bin/bash
# file-protection.sh - Example hook script to protect sensitive files
# This script receives JSON input via stdin with hook event data

# Read the JSON input
INPUT_JSON=$(cat)

# Extract the file path being written to
FILE_PATH=$(echo "$INPUT_JSON" | jq -r '.tool_input.file_path // empty' 2>/dev/null)

# Define protected files that should not be modified
PROTECTED_FILES=(
  ".env"
  ".git/config"
  "/etc/passwd"
  "/etc/hosts"
)

# Check if the file path matches any protected files
for protected in "${PROTECTED_FILES[@]}"; do
  if [[ "$FILE_PATH" == *"$protected" ]]; then
    echo "Error: Attempt to modify protected file: $protected" >&2
    exit 2  # Block the operation
  fi
done

# Check for sensitive file patterns in the project directory
if [[ "$FILE_PATH" =~ \.(pem|key|secret|password)$ ]]; then
  echo "Warning: Attempt to modify sensitive file: $FILE_PATH" >&2
  # For this example, we'll allow but log
  echo "Sensitive file operation logged for review: $FILE_PATH"
fi

# If we get here, the operation is allowed
echo "File operation validated successfully"
exit 0