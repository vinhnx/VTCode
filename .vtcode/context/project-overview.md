# Project Overview - VT Code

VT Code is an open-source coding agent with LLM-native code understanding and robust shell safety.

## Key Features
- Multi-LLM support with automatic failover (OpenAI, Anthropic, local models)
- Terminal-based TUI (Ratatui)
- Delegated subagent system (13+ specialized agents)
- Shell safety guards
- Slash commands (`/terminal-setup`, `/vim`, `/agents`, etc.)
- Skills framework (1400+ indexed skills)
- Protocol extensions (Open Responses, MCP, Zed ACP)
- OAuth 2.0 authentication with OS keychain storage

## Architecture
- **13-member Cargo workspace**
- `src/` — CLI executable (TUI, PTY, slash commands)
- `vtcode-core/` — Reusable library (~77% complexity reduction via mode-based execution)
- `docs/` — Architecture docs, guides, protocols

## References
- README: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/README.md`
- Agent guidance: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/AGENTS.md`
