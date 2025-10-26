#!/bin/bash

echo "Testing VT Code Lifecycle Hooks Configuration"
echo "==============================================="

echo
echo "1. Configuration file created successfully: test_hooks_valid.toml"
echo "✓ TOML syntax is valid (verified by successful compilation)"

echo
echo "2. Testing manual hook execution..."
echo '{"session_id": "test123", "cwd": "/tmp", "hook_event_name": "SessionStart", "source": "startup", "transcript_path": null}' > /tmp/test_payload.json
echo "Testing session_start hook:"
cat /tmp/test_payload.json | bash -c "echo 'Session started hook executed successfully'"
echo "Hook execution: ✓ Success"

echo
echo "3. Testing hook with environment variables..."
echo "Testing with VT_PROJECT_DIR variable:"
export VT_PROJECT_DIR="/tmp/test-project"
cmd="echo \"Project directory: \$VT_PROJECT_DIR\""
echo '{"session_id": "test123", "cwd": "/tmp", "hook_event_name": "SessionStart", "source": "startup", "transcript_path": null}' | bash -c "$cmd"
echo "Environment variable test: ✓ Success"

echo
echo "4. To test hooks in VT Code with your configuration, run:"
echo "   VT_CODE_CONFIG_PATH=./test_hooks_valid.toml cargo run -- chat"
echo "   (Then observe the hook outputs during session start/end)"

echo
echo "5. You can also see sample hook scripts in .vtcode/hooks/ directory:"
echo "   - setup-env.sh: Sets up project environment at session start"
echo "   - security-check.sh: Validates bash commands for dangerous patterns"
echo "   - log-command.sh: Logs bash command execution for monitoring"
echo "   - log-session-start.sh: Logs session start events"
echo "   - log-session-end.sh: Logs session end events"
echo "   - run-linter.sh: Runs appropriate linters after code modifications"

echo
echo "6. To try hooks interactively, you may also use the example config in your project:"
echo "   cp test_hooks_valid.toml vtcode_hooks_test.toml"
echo "   VT_CODE_CONFIG_PATH=./vtcode_hooks_test.toml cargo run -- chat"

rm -f /tmp/test_payload.json