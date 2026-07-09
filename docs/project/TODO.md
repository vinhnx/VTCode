implement claude advisor tool

--

context engineering: managing the context the model sees at each inference step, so that multi-turn tool use is not drowned by history, noise, tool definitions, and intermediate results.

===

as tasks evolved into long-horizon tasks, agent capability became a system capability composed of the model, harness, context, tools, evals, sandbox, and state management together: within a finite context, external tools, and a changing environment, the agent must keep pushing toward a goal and get closer to completion over time.

===

A concrete context engineering example is: when there are too many MCP tools and too much data, we should not let every tool definition and intermediate result flow through context. Instead, the agent should write code to call tools on demand, filter the data, and bring only high-signal results back to the model. Context engineering also includes compaction, structured note-taking, and even multi-agent patterns, but these mostly serve the question of “how should context be managed?” The long-running agent harness discussed later goes one step further: it does not only manage context, but separates planning, implementation, evaluation, revision, and final acceptance into different processes.

---

Context engineering

People used to focus on prompt engineering, namely how to write the system prompt. But as agents become more multi-turn and longer-running, the focus becomes: at each inference step, what did the model see? For example, an agent’s context may contain the system prompt, developer instructions, tool definitions, MCP server descriptions, user messages, conversation history, file contents, command output, browser observations, screenshots, errors, progress notes, retrieved documents, evaluator feedback. These things cannot all be blindly stuffed in. Even if the context window becomes larger, we still need to choose selectively, because noise in a long context distracts the model and increases cost and latency. So a good context is the minimal high-signal context that lets the model complete the next step at the current step. In general, we use just-in-time context: instead of stuffing all documents, tools, and history into the model, we keep references, such as paths, URLs, commit hashes, log files, database table names, and so on. When the model needs them, it loads them through tools. In this way, the agent’s working memory keeps indexes and currently relevant fragments, rather than all of the context.

Common context techniques in long tasks include:

    Compaction: When the context is almost full, compress the current conversation trace into a summary, then use that summary to start the next context. Usually this is still the continuation of the same task and the same agent loop. Its goal is to preserve conversational continuity as much as possible. In other words, the model should feel that “what I was just doing is still here”; only the history has been compressed.

    Structured note-taking: Let the agent actively write task state into persistent external artifacts, such as progress.md, feature_list.json, NOTES.md, or git commits. It is not necessarily only for the same task. A project-level NOTES.md can be read by later sessions, by planner/generator/evaluator, or reused in neighboring tasks.

    Context reset: Use external artifacts, such as files produced by note-taking, git logs, test results, task lists, and so on, as startup material to open a clean new context/session. It does not preserve the full conversation history, and can clear noise and bad assumptions so that a new agent can reorient itself.

    Multi-agent architecture: Do role division and context isolation. Different agents receive different context, do different subtasks, and finally pass only high-density results back to the main flow.

===

Tool use

There are also some things to watch out for in tool use. For example, the number of tools cannot expand without limit, otherwise the model wastes attention on choosing. Tools should not overlap too much in functionality, otherwise the model does not know which one to use. Tool descriptions should clearly explain when to use them and when not to use them, namely they should make the boundaries clear. Return values should be token-efficient and should not pour irrelevant data into context. Returned error messages should let the model know what to repair next. Names should be clear, so the agent can infer boundaries from the names. Also, tools are preferably not exposed directly to the model as direct tool calls, but mapped into code APIs / a file tree, so the agent can call them through code. This way, the agent can read only the tool definitions it currently needs, filter large data inside the code execution environment, and return only summaries or a small number of results to the model. It can use loops, conditionals, and exception handling to complete complex control flow, or write intermediate state into files instead of stuffing it into context. Tool outputs are deterministic; we should leave deterministic computation to code. Judgment and planning, such as how to call tools, which tools to call, in what order, and how many times, should be left to the model.

===

https://jinyansu1.github.io/blog/2026/07/agent-context-engineering-long-running-harness/

===

check current existing .logs/.checkpoint/.sessions infra, and see if they're redudant or can be unified. The goal is to have a single source of truth for the agent's state, context, and history, which can be efficiently queried and updated as the agent progresses through its tasks. This may involve consolidating logs, checkpoints, and session data into a more streamlined format that supports both real-time inference and long-term learning. It's also prevent overhead from accumulating in the context, ensuring that the runtime environment remains responsive and efficient.

===

IMPORTANT for vtcode's compaction engine. Compaction: When the context is almost full, compress the current conversation trace into a summary, then use that summary to start the next context. Usually this is still the continuation of the same task and the same agent loop. Its goal is to preserve conversational continuity as much as possible. In other words, the model should feel that “what I was just doing is still here”; only the history has been compressed.

===

IMPORTANT CRITICAL: review vtcode's tool systems. the number of tools cannot expand without limit, otherwise the model wastes attention on choosing.

Tool use

There are also some things to watch out for in tool use. For example, the number of tools cannot expand without limit, otherwise the model wastes attention on choosing. Tools should not overlap too much in functionality, otherwise the model does not know which one to use. Tool descriptions should clearly explain when to use them and when not to use them, namely they should make the boundaries clear. Return values should be token-efficient and should not pour irrelevant data into context. Returned error messages should let the model know what to repair next. Names should be clear, so the agent can infer boundaries from the names. Also, tools are preferably not exposed directly to the model as direct tool calls, but mapped into code APIs / a file tree, so the agent can call them through code. This way, the agent can read only the tool definitions it currently needs, filter large data inside the code execution environment, and return only summaries or a small number of results to the model. It can use loops, conditionals, and exception handling to complete complex control flow, or write intermediate state into files instead of stuffing it into context. Tool outputs are deterministic; we should leave deterministic computation to code. Judgment and planning, such as how to call tools, which tools to call, in what order, and how many times, should be left to the model.

