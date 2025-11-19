# VT Code System Prompt Reference

## Overview

VT Code is a Rust-based terminal coding agent with modular architecture supporting multiple LLM providers (Gemini, OpenAI, Anthropic) and tree-sitter parsers for 6+ languages.

This document provides an overview of the system prompt strategy. The actual system prompts are defined in `vtcode-core/src/prompts/system.rs`.

## Core Strategy

**Tone**: No preamble, no postamble. Direct answers only. No emojis or visual symbols.

**Execution**: Understand → Gather Context → Execute → Verify → Reply

**Persistence**: Once committed to a task, maintain focus until completion.

**Efficiency**: Hard thresholds on tool reuse (2+ same calls = STOP). Cache results, don't re-search.

## System Prompt Variants

### Default System Prompt
- **Use**: Primary agent for all tasks
- **Focus**: Complete prompt with detailed tool selection, persistence patterns, error handling
- **Length**: ~340 lines
- **Key Sections**: Execution algorithm, tool tiers, loop prevention, behavioral requirements

### Lightweight System Prompt
- **Use**: Resource-constrained scenarios, simple operations
- **Focus**: Core principles only, minimal verbose explanations
- **Length**: ~57 lines
- **Key Sections**: Execution steps, tool selection, loop detection essentials

### Specialized System Prompt
- **Use**: Complex multi-file refactoring, sophisticated analysis, advanced transformations
- **Focus**: Deep understanding, systematic planning, edge case handling
- **Length**: ~100 lines
- **Key Sections**: Complex task management, persistent status reporting, context management

## Tool Selection Quick Guide

| Need | Tool | Use Case |
|------|------|----------|
| Exact filename | `list_files(mode="find_name")` | Know exact file |
| File pattern | `list_files(mode="recursive")` | Know pattern (*.md) |
| File contents | `grep_file(pattern="...")` | Search code |
| Directory | `list_files(mode="list")` | Explore structure |
| Edit file | `edit_file()` | Targeted change |
| Rewrite file | `write_file()` | 50%+ changes |
| Run command | `run_pty_cmd()` | One-off (cargo, git, npm) |
| Interactive | `create_pty_session()` | Multi-step (gdb, REPL) |
| Process data | `execute_code()` | Filter/transform 100+ items |
| Track progress | `update_plan()` | 4+ step complex task |

## Loop Prevention Rules

**Hard Thresholds (STOP immediately):**
- Same tool + same params 2+ times → Try different approach
- 10+ tool calls without progress → Explain blockage
- File search unsuccessful after 3 attempts → Switch method

**Always:**
- Remember discovered file paths
- Cache search results
- Once solved, STOP
- Don't repeat outputs already shown

## Command Execution

**Default**: Use `run_pty_cmd` for all one-off commands

**Non-Retryable Errors:**
- Exit code 127 = Command not found (permanent)
- Exit code 126 = Permission denied
- `do_not_retry: true` = Fatal error
- Do NOT retry with different shells or diagnostics

**Interactive**: Use PTY sessions only for gdb, REPL, vim, step-by-step debugging

## Behavioral Requirements

- Search BEFORE reading files (never read 5+ without searching)
- Do NOT add comments unless asked
- Do NOT generate/guess URLs
- Ask confirmation for destructive operations
- Maintain focus on complex tasks
- Use consistent approach across similar requests

## Integration with AGENTS.md

The system prompts automatically load and incorporate AGENTS.md instructions with the following precedence:

1. System prompts (safety, core behavior)
2. Developer preferences (vtcode.toml)
3. User requests (current session)
4. AGENTS.md guidelines (workflow specifics)

When conflicts exist, higher-numbered entries override lower-numbered ones.

## Source Files

- **Main Prompt Definition**: `vtcode-core/src/prompts/system.rs`
- **Functions**:
  - `default_system_prompt()` → DEFAULT_SYSTEM_PROMPT
  - `generate_lightweight_instruction()` → DEFAULT_LIGHTWEIGHT_PROMPT
  - `generate_specialized_instruction()` → DEFAULT_SPECIALIZED_PROMPT

## Key Differences from Typical Prompts

1. **Emoji-Free**: All visual symbols removed; uses text labels (ANTI:, GOOD:)
2. **Hard Rules**: Loop detection thresholds are absolute, not suggestions
3. **Persistence Required**: Agents must follow through on committed tasks
4. **Minimal Output**: Responses must be direct; no explanations unless asked
5. **Efficiency Focused**: Tool selection optimized for token savings and speed
