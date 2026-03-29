# VT Code Project Info

## What is VT Code?
VT Code is an open-source coding agent with LLM-native code understanding and robust shell safety. Supports multiple LLM providers with automatic failover and efficient context management.

## Key Features
- Multiple LLM provider support with automatic failover
- Rust-powered TUI for terminal-based coding assistance
- Efficient context management
- Robust shell safety

## Architecture
- 13-member Cargo workspace
- `vtcode-core/` — Reusable library with mode-based execution (77% complexity reduction)
- `src/` — CLI executable (Ratatui TUI, PTY, slash commands)
- `vtcode-tui/` — Terminal UI implementation
- `docs/` — Architecture docs, guides, and skill documentation

## Creator
- Built by Vinh Tuyen
- Part of the learn-by-doing initiative

## Repository
- Path: /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode
- Published to crates.io
