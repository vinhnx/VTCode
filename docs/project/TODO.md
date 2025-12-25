Add LSP (Language Server Protocol) tool for code intelligence features like go-to-definition, find references, and hover documentation

--

enhance /sessions command to
add support for custom session IDs when forking sessions with --session-id combined with --resume or --continue and --fork-session

--

improve sessions resume to clear current session and open in full tui mode with transcription view when resuming from a previous session that was in full tui mode

eg: current it just open the summarization in stead.Owner

Full TUI sessions restore

Currently when using /sessions command, the program only return one summary final message. Try to find a way to fully render all message in transcript log as TUI messages.

-   (ID: session-vtcode-20251225T103836Z_356523-71490) 2025-12-25
    17:38 · Model: zai-org/GLM-4.7:novita · Workspace: vtcode

Duration: 16s · 1 msgs · 0 tools

Prompt: 1

File: /Users/vinhnguyenxuan/.vtcode/sessions/session-vtcode
-20251225T103836Z_356523-71490.json

-   (ID: session-vtcode-20251225T103107Z_704712-65298) 2025-12-25
    17:34 · Model: zai-org/GLM-4.7:novita · Workspace: vtcode

Duration: 3m 44s · 4 msgs · 0 tools

Prompt: hello

Reply: Hello! I'm VT Code, your Rust-based terminal coding
agent. I'm here to help you…

File: /Users/vinhnguyenxuan/.vtcode/sessions/session-vtcode
-20251225T103107Z_704712-65298.json

Closed session browser.

--

improve ansi and background syntax highlighting for `diff` output. currently it lags

--

add wildcard syntax mcp**server**\* for MCP tool permissions to allow or deny all tools from a server

---

Improve memory usage for large conversations

--

fix IME (Input Method Editor) support for languages like Chinese, Japanese, and Korean by correctly positioning the composition window at the cursor

---

--

REVIEW AND REFINE VTCODE AGENT TOOLS USAGE INSTRUCTIONS, REDUCE AMBIGUITY, AND TOKEN COUNT AND WORDING, AIM FOR CLARITY AND CONCISENESS, IMPROVE TOOL USAGE EXAMPLES, AND ENSURE CONSISTENCY ACROSS ALL AGENT PROMPTS.

--

REVIEW VTCODE TOOLS, REMOVE REDUNDANT TOOLS, AND IMPROVE TOOL DESCRIPTIONS FOR CLARITY AND CONCISENESS. REMOVE UNUSED TOOLS, REDUCE NUMBER OF TOOLS, AND ENSURE TOOL DESCRIPTIONS ARE CLEAR AND TO THE POINT. CURRENTLY, THERE ARE TOO MANY TOOLS WHICH MAY CAUSE AGENT CONFUSION. AIM TO STREAMLINE TOOLSET. AND KEEP CORE FUNCTIONALITY AND SYSTEM PROMPT ALIGNED. REVIEW CAREFULLY.

--

improve responsiveness when executing commands, tools, pty, and file operations. add a placeholder response while processing is ongoing.

--

COMPLETED: improve /doctor command output and format, also update new configuration options diagnosis <vtcode.toml> / config
- Organized section layout: [Core Environment], [Configuration], [API & Providers], [Dependencies], [External Services], [Workspace Links], [Skills]
- Configuration diagnostics: theme, model, max_turns, context_tokens, token_budget, decision_ledger, max_tool_loops, human_in_the_loop, tool_policy, pty_enabled
- Skills diagnostics: Shows loaded skills with count and scope indicators (user/repo)
- Enhanced formatting: Unicode indicators (check/x) with visual separators
- Improved linked directory display with indices and actual paths
- Failure suggestions: Each failed check shows contextual help arrow with resolution steps
- Recommended next actions section at the end with quick guidance
- Added documentation: DOCTOR_IMPROVEMENTS.md, DOCTOR_REFERENCE.md, DOCTOR_FAILURE_SUGGESTIONS.md
- All code compiled and tested successfully
