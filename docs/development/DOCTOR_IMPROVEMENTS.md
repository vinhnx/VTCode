# Doctor Command Improvements

## Overview

The `/doctor` command has been significantly improved with better output formatting and comprehensive configuration diagnostics.

## What Changed

### Enhanced Output Format

The `/doctor` command now displays results in organized sections with clear visual hierarchy:

```
═══════════════════════════════════════════════════════════════
VTCode Doctor v0.52.10
═══════════════════════════════════════════════════════════════

[Core Environment]
  ✓ Workspace: /path/to/workspace
  ✓ CLI Version: VTCode 0.52.10

[Configuration]
  ✓ Config File: Loaded from /path/to/vtcode.toml
  ✓ Theme: ciapre-dark
  ✓ Model: gpt-4 (+ small model: gpt-4o-mini)
  ✓ Max Turns: 50
  ✓ Context Tokens: 90000
  ✓ Token Budget: Enabled (model: gpt-4)
  ✓ Decision Ledger: Enabled (max 12 entries)
  ✓ Max Tool Loops: 20
  ✓ HITL Enabled: Yes
  ✓ Tool Policy: Prompt on tool use
  ✓ PTY Enabled: Yes

[API & Providers]
  ✓ API Key: API key configured for 'openai'

[Dependencies]
  ✓ Node.js: Node.js v18.16.0
  ✓ npm: npm 9.6.4
  ✓ Ripgrep: Ripgrep 13.0.0

[External Services]
  ✓ MCP: 2 configured, 1 active connection(s)

[Workspace Links]
  [1] sandbox → /path/to/sandbox
  [2] tools → /path/to/tools

[Skills]
  3 loaded skill(s):
    [1] frontend-design (user)
    [2] code-reviewer (user)
    [3] doc-coauthoring (user)

═══════════════════════════════════════════════════════════════

[Recommended Next Actions]
✓ All checks passed? You're ready to go. Try `/status` for session details.
✗ Failures detected? Follow the suggestions above (→) to resolve issues.
💡 For more details: `/skills list` (available skills), `/status` (session), `/context` (memory)
📖 See docs/development/DOCTOR_REFERENCE.md for comprehensive troubleshooting.

═══════════════════════════════════════════════════════════════
```

### Example with Failures

The doctor also shows contextual suggestions when checks fail:

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
```

### New Configuration Diagnostics

The doctor now displays important configuration options:

- **Theme**: Current active theme (affects ANSI styling)
- **Model**: Primary model with small model info if enabled
- **Max Turns**: Maximum conversation turns before termination
- **Context Tokens**: Maximum tokens allowed in context
- **Token Budget**: Status and assigned model for token tracking
- **Decision Ledger**: Status and max entry limit
- **Max Tool Loops**: Maximum iterations allowed per turn
- **HITL Enabled**: Human-in-the-loop approval status
- **Tool Policy**: Default tool execution policy (allow/deny/prompt)
- **PTY Enabled**: Pseudo-terminal support status

### Better Section Organization

Output is grouped into logical sections:
- **Core Environment**: Workspace and CLI version
- **Configuration**: All vtcode.toml settings
- **API & Providers**: API key status
- **Dependencies**: System tools (Node, npm, Ripgrep)
- **External Services**: MCP providers and status
- **Workspace Links**: Linked directories with indices
- **Skills**: Currently loaded skills with scope indicators

### Improved Formatting

- **Separators**: Visual section dividers for clarity
- **Unicode Indicators**: `✓` for pass, `✗` for fail (easier to scan)
- **Details**: More informative success/error messages
- **Indentation**: Hierarchical layout for readability
- **Linked Directories**: Show both alias name and actual path

### Failure Suggestions & Next Actions

- **Contextual Help**: Each failure includes a helpful suggestion arrow (→)
- **Action Recommendations**: Suggested commands and solutions for common issues
- **Next Steps Section**: Clear guidance on what to do after diagnosis completes

## Technical Changes

### File Modified
- `src/agent/runloop/unified/diagnostics.rs`

### Key Updates

1. **Section Headers**: Added clear section markers with `[Core Environment]`, `[Configuration]`, etc.
2. **Configuration Inspection**: Now reads actual `vtcode.toml` values and displays:
   - Theme setting
   - Model selection (including small model tier info)
   - Context limits and token budgets
   - Feature flags (decision ledger, token budget)
   - Tool limits
3. **Enhanced Linked Directory Display**: Shows both display name and actual path with indices
4. **Improved Messages**: More concise and informative status messages
5. **Visual Hierarchy**: Added separator lines for better visual structure

## Benefits

- **Better Diagnostics**: Users can quickly see if configuration is loaded and what values are active
- **Easier Troubleshooting**: Clear section organization makes it easier to find issues
- **Configuration Awareness**: Doctor now shows active configuration, helping users understand what's running
- **Improved UX**: More professional output with better formatting
- **Workspace Context**: Shows linked directories for workspace navigation

## Usage

Run the doctor check with:
```bash
/doctor
```

The command provides a quick overview of your VTCode environment and configuration status.
