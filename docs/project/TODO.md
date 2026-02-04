[DON'T DELETE UNTIL FEEL COMPLETE] review duplicated and redundent logic from whole code base and remove and cleanup AND DRY.

continue with your recommendation, proceed with outcome. don't stop. review overall progress and changes again carefully, can you do better? go on don't ask me

---

# Andrej Karpathy Skills

Behavioral guidelines to reduce common LLM coding mistakes. Merge with project-specific instructions as needed.

**Tradeoff:** These guidelines bias toward caution over speed. For trivial tasks, use judgment.

## 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:

- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

## 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

## 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:

- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:

- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

## 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:

- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:

```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

---

**These guidelines are working if:** fewer unnecessary changes in diffs, fewer rewrites due to overcomplication, and clarifying questions come before implementation rather than after mistakes.

--

Improve tool display

verb:

ongoing
• Explore
└ Read discovery.rs

finished
• Explored
└ Read discovery.rs

improve the prefix bullet: "•" before tool, and indent the sub-bullets. "└" for sub-bullets tools arguments.

add ">" before reasoning message.

--

https://github.com/vinhnx/VTCode/issues/595

I installed the latest version and ran it. The previous JSON and URL issues seem resolved, but now I am encountering a new error during tool calls.

Error Message:

LLM request failed: Invalid request: Anthropic Invalid request: tool_call_id is not found

It seems that when sending the tool result back to the API, the required tool_call_id is missing or not being correctly mapped.

I asked vtcode to push to git, but when it ran git status, I encountered the following error:

LLM request failed: Invalid request: Anthropic Invalid request: failed to convert tool result content: unsupported content type in ContentBlockParamUnion: json

--

Background terminal Run long-running terminal commands in the background.

---

Shell snapshot Snapshot your shell environment to avoid re-running
login scripts for every command.

---

curl https://router.huggingface.co/v1/chat/completions \
 -H "Authorization: Bearer $HF_TOKEN" \
 -H 'Content-Type: application/json' \
 -d '{
"messages": [
{
"role": "user",
"content": "What is the capital of France?"
}
],
"model": "Qwen/Qwen3-Coder-Next:novita",
"stream": true
}'

to novita huggingface inference provider.

--

1. dimmed the message's divider line

2. for warnings and info message feedbacks in the transcript. add Borders like error message. '/Users/vinhnguyenxuan/Desktop/Screenshot 2026-02-04 at 11.31.47 AM.png'

3. For each error, warning, info, show the message info type in the BORDER box https://ratatui.rs/examples/widgets/block/
