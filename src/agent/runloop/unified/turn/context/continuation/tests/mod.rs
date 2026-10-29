pub(super) use super::*;
pub(super) use crate::agent::runloop::unified::run_loop_context::RecoveryMode;
pub(super) use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnLoopResult};
pub(super) use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;

fn checkpoint_shaped_repo_overview() -> &'static str {
    "Here's a quick overview of the repo:\n\n\
    **VT Code** — a Rust coding agent for long-running autonomous workflows, with OS-native sandboxing, multi-provider LLM support, open protocols, and extensible Skills.\n\n\
    **Layout**\n\
    - Root crate `vtcode` (binary) + workspace of ~30 member crates under `vtcode-*` (e.g. `vtcode-core`, `vtcode-ui`, `vtcode-llm`, `vtcode-mcp`, `vtcode-safety`, `vtcode-exec-events`, `vtcode-indexer`, `vtcode-skills`, `vtcode-config`, `vtcode-a2a`, `vtcode-acp`, etc.)\n\
    - Rust stable, MSRV 1.99.1, edition 2024; CI runs `RUSTFLAGS=\"-D warnings\"` with `--locked`\n\
    - `default-members` = root, `vtcode-core`, `vtcode-ui`\n\n\
    **Capabilities**\n\
    - Agent runtime: interactive TUI, slash commands, streaming, `ask`/`exec` CLI, session resume\n\
    - Coding tools: safe file ops, ripgrep search, fuzzy discovery, code intelligence, project indexing, terminal execution\n\
    - Extensibility: Agent Skills, MCP client/server, lifecycle hooks, subagents, custom providers, Zed ACP, Claude Code\n\
    - Model providers: 21+ LLMs (Anthropic, OpenAI, Gemini, OpenRouter, Ollama, LM Studio, etc.)\n\
    - Safety: restricted shell sandbox, tool guardrails, subprocess isolation, full audit logging\n\
    - Protocols: Open Responses, Agent2Agent (A2A), ATIF, Anthropic Messages API\n\n\
    **Default model**: MiMo V2.6 Pro (Xiaomi), 1M-token context (`mimo-v2.6-pro`).\n\n\
    **Other top-level dirs**: `docs/`, `plans/`, `rules/`, `examples/`, `tests/`, `evals/`, `fuzz/`, `scripts/`, `crates/codegen/xtask/`, `homebrew/`.\n\n\
    Quick start:\n\
    ```shell\n\
    curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash\n\
    vtcode init\n\
    vtcode\n\
    ```\n\n\
    Let me know what you'd like to dig into next — a specific crate, the agent loop, the TUI, sandboxing, or something else."
}

mod adoption_detection;
mod autonomous_full_auto;
mod recovery_interim;
mod relaxed_continuation;
mod user_input_recap;
