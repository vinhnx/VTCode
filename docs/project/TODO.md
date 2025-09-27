idea: showing vtcode.toml config settings in ratatui modal


--

https://blog.nilenso.com/blog/2025/09/25/swe-benchmarks/

---



https://platform.openai.com/docs/guides/function-calling
improve function calling openai


--



https://docs.exa.ai/reference/exa-mcp

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

submit r/rust/

--

submit hackernew


---

fix not to execessive force screen refresh to improve performant

---

check scresnshot. whenever the human-in-the-loop prompt appear. the screen render is broken. text and visual are glitch. fix this is important

---

Review all TUI-related changes between version git v0.13.7 and 0.15.0. Version v0.13.7 had stable rendering without visual glitches. Compare the implementation details, especially around rendering and event handling. If regressions or glitches are present in 0.15.0, revert to the v0.13.7 approach for those areas while retaining any necessary updates from 0.15.0. Ensure the core logic and stable rendering behavior from v0.13.7 are preserved in the latest version.

-- 

update docs and readme about recent tui changes
