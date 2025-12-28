#!/bin/bash
# VTCode hook to log bash commands that are executed
# Receives JSON payload from VTCode containing tool information

# Read the JSON payload from stdin
payload=$(cat)

# Extract the command and description from the payload
command=$(echo "$payload" | jq -r '.tool_input.command // "unknown command"' 2>/dev/null)
description=$(echo "$payload" | jq -r '.tool_input.description // "No description"' 2>/dev/null)

# If jq is not available, try a simpler approach
if [ "$command" = "unknown command" ] || [ "$command" = "" ]; then
    command=$(echo "$payload" | python3 -c "import sys, json; data=json.load(sys.stdin); print(data.get('tool_input', {}).get('command', 'unknown command'))" 2>/dev/null)
    description=$(echo "$payload" | python3 -c "import sys, json; data=json.load(sys.stdin); print(data.get('tool_input', {}).get('description', 'No description'))" 2>/dev/null)
fi

# Log the command to a file
echo "$(date): $command - $description" >> ~/.vtcode/bash-command-log.txt

# Also output to stdout for VTCode to potentially capture
echo "Logged command: $command"