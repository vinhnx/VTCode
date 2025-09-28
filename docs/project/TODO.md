✅ COMPLETED: Full MCP integration with context7 and the inline termion renderer for improved TUI and terminal rendering:

> document are historical and should be updated or pruned as the inline renderer
> evolves.

- ✅ MCP allow list integrated with tools policy approval list
- ✅ Responsive UI for various terminal sizes with proper layout bounds checking
- ✅ MCP tool execution with proper error handling and event logging
- ✅ MCP events displayed as message blocks in chat interface
- ✅ Enhanced welcome message showing MCP status with enabled tools
- ✅ Fixed paste handling and scroll navigation with proper event handling
- ✅ Clean compilation with minimal warnings
- ✅ ANSI styling for MCP tool calls and status messages
- ✅ Improved terminal size responsiveness and text overflow handling
- ✅ Enhanced tool permission prompts with clear context
- ✅ MCP integration documented in AGENTS.md with usage best practices

---

idea: showing vtcode.toml config settings in an inline settings overlay

---

<https://docs.exa.ai/reference/exa-mcp>

---

Fix homebrew issue
<https://github.com/vinhnx/vtcode/issues/61>

brew install vinhnx/tap/vtcode
==> Fetching downloads for: vtcode
==> Fetching vinhnx/tap/vtcode
==> Downloading <https://github.com/vinhnx/vtcode/releases/download/v0.8.2/vtcode-v0.8.2-aarch64-apple-darwin.tar.gz>
curl: (56) The requested URL returned error: 404

Error: vtcode: Failed to download resource "vtcode (0.8.2)"
Download failed: <https://github.com/vinhnx/vtcode/releases/download/v0.8.2/vtcode-v0.8.2-aarch64-apple-darwin.tar.gz>
==> No outdated dependents to upgrade!

--

sync account with <https://vtchat.io.vn/>

---

vscode extenson <https://code.visualstudio.com/api/get-started/your-first-extension>

--

enhance realtime and terminal size view port changes, for example using in small panes and responsive ui in tui.

--

<https://docs.claude.com/en/docs/claude-code/hooks-guide>

---

<https://docs.claude.com/en/docs/claude-code/output-styles>

---

<https://docs.claude.com/en/docs/claude-code/settings>

--

benchmark terminal bench
<https://www.tbench.ai/>

--

<https://agentclientprotocol.com/overview/introduction>

--

mcp integration
<https://modelcontextprotocol.io/>

---

<https://github.com/mgrachev/update-informer>

--

Investigate and integrate context7 research to further improve the inline termion renderer, ensuring responsive and accurate UI/UX for various terminal sizes and use cases.

---

- Fix UI refresh issues in TUI:
  - Use MCP context7 to research best practices for forcing a full redraw on every message turn. Implement a reliable force-refresh after each message or tool output.
  - Compact and condense MCP tools output: avoid rendering excessive or verbose text in the TUI, especially for large tool responses. Summarize or truncate as needed for clarity.
  - Ensure terminal resizing and viewport changes trigger a full redraw and do not leave artifacts.

- Human-in-the-loop (HITL) prompt improvements:
  - Some confirmation prompts appear as empty blocks. Audit the HITL flow to ensure all confirmation dialogs have clear, actionable text.
  - The HITL prompt should always explain the action being confirmed and provide context (e.g., file changes, command execution).
  - Add fallback/default prompt text if the action description is missing.

- Encourage use of MCP (especially context7) for enhanced context awareness, memory, and journaling. Update agents.md and memory routines to reflect this.

- Regularly update memory for important points and decisions.

- Test with various terminal sizes and edge cases to ensure robust UI/UX.

---

IMPORTANT: Refine the system prompt instructions for the vtcode agent to explicitly encourage and remind the agent to leverage MCP (Model Context Protocol), especially context7, whenever enhanced context awareness, memory, or journaling is beneficial for a task.

---

> Legacy note: Historical troubleshooting steps for the previous renderer were removed now that the inline termion implementation is the primary UI.
