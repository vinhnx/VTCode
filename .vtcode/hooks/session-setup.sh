#!/bin/bash
# session-setup.sh - Example hook to setup session context
# This script receives JSON input via stdin with hook event data

# Read the JSON input
INPUT_JSON=$(cat)

# Extract session information
SESSION_ID=$(echo "$INPUT_JSON" | jq -r '.session_id // empty' 2>/dev/null)
SOURCE=$(echo "$INPUT_JSON" | jq -r '.source // empty' 2>/dev/null)

echo "Starting session: $SESSION_ID (source: $SOURCE)"

# Load project-specific environment variables
if [[ -f ".env" ]]; then
  export $(grep -v '^#' .env | xargs)
fi

# Add any project-specific setup here
# For example, check if dependencies are installed
if [[ -f "Cargo.toml" ]]; then
  echo "Rust project detected, checking dependencies..."
  cargo check --quiet
elif [[ -f "package.json" ]]; then
  echo "Node.js project detected, checking dependencies..."
  if [[ ! -d "node_modules" ]]; then
    echo "Installing dependencies..."
    npm install
  fi
elif [[ -f "requirements.txt" ]]; then
  echo "Python project detected, checking dependencies..."
  pip check
fi

# Log session start
echo "$(date): Session $SESSION_ID started from $SOURCE" >> /tmp/vtcode-sessions.log

exit 0