#!/bin/bash
# VTCode hook to automatically format code files after editing
# Receives JSON payload from VTCode containing tool information

# Read the JSON payload from stdin
payload=$(cat)

# Extract the file path from the payload
file_path=$(echo "$payload" | jq -r '.tool_input.file_path // ""' 2>/dev/null)

# If jq is not available, try a simpler approach
if [ -z "$file_path" ] || [ "$file_path" = "" ]; then
    file_path=$(echo "$payload" | python3 -c "import sys, json; data=json.load(sys.stdin); print(data.get('tool_input', {}).get('file_path', ''))" 2>/dev/null)
fi

# Check if the file exists and format based on its extension
if [ -n "$file_path" ] && [ -f "$file_path" ]; then
    case "$file_path" in
        *.ts|*.tsx|*.js|*.jsx|*.json)
            # Format with prettier if available
            if command -v npx >/dev/null 2>&1; then
                npx prettier --write "$file_path" 2>/dev/null
                echo "Formatted $file_path with prettier"
            elif command -v prettier >/dev/null 2>&1; then
                prettier --write "$file_path" 2>/dev/null
                echo "Formatted $file_path with prettier"
            else
                echo "Prettier not available, skipping formatting for $file_path"
            fi
            ;;
        *.rs)
            # Format with rustfmt if available
            if command -v rustfmt >/dev/null 2>&1; then
                rustfmt "$file_path" 2>/dev/null
                echo "Formatted $file_path with rustfmt"
            else
                echo "rustfmt not available, skipping formatting for $file_path"
            fi
            ;;
        *.go)
            # Format with gofmt if available
            if command -v gofmt >/dev/null 2>&1; then
                gofmt -w "$file_path" 2>/dev/null
                echo "Formatted $file_path with gofmt"
            else
                echo "gofmt not available, skipping formatting for $file_path"
            fi
            ;;
        *.py)
            # Format with black if available
            if command -v black >/dev/null 2>&1; then
                black "$file_path" 2>/dev/null
                echo "Formatted $file_path with black"
            else
                echo "black not available, skipping formatting for $file_path"
            fi
            ;;
        *)
            echo "No formatter configured for $file_path"
            ;;
    esac
else
    echo "File does not exist: $file_path"
fi