scan for large monolithic files and functions and break them down into smaller, more focused functions that adhere to the Single Responsibility Principle. This will enhance readability, maintainability, and testability of the codebase.

---

Advisor tool

https://claude.com/blog/the-advisor-strategy

https://platform.claude.com/docs/en/agents-and-tools/tool-use/advisor-tool#suggested-system-prompt-for-coding-tasks

---

Top 25 longest tracked Rust files:
| Lines | Path |
| ---: | --- |

/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/target/package/vtcode-0.98.0/src/agent/runloop/unified/turn/tool_outcomes/execution_result.rs


1/ Execute a comprehensive, line-by-line audit of the `file` to systematically identify and resolve optimization opportunities, prioritizing efficiency, scalability, and maintainability. Rigorously enforce the DRY (Don't Repeat Yourself) principle by detecting and eliminating all duplicated or redundant logic, consolidating patterns into reusable, modular components. Validate strict alignment between agent loops, tool calls, and system prompts, ensuring consistency in logic flow, error handling, and state management. Refactor the agent harness and core execution logic to enforce autonomous yet safe tool execution, incorporating robust validation, fallback mechanisms, and rate-limiting. Adhere to best practices regarding modular design, separation of concerns, and minimal dependency overhead. Exclude all non-code deliverables—such as summaries or documentation—and output only the fully optimized, refactored code.

2/ reduce, extract simplify < 2000 lines

3/ Use /rust-skills and enhance implementation. Review overall changes again carefully. Can you do better? Continue with your careful recommendations; proceed with the outcome. KISS and DRY, but focus on main logic; no need for DRY for tests. Do it repeatedly until all is done; don't stop.

===

Ctrl+O now copies the last agent response as markdown - works everywhere, even over SSH. I've been using it nonstop to grab plans for review. https://github.com/openai/codex/pull/16966

===

https://deepwiki.com/search/how-does-tree-work-and-how-pi_13a0d64b-8d67-4c6a-bfd7-41de92a7261c?mode=fast

===

https://deepwiki.com/search/how-does-load-works-and-how-pi_1713c944-fe9f-4863-a3c0-3d695d5ca7ae?mode=fast

---

improve genering patch UX.

example: [spinner] Generating patch (825 lines) in execution_result.rs
