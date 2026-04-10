scan for large monolithic files and functions and break them down into smaller, more focused functions that adhere to the Single Responsibility Principle. This will enhance readability, maintainability, and testability of the codebase.

---

fix copy, yank text is broken block on scroll. for example: when user already select some text, then scroll the trascript up and down in the terminal, the highlight block is broken and becomes unaligned with the text, making it difficult for users to see what they have selected. This issue can be frustrating for users who rely on the copy and yank functionality to quickly capture important information from the transcript. To fix this, we need to ensure that the highlight block remains properly aligned with the selected text even when the user scrolls through the transcript. This may involve adjusting the way the terminal handles text rendering and selection, ensuring that the highlight block is dynamically updated as the user scrolls, and testing across different terminal environments to ensure consistent behavior. By addressing this issue, we can improve the overall user experience and make it easier for users to interact with the transcript effectively.

---

Advisor tool

https://claude.com/blog/the-advisor-strategy

https://platform.claude.com/docs/en/agents-and-tools/tool-use/advisor-tool#suggested-system-prompt-for-coding-tasks

---

https://x.com/bcherny/status/2042352728094245016

---

Top 25 longest tracked Rust files:
| Lines | Path |
| ---: | --- |
| 6967 | vtcode-tui/src/core_tui/session/tests.rs |
| 4472 | vtcode-core/src/subagents/mod.rs |
| 4462 | vtcode-core/src/llm/providers/openai/provider/tests.rs |
| 4010 | vtcode-core/src/persistent_memory.rs |
| 3801 | src/agent/runloop/unified/turn/compaction.rs |
| 2851 | src/agent/runloop/unified/turn/tool_outcomes/execution_result.rs |
| 2668 | vtcode-core/src/utils/session_archive.rs |
| 2563 | src/agent/runloop/unified/turn/turn_processing/llm_request/copilot_runtime.rs |
| 2446 | vtcode-core/src/tools/skills/mod.rs |
| 2363 | vtcode-core/src/components.rs |
| 2332 | vtcode-core/src/copilot/acp_client.rs |
| 2253 | vtcode-core/src/scheduler/mod.rs |
| 2236 | src/agent/runloop/unified/tool_routing/mod.rs |
| 2204 | vtcode-core/src/tools/structural_search.rs |
| 2173 | src/agent/runloop/unified/turn/session_loop_runner/mod.rs |
| 2076 | src/agent/runloop/unified/turn/session/slash_commands/agents.rs |
| 2056 | vtcode-core/src/core/agent/runner/tests.rs |
| 2055 | vtcode-core/src/tools/registry/executors.rs |
| 1968 | vtcode-core/src/commands/init.rs |
| 1939 | vtcode-core/src/utils/ansi.rs |
| 1938 | vtcode-core/src/open_responses/bridge.rs |
| 1863 | vtcode-core/src/llm/providers/gemini/tests.rs |
| 1846 | src/agent/runloop/unified/turn/session/slash_commands/ui.rs |
| 1830 | vtcode-core/src/tools/handlers/plan_mode.rs |
| 1824 | vtcode-core/src/skills/executor.rs |