===

Fix plan: tool-free recovery dies instead of retrying synthesis (checkpoint turn_621)

Root cause chain

1. Turn hit the 600s tool wall-clock budget mid-exploration; harness correctly switched to a tool-free recovery pass ([run_loop_context.rs#L154](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/src/agent/runloop/unified/run_loop_context.rs#L154)).
2. The model emitted its answer as text containing <tool_call>…</tool_call> markup. In [result_handler.rs#L204-L272](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/src/agent/runloop/unified/turn/turn_processing/result_handler.rs#L204-L272), the two cleanup attempts (strip_dsml_markup, strip_textual_tool_call_regions) failed, so it broke with RECOVERY_CONTRACT_VIOLATION_REASON — with no retry.
3. normalize_tool_free_recovery_break_outcome ([post_tool_recovery.rs#L145-L165](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/src/agent/runloop/unified/turn/turn_loop/post_tool_recovery.rs#L145-L165)) immediately converted that to the canned final answer "Recovery synthesis failed; no tool call applied…", discarding ~60 messages of gathered context.

Contrast: the Empty + native-tool-calls path in [turn_loop.rs#L862-L878](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/src/agent/runloop/unified/turn/turn_loop.rs#L862-L878) does retry up to MAX_RECOVERY_RETRIES (3) with a corrective directive. The textual-markup path and the ToolCalls-variant path ([result_handler.rs#L112-L118](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/src/agent/runloop/unified/turn/turn_processing/result_handler.rs#L112-L118)) get zero retries.

Contributing factors

- RECOVERY_SYNTHESIS_MAX_TOKENS = 1024 ([turn_loop.rs#L68](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/src/agent/runloop/unified/turn/turn_loop.rs#L68)) — far too small to synthesize a launch-time plan from a 60-message exploration; likely truncates and destabilizes the response.
- Three stacked, partly contradictory system messages injected before recovery (budget exhausted / synthesize now, POST_TOOL_RESUME_DIRECTIVE "follow tool guidance first", POST_TOOL_RECOVERY_REASON).
- The turn burned budget on 3 identical unified_search preflight validation failures (invalid content arg) with no corrective schema hint injected.

Changes

1. Retry instead of bail on recovery contract violation (primary fix)

- In result*handler.rs, both Blocked-with-RECOVERY_CONTRACT_VIOLATION_REASON sites: instead of breaking immediately, route through the same retry mechanism as the Empty path — if recovery_retry_count() < MAX_RECOVERY_RETRIES, push RECOVERY_TOOL_CALL_RETRY_DIRECTIVE, call retry_recovery_pass(), and continue the loop. Only after retries exhaust fall through to the fallback. This likely needs a new TurnHandlerOutcome/TurnLoopResult signal (e.g. Blocked reason checked in turn_loop.rs before normalize*… converts it) so the retry counter lives in one place.

2. Salvage a real answer in the fallback

- In complete_turn_after_failed_tool_free_recovery, before pushing the canned string: if the rejected response's markup-stripped text has non-trivial prose (even if markers remain), use that as the final answer with a [!] recovery note, rather than discarding it. Requires threading the last rejected text into the fallback (or storing it on the turn ctx).

3. Raise recovery synthesis token budget

- Bump RECOVERY_SYNTHESIS_MAX_TOKENS from 1024 to ~4096. Synthesizing a plan from a long exploration cannot fit in 1024.

4. Consolidate recovery system messages

- In prepare_post_tool_tool_free_recovery / budget-exhaustion path, emit one coherent directive instead of stacking POST_TOOL_RESUME_DIRECTIVE (which tells the model to follow tool-output guidance/next_action — wrong when tools are disabled) plus the recovery reason. When tool-free recovery is active, suppress POST_TOOL_RESUME_DIRECTIVE.

5. (Secondary) Preflight-failure schema hint

- After a repeated identical preflight validation failure for the same tool, append the tool's valid parameter list to the error payload so the model self-corrects instead of retrying blind. Scope: wherever the "Tool preflight validation failed" error is built.

Verification

- Unit tests in turn_loop/tests.rs / result_handler.rs tests: recovery pass returning textual tool-call markup retries up to MAX_RECOVERY_RETRIES, then falls back; fallback prefers salvaged prose over canned string; POST_TOOL_RESUME_DIRECTIVE absent when tool-free recovery active.
- ./scripts/check-dev.sh --test, plus cargo nextest run -p vtcode -E 'binary(/inline_events/)' for harness regressions.

Items 1–4 are one coherent change in src/agent/runloop/unified/turn/; item 5 is separable. Want me to implement 1–4 now?

--> use amp fable

===

https://github.com/vinhnx/VTCode/issues/698

---

IMPORTANT: when the agent loop processing is running, prevent switch mode (e.g. from tool-free to tool-enabled) from happening mid-turn. The agent loop should complete the current turn before switching modes, to avoid inconsistent state and context corruption. This may involve locking the mode state during turn processing or queuing mode switch requests until the turn is fully processed. also disable keyboard shortcuts that can trigger mode switches during active processing, to ensure that the agent's state remains stable and predictable.

---

check vtcode reasoning trace, it seems to have a lot of blank lines. make it more compact and readable, by removing unnecessary whitespace and formatting the output in a structured way. This will help in debugging and understanding the agent's decision-making process. Consider implementing a logging utility that formats the reasoning trace consistently, highlighting key decisions, tool calls, and context changes.

---
