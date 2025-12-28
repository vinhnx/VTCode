#!/bin/bash
# code-formatter.sh - Example code formatting hook
# This script receives JSON input via stdin with hook event data

# Read the JSON input
INPUT_JSON=$(cat)

# Extract the file path that was written
FILE_PATH=$(echo "$INPUT_JSON" | jq -r '.tool_input.file_path // empty' 2>/dev/null)

# Determine the file type and format accordingly
if [[ "$FILE_PATH" =~ \.rs$ ]]; then
  # Rust file - use rustfmt if available
  if command -v rustfmt &> /dev/null; then
    rustfmt "$FILE_PATH"
    echo "Formatted Rust file: $FILE_PATH"
  fi
elif [[ "$FILE_PATH" =~ \.js$|\.ts$|\.jsx$|\.tsx$ ]]; then
  # JavaScript/TypeScript file - use prettier if available
  if command -v prettier &> /dev/null; then
    prettier --write "$FILE_PATH"
    echo "Formatted JS/TS file: $FILE_PATH"
  fi
elif [[ "$FILE_PATH" =~ \.py$ ]]; then
  # Python file - use black if available
  if command -v black &> /dev/null; then
    black "$FILE_PATH"
    echo "Formatted Python file: $FILE_PATH"
  fi
elif [[ "$FILE_PATH" =~ \.json$ ]]; then
  # JSON file - use jq to format
  if command -v jq &> /dev/null; then
    cp "$FILE_PATH" "$FILE_PATH.tmp" && jq '.' "$FILE_PATH.tmp" > "$FILE_PATH" && rm "$FILE_PATH.tmp"
    echo "Formatted JSON file: $FILE_PATH"
  fi
fi

exit 0