# Allocation Optimization Plan

Eliminate redundant heap allocations in vtcode's hot paths: the agent turn loop,
tool execution pipeline, and conversation format conversion. No new dependencies
required — all fixes are architectural.

---

## Change 1: Eliminate Double Message Clone Per Turn

**Problem**

The agent turn loop clones the entire `Vec<Message>` up to three times per turn:

1. `execute.rs:660` — `runtime.state.messages.clone()` to create a snapshot before
   calling `prepare_responses_request_messages` (needed because the function borrows
   `&mut runtime.state`).
2. `execute.rs:706` — `request.messages.clone()` to preserve messages before `request`
   is moved into `run_turn_once()`.
3. `session/mod.rs:238` — `messages.to_vec()` inside `set_previous_response_chain`,
   cloning the already-owned `sent_messages` a third time.

For a session with 100 messages, each clone deep-copies every `String`, `Vec<ToolCall>`,
`Vec<ContentPart>`, and `Option<Vec<Value>>` field on every `Message`.

**Root Cause**

`prepare_responses_request_messages` takes `(&mut AgentSessionState, Vec<Message>)` but
only mutates `session_state.previous_response_chains`. The full `&mut` borrow on
`AgentSessionState` prevents passing `&runtime.state.messages` simultaneously, forcing
the caller to clone first.

**Fix**

- Split the borrow in `prepare_responses_request_messages` (execute.rs:103). Change
  signature to:

  ```rust
  fn prepare_responses_request_messages(
      previous_chains: &mut HashMap<(String, String), ResponsesContinuationState>,
      provider_name: &str,
      provider_supports_responses_compaction: bool,
      model: &str,
      messages: &[Message],
  ) -> (Cow<'_, [Message]>, Option<String>)
  ```

  The caller passes `&mut runtime.state.previous_response_chains` and
  `&runtime.state.messages` separately. No clone needed to create the snapshot.

- For the incremental-history path (OpenAI), `prepare_openai_responses_request` currently
  takes `Vec<Message>` and slices `messages[previous_len..].to_vec()`. Change to return
  `Cow::Owned` for the slice case, `Cow::Borrowed` for the pass-through case.

- Change `set_previous_response_chain` (session/mod.rs:219) to accept `Vec<Message>` by
  value instead of `&[Message]`. This eliminates the `.to_vec()` at line 238. The caller
  at execute.rs:730 passes `sent_messages` by move.

- For `sent_messages` at execute.rs:706: since `request` is moved into `run_turn_once`,
  extract `request.messages` before the move. One approach — destructure or add a method
  to take messages out of the request, then reassemble or pass messages separately to
  `set_previous_response_chain` after the turn completes.

**Files**

| File | Lines | Change |
|------|-------|--------|
| `vtcode-core/src/core/agent/runner/execute.rs` | 103-121, 660-668, 706, 726-731 | Split borrow, remove clones |
| `vtcode-core/src/core/agent/session/mod.rs` | 219-241 | Accept `Vec<Message>` by value |
| `vtcode-core/src/llm/provider/responses_continuation.rs` | 45-96 | Return `Cow<'_, [Message]>` |

**Savings**: Eliminates 1-2 deep copies of the full message history per turn. For a
50-message session with rich content, this is hundreds of KB per turn.

---

## Change 2: Pass `Value` Through Tool Result Pipeline

**Problem**

Tool results follow this allocation-heavy path:

```
Value (tool output)
  → serde_json::to_string → String          [tool_exec.rs:560/712]
  → push_tool_result(String)
    → if Gemini:
        serde_json::from_str → Value         [session/mod.rs:341]  ← redundant re-parse
        → FunctionResponse { response: Value }
    → Message::tool_response(call_id, String)
```

For Gemini providers, the `Value` is serialized to `String` then immediately re-parsed
back to `Value` — two wasted allocations per tool call.

**Fix**

- Change `push_tool_result` (session/mod.rs:333) signature to accept `Value`:

  ```rust
  pub fn push_tool_result(
      &mut self,
      call_id: String,
      tool_name: &str,
      result: serde_json::Value,  // was: String
      is_gemini: bool,
  )
  ```

  Internally:
  - Gemini path: use `result` directly in `FunctionResponse.response` (no re-parse).
  - OpenAI/Anthropic path: `serde_json::to_string(&result)` once for
    `Message::tool_response`.

- Change `push_tool_error` (session/mod.rs:362) similarly. Error payloads are small
  `serde_json::json!({...})` values — pass them as `Value`.

- Update callers in `tool_exec.rs`:
  - Line 560-566 (parallel): pass `optimized_result` directly instead of serializing.
  - Line 712-721 (sequential): same.

**Files**

| File | Lines | Change |
|------|-------|--------|
| `vtcode-core/src/core/agent/session/mod.rs` | 332-386 | Accept `Value`, serialize only for OpenAI path |
| `vtcode-core/src/core/agent/runner/tool_exec.rs` | 559-566, 711-721 | Remove `serde_json::to_string`, pass `Value` |

**Savings**: Eliminates 1 `serde_json::to_string` + 1 `serde_json::from_str` per tool
call for Gemini. No regression for other providers (serialization moves into
`push_tool_result`).

---

## Change 3: Remove Redundant Clones in Parallel Tool Batch

**Problem**

In `execute_prepared_parallel_tool_calls` (tool_exec.rs:485):

