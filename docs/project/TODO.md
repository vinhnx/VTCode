benchmark terminal bench
https://www.tbench.ai/

--

https://x.com/vbingliu/status/1969460781495566611?s=46

--

implement planning mode and TODO list (research)

--

https://agentclientprotocol.com/overview/introduction

--

https://github.com/Stebalien/term

A Rust library for terminfo parsing and terminal colors.

--

mcp integration
https://modelcontextprotocol.io/

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
