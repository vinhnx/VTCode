# Doctor Command: Failure Suggestions & Next Actions

## Overview

The `/doctor` command now provides contextual suggestions when diagnostic checks fail, helping users quickly resolve issues and know what to do next.

## Features

### 1. Inline Failure Suggestions

When a check fails, a helpful suggestion appears immediately below:

```
✗ API Key: Missing API key for 'openai': OPENAI_API_KEY not found
  → Set API key: export OPENAI_API_KEY=sk-... or similar for your provider.
```

The suggestion (→) provides:

-   What went wrong
-   How to fix it
-   Relevant commands or documentation

### 2. Recommended Next Actions Section

A new `[Recommended Next Actions]` section appears at the end:

```
[Recommended Next Actions]
✓ All checks passed? You're ready to go. Try `/status` for session details.
✗ Failures detected? Follow the suggestions above (→) to resolve issues.
💡 For more details: `/skills list` (available skills), `/status` (session), `/context` (memory)
📖 See docs/development/DOCTOR_REFERENCE.md for comprehensive troubleshooting.
```

This provides:

-   Quick status: All pass or some failures?
-   Action items based on results
-   Cross-references to related commands
-   Links to comprehensive docs

## Supported Suggestions

### Workspace

```
✗ Workspace: Workspace directory is missing or inaccessible
  → Ensure workspace directory is accessible and not deleted.
```

### API Key

```
✗ API Key: Missing API key for 'openai': OPENAI_API_KEY not found
  → Set API key: export OPENAI_API_KEY=sk-... or similar for your provider.
```

### Configuration

```
✗ Config File: Failed to load configuration from vtcode.toml
  → Copy vtcode.toml.example to vtcode.toml to customize settings.
```

### Dependencies

```
✗ Node.js: Node.js not found in PATH
  → Install Node.js: brew install node (macOS) or see nodejs.org

✗ npm: npm not found in PATH
  → Install npm with Node.js or update: npm install -g npm@latest

✗ Ripgrep: Ripgrep not installed (searches will fall back to built-in grep)
  → Install Ripgrep: brew install ripgrep (macOS), apt install ripgrep (Linux), or cargo install ripgrep
```

### External Services (MCP)

```
✗ MCP: Init error: Failed to start MCP server 'time'
  → Check MCP configuration in vtcode.toml: ensure servers are running and timeouts are reasonable.

✗ MCP: Disabled in configuration
  → Enable MCP in vtcode.toml: set [mcp] enabled = true
```

## Example Output

### All Checks Pass

```
═══════════════════════════════════════════════════════════════
VT Code Doctor v0.52.10
═══════════════════════════════════════════════════════════════

[Core Environment]
  ✓ Workspace: /path/to/workspace
  ✓ CLI Version: VT Code 0.52.10

[Configuration]
  ✓ Config File: Loaded from /path/to/vtcode.toml
  ✓ Theme: ciapre-dark
  ✓ Model: gpt-4 (+ small model: gpt-4-mini)
  ✓ Max Turns: 50
  ...

[Recommended Next Actions]
✓ All checks passed? You're ready to go. Try `/status` for session details.
✗ Failures detected? Follow the suggestions above (→) to resolve issues.
💡 For more details: `/skills list` (available skills), `/status` (session), `/context` (memory)
📖 See docs/development/DOCTOR_REFERENCE.md for comprehensive troubleshooting.

═══════════════════════════════════════════════════════════════
```

### With Failures

```
[API & Providers]
  ✗ API Key: Missing API key for 'openai': OPENAI_API_KEY not found
  → Set API key: export OPENAI_API_KEY=sk-... or similar for your provider.

[Dependencies]
  ✗ Node.js: Node.js not found in PATH
  → Install Node.js: brew install node (macOS) or see nodejs.org
  ✓ npm: npm 9.6.4
  ✗ Ripgrep: Ripgrep not installed (searches will fall back to built-in grep)
  → Install Ripgrep: brew install ripgrep (macOS), apt install ripgrep (Linux), or cargo install ripgrep

...

[Recommended Next Actions]
✓ All checks passed? You're ready to go. Try `/status` for session details.
✗ Failures detected? Follow the suggestions above (→) to resolve issues.
💡 For more details: `/skills list` (available skills), `/status` (session), `/context` (memory)
📖 See docs/development/DOCTOR_REFERENCE.md for comprehensive troubleshooting.

═══════════════════════════════════════════════════════════════
```

## Implementation Details

### Code Location

-   File: `src/agent/runloop/unified/diagnostics.rs`
-   Functions:
    -   `render_doctor_check()` - Shows check status and triggers suggestion
    -   `get_suggestion_for_failure()` - Generates context-aware suggestions

### How It Works

1. **Check Execution**: Each diagnostic check produces a `Result<String, String>`
2. **Render Success**: Pass results show `✓ label: detail`
3. **Render Failure**: Fail results show `✗ label: detail`
4. **Suggestion Query**: `get_suggestion_for_failure()` is called with the label and error message
5. **Smart Matching**: Suggestions are matched based on keywords in the check name
6. **Display**: Suggestion appears as `→ helpful text` indented below the failure

### Smart Suggestion Matching

Suggestions are matched using:

-   Case-insensitive label matching
-   Error message content checking
-   Context-aware recommendations

Example:

```rust
if label_lower.contains("api key") {
    Some("Set API key: export OPENAI_API_KEY=sk-... or similar for your provider.".to_string())
} else if label_lower.contains("ripgrep") {
    Some("Install Ripgrep: brew install ripgrep (macOS), apt install ripgrep (Linux), or cargo install ripgrep".to_string())
}
```

## User Workflow

1. **Run Diagnosis**: `/doctor`
2. **Review Results**:
    - Green checks (✓) = OK
    - Red failures (✗) = Need attention
3. **Follow Suggestions**: Read the arrow (→) text below each failure
4. **Fix Issues**: Execute suggested commands
5. **Verify**: Re-run `/doctor` to confirm fixes
6. **Explore More**: Use recommended commands for additional details

## Benefits

### For Users

-   **Faster Resolution**: Suggestions appear immediately without searching docs
-   **Clarity**: Clear explanation of what's wrong and how to fix it
-   **Guidance**: Next actions section guides them on what to do after diagnosis
-   **Learning**: Helps users understand system requirements

### For Developers

-   **Self-Healing**: Users can often resolve issues on their own
-   **Support Load**: Reduces support requests for common issues
-   **Discovery**: Helps users find related commands and features

## Future Enhancements

Possible improvements:

-   Interactive fix suggestions (execute directly)
-   Severity levels for failures (warning vs critical)
-   Custom suggestion providers via plugins
-   Suggestion feedback (thumbs up/down)
-   Analytics on most common failures

## Related Documentation

-   `DOCTOR_REFERENCE.md` - Quick reference guide
-   `DOCTOR_COMPLETE_CHANGELOG.md` - Technical implementation details