1. Lines 495-499: Every `PreparedRunnerToolCall` is cloned into a `HashMap` via
   `.iter().map(|c| (c.tool_call_id.clone(), c.clone())).collect()`.
2. Lines 516-521: Inside the loop, `canonical_name`, `effective_args`, and `prepared`
   are cloned again for each future.

The `prepared_map` may not even be needed — it appears unused after construction in the
visible code.

**Fix**

- Verify whether `prepared_map` is used after line 499. If unused, remove it entirely.
- If it is used, build it by consuming `prepared_calls` (not borrowing), then iterate
  the map's `into_values()` for the futures loop.
- Restructure the futures loop to avoid redundant clones:

  ```rust
  for call in prepared_calls {
      let PreparedRunnerToolCall { tool_call_id, prepared } = call;
      let name = prepared.canonical_name.clone();  // still needed for logging
      let args = prepared.effective_args.clone();   // still needed for post-exec
      let tool_call_item = resolve_tool_call_item(...);
      let runner = self;
      let circuit_before = snapshot_circuit_diagnostics(runner, &name);
      let sem = semaphore.clone();
      futures.push(async move {
          // ... execute with `prepared` moved in ...
          (name, tool_call_id, args, tool_call_item, result, circuit_before)
      });
  }
  ```

  This moves `prepared` into the future instead of cloning it.

**Files**

| File | Lines | Change |
|------|-------|--------|
| `vtcode-core/src/core/agent/runner/tool_exec.rs` | 495-547 | Remove `prepared_map`, restructure loop |

**Savings**: Eliminates 1 clone of every `PreparedToolCall` (including its
`effective_args: Value` and `canonical_name: String`) per parallel batch. For 5
concurrent tool calls, that's 5 `Value` clones saved.

---

## Change 4: Make `normalize_tool_args` Mutate In-Place

**Problem**

`normalize_tool_args` (tool_args.rs:50) clones the entire `args.as_object()` Map at
line 60, even though it only conditionally inserts 1-3 entries. The clone happens on
every tool call admission.

**Fix**

- Change `normalize_tool_args` to take `args: Value` by value (move) instead of
  `&Value`:

  ```rust
  pub(super) fn normalize_tool_args(
      &self,
      name: &str,
      args: Value,                    // was: &Value
      session_state: &mut AgentSessionState,
  ) -> Value
  ```

- Inside, convert the input directly into a mutable Map:

  ```rust
  let mut normalized = match args {
      Value::Object(map) => map,      // zero-cost, no clone
      other => return other,           // non-object passthrough
  };
  // ... insert missing defaults into `normalized` ...
  Value::Object(normalized)
  ```

- Update callers:
  - `admit_tool_call` (tool_access.rs:74): change `args: &Value` to `args: Value`.
  - `admit_runner_tool_call` (tool_exec.rs:437): pass `args` by move instead of
    `&args`. The `args` value is created by `execution_arguments()` and not used
    after admission.

**Files**

| File | Lines | Change |
|------|-------|--------|
| `vtcode-core/src/core/agent/runner/tool_args.rs` | 50-100 | Take `Value` by move, avoid `Map` clone |
| `vtcode-core/src/core/agent/runner/tool_access.rs` | 74-85 | Change `args` param to `Value` |
| `vtcode-core/src/core/agent/runner/tool_exec.rs` | 403-437 | Pass `args` by move |

**Savings**: Eliminates 1 `Map` clone (heap allocation of all key-value pairs) per tool
call admission. For tool args with nested objects (e.g., file diffs, search filters),
this can be significant.

---

## Implementation Order

| Step | Change | Risk | Effort |
|------|--------|------|--------|
| 1 | Change 4: normalize_tool_args in-place | Low | Small — self-contained, no API ripple |
| 2 | Change 3: parallel batch clone removal | Low | Small — localized to tool_exec.rs |
| 3 | Change 2: push_tool_result takes Value | Medium | Medium — touches session/mod.rs + tool_exec.rs |
| 4 | Change 1: message clone elimination | High | Large — restructures borrow pattern in agent loop |

Changes 1-3 are independent and can be done in any order. Change 4 is the most
impactful but also the riskiest due to borrow restructuring.

---

## Verification

After each change:

```bash
cargo check --locked -p vtcode-core
cargo nextest run -p vtcode-core
cargo clippy -p vtcode-core
```

After all changes:

```bash
./scripts/check-dev.sh
```

---

## Fallback for Change 4 (if borrow split is too complex)

If splitting the `&mut AgentSessionState` borrow proves too invasive, a simpler
fallback:

- Keep `prepare_responses_request_messages` signature as-is.
- Only eliminate the `.to_vec()` inside `set_previous_response_chain` by changing it to
  accept `Vec<Message>` by value.
- This still saves one full deep-copy per turn (from three clones down to two).

---

## Summary

| Change | Allocation Eliminated | Per-Call Savings |
|--------|----------------------|------------------|
| 1. Message clone | `Vec<Message>` deep copy | Hundreds of KB per turn |
| 2. Value passthrough | serialize + re-parse cycle | ~2 allocs per tool call (Gemini) |
| 3. Batch clone | `PreparedToolCall` clone | 1 `Value` clone per parallel tool |
| 4. In-place normalize | `Map` clone | 1 `Map` clone per tool call |
