#!/bin/bash
# VTCode notification hook
# Sends desktop notifications when VTCode needs input

# Read the JSON payload from stdin
payload=$(cat)

# Extract the notification details
event_type=$(echo "$payload" | jq -r '.hook_event_name // "unknown"' 2>/dev/null)
session_id=$(echo "$payload" | jq -r '.session_id // "unknown"' 2>/dev/null)

if [ "$event_type" = "unknown" ]; then
    event_type=$(echo "$payload" | python3 -c "import sys, json; data=json.load(sys.stdin); print(data.get('hook_event_name', 'unknown'))" 2>/dev/null)
    session_id=$(echo "$payload" | python3 -c "import sys, json; data=json.load(sys.stdin); print(data.get('session_id', 'unknown'))" 2>/dev/null)
fi

# Create notification based on the event type
case "$event_type" in
    "UserPromptSubmit")
        title="VTCode Awaiting Input"
        message="VTCode is processing your prompt..."
        ;;
    "PreToolUse")
        title="VTCode Tool Execution"
        message="VTCode is about to execute a tool..."
        ;;
    "SessionStart")
        title="VTCode Session Started"
        message="New VTCode session started (ID: $session_id)"
        ;;
    "SessionEnd")
        title="VTCode Session Ended"
        message="VTCode session completed (ID: $session_id)"
        ;;
    *)
        title="VTCode Notification"
        message="VTCode event: $event_type (Session: $session_id)"
        ;;
esac

# Try to send notification using different methods depending on the OS
if command -v osascript >/dev/null 2>&1; then
    # macOS
    osascript -e "display notification \"$message\" with title \"$title\""
elif command -v notify-send >/dev/null 2>&1; then
    # Linux with libnotify
    notify-send "$title" "$message"
elif command -v terminal-notifier >/dev/null 2>&1; then
    # macOS with terminal-notifier
    terminal-notifier -title "$title" -message "$message"
else
    # Fallback: log to file
    echo "$(date): $title - $message" >> ~/.vtcode/notifications.log
fi

echo "Notification sent: $title - $message"