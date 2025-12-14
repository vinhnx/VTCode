# Bug Report: Agent Shows User Asking Command Twice

## Issue Description
The VTCode agent displays the user's command/query twice in the output, even though it was only asked once.

Example output shows:
```
User: "show diff"
[Agent processes and shows diff]
Thinking: User asked "show diff" twice. We already showed diff...
[Agent repeats diff output again]
```

## Root Cause Analysis - CORRECTED

### THE ACTUAL BUG: Duplicate User Message in Conversation History
**File:** `src/agent/runloop/unified/turn/turn_loop.rs`
**Function:** `run_turn_loop()`  
**Lines:** 207-213

**Problem:** The user input is being added to the conversation history TWICE:

#### Call Chain:
1. **session_loop.rs (line 884)**: User message is pushed to `conversation_history`
2. **session_loop.rs (line 888)**: `working_history = conversation_history.clone();` - includes the user message
3. **session_loop.rs (line 943-954)**: `run_turn_loop(input, working_history.clone(), ...)` is called
4. **turn_loop.rs (line 213)**: BUG - the input is pushed to working_history AGAIN!

```rust
// In turn_loop.rs, line 213
working_history.push(uni::Message::user(input.to_string()));  // <-- DUPLICATE!
```

This causes the conversation history to contain the user message twice, which makes the LLM think the user asked the same question twice. That's why the agent responds with reasoning about being asked twice!

## The Fix

**Remove line 213 from turn_loop.rs** that adds the user input again:

```rust
// BEFORE (BUGGY):
working_history.push(uni::Message::user(input.to_string()));

// AFTER (FIXED):
// NOTE: The user input is already in working_history from the caller (session_loop or run_loop)
// Do NOT add it again here, as it will cause duplicate messages in the conversation
```

The user message is ALREADY included in `working_history` when passed to `run_turn_loop()` from `session_loop.rs` line 945. Adding it again creates a duplicate.

## Impact of Bug
- LLM receives the user's message twice in the conversation history
- Agent thinks the user asked the same question twice
- Agent's reasoning mentions "user asked... twice" even though user only asked once
- Causes confusion and poor response quality

## Files Changed
- `src/agent/runloop/unified/turn/turn_loop.rs` - Removed duplicate user message push

## Verification
-   Code compiles without errors (`cargo check`)
-   All tests pass (`cargo test --lib`)
-   No breaking changes to public API
-   Only affects session_loop path that calls `run_turn_loop()`

## Testing Strategy
To verify the fix works:
1. Run a query in the agent
2. Check that the user message appears only once in the conversation history
3. Verify agent doesn't mention being asked the same question multiple times
4. Check that conversation flows normally without duplicate messages
