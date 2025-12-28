#!/bin/bash
# session-cleanup.sh - Example hook to cleanup after session
# This script receives JSON input via stdin with hook event data

# Read the JSON input
INPUT_JSON=$(cat)

# Extract session information
SESSION_ID=$(echo "$INPUT_JSON" | jq -r '.session_id // empty' 2>/dev/null)
REASON=$(echo "$INPUT_JSON" | jq -r '.reason // empty' 2>/dev/null)

echo "Ending session: $SESSION_ID (reason: $REASON)"

# Perform any cleanup tasks
# For example, clean up temporary files
find /tmp -name "vtcode-*-$SESSION_ID*" -delete 2>/dev/null

# Log session end
echo "$(date): Session $SESSION_ID ended with reason: $REASON" >> /tmp/vtcode-sessions.log

# Add any project-specific cleanup here
# For example, stop background services if needed

exit 0