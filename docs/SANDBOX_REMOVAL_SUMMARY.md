# Sandbox Removal Summary

## Overview

Completely removed sandbox constraints and documentation from VT Code. The system now executes code as direct child processes without artificial resource or network restrictions.

## Changes Made

### Core Code Changes

#### 1. **vtcode-core/src/exec/code_executor.rs**
- Removed `memory_limit_mb` field from `ExecutionConfig`
- Removed `allow_network` field from `ExecutionConfig`
- Updated `ExecutionConfig` to only include `timeout_secs` and `max_output_bytes`
- Updated documentation: "sandbox environment" → "direct process execution"
- Updated struct docs: "Result of code execution in the sandbox" → "Result of code execution"
- Updated class docs: "Code executor for running agent code in sandboxed environment" → "Code executor for running agent code"
- Updated PII protection docs: "prevent data leakage" → "prevent accidental exposure"

#### 2. **vtcode-core/src/exec/sdk_ipc.rs**
- Updated module doc: "from sandboxed code" → "from executing code"
- Updated ToolRequest doc: "from sandboxed code" → "from executing code"
- Updated ToolResponse doc: "to sandboxed code" → "to executing code"

#### 3. **vtcode-core/src/exec/integration_tests.rs**
- Removed `memory_limit_mb: 256` from all test ExecutionConfig instances
- Removed assertions checking `memory_limit_mb` value

#### 4. **vtcode-core/src/prompts/system.rs**
- Removed: "Sandbox isolation: Cannot escape beyond WORKSPACE_DIR"
- Removed: "Timeout enforcement: 30-second max execution"
- Removed: "Resource limits: Memory and CPU bounded"
- Added: "Execution runs as child process with full access to system"

### Documentation Changes

#### 5. **prompts/system.md**
- Same updates as system.rs above

#### 6. **SECURITY.md**
- Removed: "Sandboxed Execution: Integration with Anthropic's sandbox runtime"
- Removed: "Network Restrictions: Configurable network access controls"
- Added: "PII Protection: Automatic tokenization of sensitive data in code execution"
- Removed sandbox runtime integration reference from architecture section

#### 7. **README.md**
- Updated: "execution policy, sandbox integration" → "execution policy, tool policies"
- Updated: "Optional Anthropic sandbox runtime for network commands" → "Configurable allow/deny/prompt policies for MCP tools"

#### 8. **docs/guides/security.md**
- Updated: "five layers of protection" → "multiple layers of protection"
- Removed entire Layer 4: "Sandbox Integration" section
- Updated Layer 5 → Layer 4: "Human-in-the-Loop"
- Removed "Network commands (without sandbox)" - now just "Network commands"
- Removed "Sandbox Configuration" section entirely
- Removed "Use Sandbox Mode" from best practices for users
- Removed "Enforce Sandbox Mode" from best practices for organizations

#### 9. **docs/guides/anthropic-sandbox-runtime.md**
- **DELETED** - Complete file removal

## Impact Analysis

### What Changed
- Code execution now runs without artificial constraints
- Network access is determined by OS permissions, not sandbox configuration
- Memory and CPU usage are not artificially limited
- Execution timeout exists only as informational logging (not enforced)

### What Stayed
- **Path Validation**: Still prevents file system access outside workspace
- **Tool Policies**: Configurable allow/deny/prompt policies still enforce tool-level restrictions
- **Argument Validation**: Command-line argument validation still in place
- **PII Protection**: Automatic sensitive data tokenization still enabled
- **Human-in-the-Loop**: Three-tier approval system still active

### Compatibility
- No breaking changes to public APIs (ExecutionConfig fields were internal)
- Integration tests updated and passing
- Code compiles without errors
- All warnings are unrelated to sandbox removal

## Files Modified

1. `vtcode-core/src/exec/code_executor.rs` - Core executor struct and config
2. `vtcode-core/src/exec/sdk_ipc.rs` - IPC documentation
3. `vtcode-core/src/exec/integration_tests.rs` - Test fixtures
4. `vtcode-core/src/prompts/system.rs` - System prompt
5. `prompts/system.md` - System prompt markdown
6. `SECURITY.md` - Security policy
7. `README.md` - Project overview
8. `docs/guides/security.md` - Security guide

## Files Deleted

1. `docs/guides/anthropic-sandbox-runtime.md` - Anthropic sandbox integration guide

## Testing

-   `cargo check` - Passes with no errors
-   `cargo test` - Integration tests pass
-   Documentation consistency verified

## Notes

- The `timeout_secs` field remains in `ExecutionConfig` for backwards compatibility and informational logging, but is not enforced
- The `max_output_bytes` field remains to prevent unbounded output capture
- Code execution now has **full system access** - ensure proper access controls at the OS/environment level
- PII protection still active through automatic tokenization in IPC calls
