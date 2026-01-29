extract vtcode-\* module as separate crates like tui-shimmer.

--

the agent was able to use `rm`

Run bash .. Timeout: 180 output

$ rm -rf /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/target && sleep 2 && cargo clippy 2>&1 | head -n 200

rm: /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/target/debug/.fingerprint: Directory not empty
rm: /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/target/debug: Directory not empty
rm: /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/target: Directory not empty
The lock file is being held by a previous process. Let me kill any cargo processes and try again.

-> for destructive commands, the vtcode agent should use human in the loop with persmission prompt

--

Nobody should be using up arrows to get previous commands in a terminal or shell. You have to move your hand and its linear complexity in keystrokes. Use ctrl+p (for low n) or ctrl+r. Use a real shell or history manager (fish, fzf, atuin) for ctrl+r.

-> implement pseudo control+r -> show a list of all previous commands with fuzzy search (check @files picker impl)

===

idea '/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/Screenshot 2026-01-28 at 2.43.32 PM.png'

---

'/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/Screenshot 2026-01-28 at 2.43.39 PM.png'

---

'/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/Screenshot 2026-01-28 at 11.19.28 AM.png'

--

review https://www.openresponses.org/specification carefully and update if needed
on feature/openresponse-api-spec branch

check vtcode-core/src/open_responses/integration.rs for full integration

✅ DONE - Open Responses Spec Compliance (2026-01-28)

**Spec Compliance Fixes:**
1. **Streaming Event Order** - Fixed content part lifecycle (added → delta → done → content_part.done)
2. **Extension Type Convention** - Custom items serialize with `custom_type` as actual `"type"` field
3. **Required Fields** - Removed `skip_serializing_if` from `error`, `usage`, `tools` (now serialize as `null`)
4. **Item Status** - Added `completed_message()` and `completed_function_call_output()` constructors
5. **Sequence Numbers** - Added `SequencedEvent<'a>` wrapper for SSE transport

**Main Flow Integration:**
- `HarnessEventEmitter` now supports Open Responses via `enable_open_responses()`
- Events are dual-written: harness JSONL + Open Responses SSE format
- Terminal `[DONE]` marker written on session finish
- Configured via `[agent.open_responses]` in vtcode.toml

**Files Changed:**
- vtcode-core/src/open_responses/{bridge,events,items,response,integration,mod}.rs
- src/agent/runloop/unified/inline_events/harness.rs
- src/agent/runloop/unified/turn/session_loop.rs

---

check Jetbrain (rustrover) ACP integration doesn't wokr

```
{
  "agent_servers": {
    "vtcode": {
      "command": "/Users/vinhnguyenxuan/.cargo/bin/vtcode",
      "args": ["acp"],
      "env": {
        "VT_ACP_ENABLED": "1",
//
//        "VT_ACP_ZED_ENABLED": "1",
      },
    },
  }
}
```

reference 
https://docs.factory.ai/integrations/jetbrains
https://agentclientprotocol.com/llms.txt