#!/bin/bash
# run-tests.sh - Example hook to run tests after file modifications
# This script receives JSON input via stdin with hook event data

# Read the JSON input
INPUT_JSON=$(cat)

# Extract the file path that was written
FILE_PATH=$(echo "$INPUT_JSON" | jq -r '.tool_input.file_path // empty' 2>/dev/null)

# Check if the modified file is in a location that should trigger tests
if [[ "$FILE_PATH" =~ src/.*\.(rs|js|ts|py|go)$ ]]; then
  echo "File change detected in source directory: $FILE_PATH"

  # Check if there's a test script in the project
  if [[ -f "test.sh" ]]; then
    echo "Running project tests..."
    ./test.sh
  elif [[ -f "Cargo.toml" ]] && [[ "$FILE_PATH" =~ \.rs$ ]]; then
    # Rust project - prefer nextest for faster execution
    echo "Running Rust tests..."
    if command -v cargo-nextest &> /dev/null; then
      cargo nextest run --profile quick
    else
      cargo test --quiet
    fi
  elif [[ -f "package.json" ]] && [[ "$FILE_PATH" =~ \.(js|ts)$ ]]; then
    # Node.js project
    echo "Running Node.js tests..."
    npm test
  elif [[ -f "pytest.ini" ]] || [[ -f "setup.py" ]]; then
    # Python project
    echo "Running Python tests..."
    python -m pytest
  fi
fi

exit 0