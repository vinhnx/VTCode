<https://newsletter.pragmaticengineer.com/p/how-claude-code-is-built>

--

reference <https://x.com/arafatkatze/status/1970171291295506491>

--

benchmark terminal bench
<https://www.tbench.ai/>

--

<https://agentclientprotocol.com/overview/introduction>

---

benchmark terminal bench
<https://www.tbench.ai/>

--

mcp integration
<https://modelcontextprotocol.io/>

---

<https://github.com/mgrachev/update-informer>

---

<https://github.com/catppuccin/rust>

--

<https://github.com/catppuccin/rust/blob/main/examples/ratatui.rs>

=-

<https://github.com/catppuccin/rust/blob/main/examples/serde.rs>

--

<https://github.com/catppuccin/rust/blob/main/examples/term_grid.rs>

--

fix viewport for the whole vtcode viewport to appear inline in terminaml not as fullscreen. fetch and read and fix vtcode inline presenetation

1. <https://ratatui.rs/examples/apps/inline/>
2. <https://docs.rs/ratatui/latest/ratatui/enum.Viewport.html#variant.Inline>

All I need to do was to create terminal with viewport

let mut terminal = Terminal::with_options(
backend,
TerminalOptions {
viewport: Viewport::Inline(8),
},
)?;

---

document context:

- <https://deepwiki.com/ratatui/ansi-to-tui>
- <https://crates.io/crates/ratatui/>
- <https://docs.rs/ratatui/latest/ratatui/>

--

check prompt_tool_permission should show a tui action prompt form

--

check todo list render

---

research and implement Prompt caching system to save costs. use web search for document on specific provider and model support. add this as a conigurable feature in vtcode.toml.

---
