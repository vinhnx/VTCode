# Tools Configuration

This document describes the tools-related configuration in `vtcode.toml`.

- max_tool_loops: Maximum number of inner tool-call loops per user turn. Set to `0` to disable the limit and rely on the other turn safeguards.
  - Configuration: `[tools].max_tool_loops` in `vtcode.toml`
  - Code default: defined in `vtcode-config/src/core/tools.rs`
  - Default: `0`

Example:

```toml
[tools]
default_policy = "prompt"
max_tool_loops = 0
```


Tool outputs are rendered with ANSI styles in the chat interface. Tools should return plain text.
