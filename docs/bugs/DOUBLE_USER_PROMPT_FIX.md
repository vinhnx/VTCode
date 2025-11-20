# Fix: Duplicate User Message in Conversation History

## Problem
Agent was displaying behavior as if the user asked a command twice, even when asked only once.

### Example Output (Before Fix)
```
User: show diff
Thinking: User asked "show diff" twice. We already showed diff. Probably just respond with the diff?
[Shows diff output again]
```

## Root Cause
The user's message was being added to the conversation history **twice**:

1. **First addition** (session_loop.rs:884):
   ```rust
   let user_message = match refined_content { ... };
   conversation_history.push(user_message);  // <-- First push
   ```

2. **Second addition** (session_loop.rs:888 → turn_loop.rs:213):
   ```rust
   let working_history = conversation_history.clone();  // Clone includes user message
   // ...
   run_turn_loop(input, working_history.clone(), ...)
   
   // Inside run_turn_loop (BUGGY):
   working_history.push(uni::Message::user(input.to_string()));  // <-- DUPLICATE PUSH!
   ```

The LLM received the user message twice in the conversation history, leading it to believe the user had asked the same question twice.

## Solution
**File:** `src/agent/runloop/unified/turn/turn_loop.rs`  
**Lines:** 212-213

Removed the duplicate message push. The user message is already in `working_history` when passed from the caller:

```rust
// BEFORE
// Add the user input to the working history
working_history.push(uni::Message::user(input.to_string()));

// AFTER
// NOTE: The user input is already in working_history from the caller (session_loop or run_loop)
// Do NOT add it again here, as it will cause duplicate messages in the conversation
```

## Changes Made
- **File modified:** `src/agent/runloop/unified/turn/turn_loop.rs`
- **Lines removed:** 1 (the redundant push)
- **Lines added:** 2 (explanatory comment)
- **Net change:** +1 line, -1 line

## Testing
✓  **Code compilation:** `cargo check` - PASS  
✓  **Unit tests:** `cargo test --lib` - All 17 tests PASS  
✓  **Linting:** `cargo clippy` - No new warnings  
✓  **No breaking API changes**

## Verification Steps
To confirm the fix resolves the issue:

1. Start the agent
2. Send a query (e.g., "show diff")
3. Observe that:
   - The user message appears only once in the history
   - Agent doesn't mention being asked the same question multiple times
   - Response quality is normal without duplicate context

## Commit
```
commit 8f28c05d
fix: remove duplicate user message in turn loop

The user input was being added to working_history twice:
1. First in session_loop.rs (line 884)
2. Then in turn_loop.rs (line 213) 

This caused the LLM to receive duplicate user messages, leading the agent
to think the user asked the same question twice.
```

## Impact
- **Severity:** Medium - Affects conversation context quality
- **Scope:** Only affects session_loop path that calls `run_turn_loop()`
- **Users affected:** All users in interactive agent mode
- **Performance impact:** None (actually slight improvement - less data to process)
