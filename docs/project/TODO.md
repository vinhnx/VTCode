https://newsletter.pragmaticengineer.com/p/how-claude-code-is-built

--

reference <https://x.com/arafatkatze/status/1970171291295506491>

--

benchmark terminal bench
<https://www.tbench.ai/>

--

<https://x.com/vbingliu/status/1969460781495566611?s=46>

--

implement planning mode and TODO list (research)

--

<https://agentclientprotocol.com/overview/introduction>

--

<https://github.com/Stebalien/term>

A Rust library for terminfo parsing and terminal colors.

--

mcp integration
<https://modelcontextprotocol.io/>

---

benchmark terminal bench
<https://www.tbench.ai/>

--

<https://x.com/vbingliu/status/1969460781495566611?s=46>

--

implement planning mode and TODO list (research)

--

<https://agentclientprotocol.com/overview/introduction>

--

<https://github.com/Stebalien/term>

A Rust library for terminfo parsing and terminal colors.

--

mcp integration
<https://modelcontextprotocol.io/>

---

this <https://crates.io/crates/tui-term>

---

<https://github.com/mgrachev/update-informer>

---

<https://crates.io/crates/tui-scrollview>

---

<https://github.com/catppuccin/rust>

--

<https://github.com/catppuccin/rust/blob/main/examples/ratatui.rs>

=-

<https://github.com/catppuccin/rust/blob/main/examples/serde.rs>

--

<https://github.com/catppuccin/rust/blob/main/examples/term_grid.rs>
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

check <https://chatgpt.com/codex/tasks/task_e_68d0d1a220e883239b47587dd9dc9a8f> and apply each one again

--

implement and enhance tui

1. allow mouse selection in tui
2. change the "vt code" logo to #D99A4E
3. remove tui-term crate for pty. for pty terminmal pseudo bash_command, run_terminal_cmd. render inside a rounded border block <https://ratatui.rs/examples/widgets/block/>
4. revamp and make the status bar more concise and compact. less wording and make sure fit content. only show important infos
5. remove the "Agent" message block if no agent message
6. add chat user input with block rartui with decorated blocktype == rounded border
7. integrate fully ANSI-to-TUI: <https://github.com/ratatui/ansi-to-tui> (integrate to render agent responses with colors/styles from existing CLI ANSI output, including traces and statuses).
8. don't show cursor while agent is thinking or spinning view is shown. show cursor only if idle
9. move chat input to messgaes block in chat cell list. not fixed at bottom
10. add placeholder text to user chat input "Implement {feature}..."

---

document context:

-   <https://deepwiki.com/ratatui/ansi-to-tui>
-   <https://crates.io/crates/ratatui/>
-   <https://docs.rs/ratatui/latest/ratatui/>

--

check prompt_tool_permission should show a tui action prompt form

--

check todo list render

---
