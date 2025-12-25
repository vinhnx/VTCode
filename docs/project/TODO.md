Add LSP (Language Server Protocol) tool for code intelligence features like go-to-definition, find references, and hover documentation

--

add support for custom session IDs when forking sessions with --session-id combined with --resume or --continue and --fork-session

--

improve ansi and background syntax highlighting for `diff` output. currently it lags

--

add wildcard syntax mcp**server**\* for MCP tool permissions to allow or deny all tools from a server

---

Improve memory usage for large conversations

--

fix IME (Input Method Editor) support for languages like Chinese, Japanese, and Korean by correctly positioning the composition window at the cursor

---

improve /doctor command output and format, also update new configuration options diagnosis <vtcode.toml> / conig

--

Add --agent CLI flag to override the agent setting for the current session

--

REVIEW AND REFINE VTCODE AGENT SYSTEM PROMPT AND TOOL USAGE INSTRUCTIONS, REDUCE AMBIGUITY, AND TOKEN COUNT, AIM FOR CLARITY AND CONCISENESS, IMPROVE TOOL USAGE EXAMPLES, AND ENSURE CONSISTENCY ACROSS ALL AGENT PROMPTS.

--

improve responsiveness when executing commands, tools, pty, and file operations. add a placeholder response while processing is ongoing.
