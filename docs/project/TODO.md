
Add /settings as an alias for the /config command

--



Add LSP (Language Server Protocol) tool for code intelligence features like go-to-definition, find references, and hover documentation

--

add guidance for macOS users when Alt shortcuts fail due to terminal configuration

-


fix skill allowed-tools not being applied to tools invoked by the skill


--

fix a potential crash when syntax highlighting isn't initialized correctly

--

add support for custom session IDs when forking sessions with --session-id combined with --resume or --continue and --fork-session

--

implement input history cycling

--

fix slow input history cycling and race condition that could overwrite text after message submission

--

Reduced terminal flickering and scrolling performance issues for large and long transcripts. improve cpu and memory usage

---

add loading indicator when resuming conversations for better feedback

--

add loading indicator when using skills

--

fix permission rules incorrectly rejecting valid bash commands containing shell glob patterns (e.g., ls *.txt, for f in *.png)

--

improve ansi and background syntax highlighting for `diff` output. currently it lags

--

add wildcard syntax mcp__server__* for MCP tool permissions to allow or deny all tools from a server

---

Improve memory usage for large conversations

--

fix IME (Input Method Editor) support for languages like Chinese, Japanese, and Korean by correctly positioning the composition window at the cursor

---

Improve plan mode exit UX: show simplified yes/no dialog when exiting with empty or missing plan instead of throwing an error

---

Add search functionality to /permissions command with / keyboard shortcut for filtering rules by tool name

---

improve /doctor command output and format, also update new configuration options diagnosis <vtcode.toml> / conig

---

add command "/mcp enable [server-name]" or "/mcp disable [server-name]" to quickly toggle all servers

--

Update Fetch to skip summarization for pre-approved websites

--

Add --agent CLI flag to override the agent setting for the current session

--

Plan Mode should builds more precise plans and executes more thoroughly

--

Fix handling of thinking errors