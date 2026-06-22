# Audit: Raw println!/print! calls that could leak into the TUI

## Summary

Raw `println!`/`print!` calls bypass crossterm's terminal management, causing:
- Corrupted display (text appears over TUI rendering)
- Blocked event loop (when paired with `io::stdin().read_line()`)
- Screen flashing or black screens

## Current Status: All Known Issues Fixed

| Severity | Issue | Fix | Commit |
|----------|-------|-----|--------|
| CRITICAL | `terminal.clear()` in ForceRedraw handler blanked entire screen | Removed `terminal.clear()`, session dirty flag suffices | `704b65c7c` |
| HIGH | MCP elicitation used raw `print!`+`stdin.read_line()` in TUI | Auto-decline in TUI mode with tracing log | `61baed0db` |
| HIGH | Agent switch did trust check AFTER `select_from_specs`, causing switch-revert flicker | Trust check moved BEFORE switch; cycle functions skip untrusted auto agents | `704b65c7c` |
| MEDIUM | `ensure_full_auto_workspace_trust` status messages used raw `println!` | `tui_safe_println()` helper routes to tracing in TUI mode | `61baed0db` |
| MEDIUM | `prompt_capable()` didn't account for TUI mode | Added `is_tui_mode()` guard | `7582512e3` |

## Remaining Low-Risk Items

### Atomicity gap in agent switching

**File:** `src/agent/runloop/unified/turn/session/interaction_loop_runner/support.rs:939-998`

If `select_from_specs` succeeds but a subsequent `sync_primary_agent_hook_runtime` or `sync_primary_agent_mcp_runtime` call fails, the agent state is partially updated. This is a recoverability concern, not a crash risk. The system would continue operating with the new agent but without synchronized hooks/MCP.

**Severity:** Low  
**Impact:** Partial state on async failure (agent active but hooks/MCP not synced)  
**Mitigation:** The async operations are idempotent and can be retried on next interaction.

### force_redraw during config live-reload

**File:** `src/agent/runloop/unified/turn/session/interaction_loop_runner/support.rs:631`

`apply_live_theme_and_appearance` calls `force_redraw()` on workspace config file reload. The 200ms debounce at `interaction_loop_runner.rs:59` mitigates frequent triggers.

**Severity:** Low  
**Impact:** Potential unnecessary redraws if config files change frequently  
**Mitigation:** Debounce duration is configurable.

## Safe I/O Patterns (Verified)

| File | Lines | Why Safe |
|------|-------|----------|
| `src/agent/runloop/git.rs` | 327-328 | `is_tui_mode()` check skips interactive prompt |
| `src/agent/runloop/unified/postamble.rs` | 41-122 | Called after `restore_tui()` |
| `src/codex_app_server/runtime.rs` | 266, 439-492, 709-719, 930 | Own TUI (not crossterm-based) |
| `src/cli/` | various | CLI commands, not TUI context |
| `src/main.rs`, `src/main_helpers/` | various | Startup/shutdown, not TUI |
| `src/process_hardening.rs` | various | `eprintln!` for error messages |
| `src/startup/first_run_prompts/` | various | First-run setup, not TUI |
| `vtcode-ui/src/tui/core_tui/session/terminal_title.rs` | 261-266 | Writes to stdout (TUI uses stderr) |
| `vtcode-ui/src/tui/core_tui/panic_hook.rs` | 89-208 | `eprintln!` in panic hook by design |

## Recommendations

### 1. Add lint rule (PREVENTIVE)

Consider adding an `ast-grep` rule or CI check to flag `println!`/`print!` in `src/` outside of:
- `#[cfg(test)]` modules
- CLI command handlers (`src/cli/`)
- Files with explicit `is_tui_mode()` guards

### 2. Consider atomic agent switching (LOW priority)

Wrap `select_from_specs` + sync operations in a rollback-capable pattern:
```rust
// Pseudocode
let result = select_and_sync(specs, name).await;
if result.is_err() {
    select_from_specs(specs, previous_name)?;  // rollback
}
```

This is low priority because the current failure modes are non-critical.

## Verification

```bash
# Find all non-test println/print calls in src/
grep -rn "println!\|print!(" --include="*.rs" src/ | grep -v "#\[test\]" | grep -v "// "
```

Any remaining hits outside `src/cli/` should have an `is_tui_mode()` guard or be provably TUI-safe.
