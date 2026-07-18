<p align="center">
  <img src="./resources/logo/vt_code_adaptive.svg" alt="VT Code" style="border-radius: 12px" />
</p>

<p align="center">
  <a href="./docs/skills/SKILLS_GUIDE.md"><img src="https://img.shields.io/badge/Agent%20Skills-BFB38F?style=flat-square" alt="Skills" /></a>
  <a href="./docs/guides/zed-acp.md"><img src="https://img.shields.io/badge/ACP-Zed-383B73?style=flat-square&logo=zedindustries" alt="Zed ACP" /></a>
  <a href="./docs/guides/mcp-integration.md"><img src="https://img.shields.io/badge/MCP-A63333?style=flat-square&logo=modelcontextprotocol" alt="MCP" /></a>
  <a href="https://ratatui.rs/highlights/v030/"><img src="https://img.shields.io/badge/Built_With-Ratatui-000?logo=ratatui&logoColor=fff&labelColor=000&color=fff" alt="Built with Ratatui" /></a>
  <a href="https://deepwiki.com/vinhnx/VTCode"><img src="https://deepwiki.com/badge.svg" alt="Ask DeepWiki" /></a>
</p>

<p align="center">
  <img src="./resources/gif/vtcode.gif" alt="VT Code demo" width="40%" style="border-radius: 10px" />&nbsp;&nbsp;&nbsp;&nbsp;<img src="./resources/screenshots/vtcode-01237.png" alt="VT Code screenshot" width="46%" style="border-radius: 10px" />
  <br><em>Secure, open, universal.</em>
</p>

## What is VT Code?

VT Code is a Rust coding agent built for long-running autonomous workflows, with OS-native sandboxing, multi-provider LLM support, open protocols, and extensible Skills.

## Features

- **Agent runtime** - Interactive TUI, slash commands, streaming, `ask`/`exec` CLI, session resume
- **Coding tools** - Safe file ops, [ripgrep](https://github.com/BurntSushi/ripgrep) search + [ast-grep](https://ast-grep.github.io/) outline symbol maps, fuzzy discovery, code intelligence, project indexing, terminal execution
- **Extensibility** - [Agent Skills](https://agentskills.io), [Model Context Protocol](https://modelcontextprotocol.io/) MCP client/server, lifecycle hooks, subagents, custom providers, [Agent Client Protocol](https://agentclientprotocol.com) (ACP).
- **Model providers** - 21+ LLM providers: Anthropic, OpenAI, Gemini, OpenRouter, **local inference via Ollama, LM Studio, and llama.cpp** (managed with the `/local` command), and more
- **Safety** - Restricted shell sandbox, tool guardrails, subprocess isolation, full audit logging
- **Protocols** - Open Responses, Agent2Agent (A2A), ATIF, Anthropic Messages API
- **Loop engineering** - Worktree isolation for parallel agents, propose/verify sub-agent separation, durable loop state, cost guardrails
- **Planning workflow** - Iterate on a build plan with `/plan` and the `plan` primary agent, then hand off to `build`/`auto` via a structured review gate

## Quick start

Install via homebrew

```yaml
brew install vinhnx/tap/vtcode
```

One-liner for macOS/Linux (also installs ripgrep + ast-grep)

```yaml
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash
```

Scaffolds `vtcode.toml`, `.vtcode/`, and `AGENTS.md` in your project

```yaml
vtcode init
```

Launch VT Code

```yaml
vtcode
```

### Common commands

```yaml
vtcode                         # interactive TUI
vtcode init                    # scaffold project config + AGENTS.md
vtcode ask "explain Rc vs Arc" # one-shot question
vtcode exec "refactor main.rs" # headless task with full tool access
vtcode review                  # review uncommitted changes
vtcode update                  # self-update
```

## Documentation

- [**Interactive TUI**](./docs/user-guide/interactive-mode.md) - Primary agents, slash commands (`/model`, `/review`, `/mcp`, `/skills`, `/theme`, `/compact`)
- [**Full automation**](./docs/guides/full-automation.md) - `--full-auto` CLI, plan-build-evaluate harness, subagents, scheduled tasks
- [**Providers**](./docs/providers/PROVIDER_GUIDES.md) - Setup guides for all 21 providers
- [**Configuration**](./docs/config/CONFIG_FIELD_REFERENCE.md) - `vtcode.toml`, tool config, lifecycle hooks
- [**Agent Skills**](./docs/skills/SKILLS_GUIDE.md) - Creating, loading, and sharing skills
- [**MCP Integration**](./docs/guides/mcp-integration.md) - Client and server modes
- [**Editor guides**](./docs/guides/zed-acp.md) - Zed ACP, VS Code, Claude Code
- [**Safety**](./docs/security/SECURITY_MODEL.md) - Shell sandbox, security hardening, threat model
- [**Protocols**](./docs/protocols/OPEN_RESPONSES.md) - Open Responses, ATIF, A2A, Anthropic Messages API
- [**Loop engineering**](./docs/project/PLAN-loop-engineering.md) - Worktree isolation, propose/verify, loop state, cost guardrails
- [**Planning workflow**](./docs/guides/planning-workflow.md) - `/plan`, review gate, plan handoff to build/auto agents

## Providers

VT Code supports 21+ LLM providers out of the box, plus any OpenAI-compatible API via `[[custom_providers]]`.

| Category            | Providers                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| ------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Cloud LLMs**      | [Anthropic](./docs/providers/PROVIDER_GUIDES.md#anthropic-claude) · [OpenAI](./docs/providers/PROVIDER_GUIDES.md#openai-gpt) · [Gemini](./docs/providers/PROVIDER_GUIDES.md#google-gemini) · [Z.AI](./docs/providers/PROVIDER_GUIDES.md#zai-zai) · [Moonshot (Kimi)](./docs/providers/PROVIDER_GUIDES.md#moonshot-kimi) · [StepFun](./docs/providers/PROVIDER_GUIDES.md#stepfun) · [MiniMax](./docs/providers/PROVIDER_GUIDES.md#minimax) · [Mistral](./docs/providers/PROVIDER_GUIDES.md#mistral) · [Qwen](./docs/providers/PROVIDER_GUIDES.md#qwen) |
| **Gateways**        | [OpenRouter](./docs/providers/PROVIDER_GUIDES.md#openrouter-marketplace) · [Evolink](./docs/providers/PROVIDER_GUIDES.md#evolink-multi-model-gateway) · [HuggingFace](./docs/providers/PROVIDER_GUIDES.md#huggingface) · [Atlas Cloud](./docs/providers/PROVIDER_GUIDES.md#atlas-cloud)                                                                                                                                                                                                                                                               |
| **Local inference** | [Ollama](./docs/providers/PROVIDER_GUIDES.md#ollama-local--cloud-models) · [LM Studio](./docs/providers/PROVIDER_GUIDES.md#lm-studio-local-server) · [llama.cpp](./docs/providers/PROVIDER_GUIDES.md#llamacpp-local-server)                                                                                                                                                                                                                                                                                                                           |
| **Other**           | [GitHub Copilot](./docs/providers/PROVIDER_GUIDES.md#github-copilot) · [Anthropic API Compat](./docs/providers/PROVIDER_GUIDES.md#anthropic-api-compatibility-server) · [Poolside](./docs/providers/PROVIDER_GUIDES.md#poolside)                                                                                                                                                                                                                                                                                                                      |

Read: [Provider Guides](./docs/providers/PROVIDER_GUIDES.md).

## Local models (experimental)

Run models entirely on your machine for privacy, offline use, or zero token
cost. VT Code supports three local backends, all managed from the TUI:

- **Ollama** (`ollama serve`) — best-supported local backend; auto-loads pulled models.
- **LM Studio** (`lms server start`) — OpenAI-compatible; select the loaded model in the picker.
- **llama.cpp** (`llama-server -m model.gguf`) — most automated; auto-starts via `LLAMACPP_MODEL_PATH`.

```yaml
/local                 # interactive local server manager
/local start ollama   # start a specific backend
/local troubleshoot   # diagnose connection / model issues
```

Before each generation VT Code verifies the server is up and the model is
loaded, and on failure prints the exact recovery command (e.g.
`ollama pull gpt-oss:20b`) instead of a cryptic error. Local inference is
**experimental** and depends on your hardware — see
[Local Models guide](./docs/guides/local-models.md) for trade-offs, hardware
sizing, and a reliable-setup checklist, and
[Local Inference Servers](./docs/providers/local-servers.md) for the full
`/local` reference.

## Development

```yaml
git clone https://github.com/vinhnx/vtcode.git
cd vtcode
./scripts/run-debug.sh
```

Rust stable, edition 2024, MSRV 1.93.0. Workspace of ~30 crates:

| Layer          | Crates                                                                     |
| -------------- | -------------------------------------------------------------------------- |
| Binary         | `vtcode`                                                                   |
| Common         | `vtcode-commons`, `vtcode-exec-events`, `vtcode-macros`, `vtcode-utility-tool-specs` |
| Codegen        | `vtcode-core`, `vtcode-ui`, `vtcode-config`, `vtcode-llm`, `vtcode-skills`, `vtcode-safety`, `vtcode-a2a`, `vtcode-mcp`, `vtcode-auth`, `vtcode-acp`, `vtcode-indexer`, `vtcode-bash-runner`, `vtcode-memory`, `vtcode-eval` |

Want to use VT Code as a library? See [`vtcode-battery-pack`](https://github.com/vinhnx/vtcode-battery-pack) for a curated set of crates you can add to your own Rust projects.

```yaml
./scripts/check-dev.sh  # fast quality gate (clippy, fmt, check)
cargo nextest run        # parallel test runner
```

## Contributing

I'd love to have you, bug reports, docs, features, ideas, all welcome. Start with [issues](https://github.com/vinhnx/vtcode/issues) or [good first issues](https://github.com/vinhnx/vtcode/issues?q=is%3Aopen+is%3Aissue+label%3A%22good+first+issue%22). AI agents see [AGENTS.md](./AGENTS.md). Humans see [CONTRIBUTING.md](./docs/CONTRIBUTING.md).

<p align="center">
  <a href="https://github.com/kernitus"><img src="https://avatars.githubusercontent.com/u/2789734?s=60" width="40" height="40" alt="@kernitus" title="@kernitus (54 commits)" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/oiwn"><img src="https://avatars.githubusercontent.com/u/398035?s=60" width="40" height="40" alt="@oiwn" title="@oiwn (12 commits)" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/chenrui333"><img src="https://avatars.githubusercontent.com/u/1580956?s=60" width="40" height="40" alt="@chenrui333" title="@chenrui333 (6 commits)" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/Sachin-Bhat"><img src="https://avatars.githubusercontent.com/u/25080916?s=60" width="40" height="40" alt="@Sachin-Bhat" title="@Sachin-Bhat (6 commits)" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/gzsombor"><img src="https://avatars.githubusercontent.com/u/66230?s=60" width="40" height="40" alt="@gzsombor" title="@gzsombor (4 commits)" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/lucaszhu-hue"><img src="https://avatars.githubusercontent.com/u/278269343?s=60" width="40" height="40" alt="@lucaszhu-hue" title="@lucaszhu-hue (4 commits)" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/leonj1"><img src="https://avatars.githubusercontent.com/u/5171829?s=60" width="40" height="40" alt="@leonj1" title="@leonj1 (4 commits)" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/poelzi"><img src="https://avatars.githubusercontent.com/u/66107?s=60" width="40" height="40" alt="@poelzi" title="@poelzi (2 commits)" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/EvoLinkAI"><img src="https://avatars.githubusercontent.com/u/253253881?s=60" width="40" height="40" alt="@EvoLinkAI" title="@EvoLinkAI (2 commits)" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/gurdasnijor"><img src="https://avatars.githubusercontent.com/u/1755404?s=60" width="40" height="40" alt="@gurdasnijor" title="@gurdasnijor (2 commits)" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/morler"><img src="https://avatars.githubusercontent.com/u/478444?s=60" width="40" height="40" alt="@morler" title="@morler (2 commits)" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/uiYzzi"><img src="https://avatars.githubusercontent.com/u/40852301?s=60" width="40" height="40" alt="@uiYzzi" title="@uiYzzi (2 commits)" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/search?q=li%40maisiliym.criome.net&type=commits"><img src="https://avatars.githubusercontent.com/u/0?s=60" width="40" height="40" alt="@li" title="li (2 commits)" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/RobertBorg"><img src="https://avatars.githubusercontent.com/u/1288566?s=60" width="40" height="40" alt="@RobertBorg" title="@RobertBorg (1 commit)" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/ForrestThump"><img src="https://avatars.githubusercontent.com/u/44280834?s=60" width="40" height="40" alt="@ForrestThump" title="@ForrestThump (1 commit)" style="border-radius: 50%" /></a>
</p>

## Support

VT Code is a labor of love built in my spare time. If it's helped you ship something or learn something, a [sponsorship](https://github.com/sponsors/vinhnx) would mean the world.

<p align="center">
  <a href="https://github.com/dnhn"><img src="https://avatars.githubusercontent.com/u/2561973" width="80" height="80" alt="@dnhn" style="border-radius: 50%" /></a>
  <a href="https://github.com/codemod"><img src="https://avatars.githubusercontent.com/u/78830094" width="80" height="80" alt="@codemod" style="border-radius: 50%" /></a>
  <a href="https://github.com/coderabbitai"><img src="https://avatars.githubusercontent.com/u/132028505" width="80" height="80" alt="@coderabbitai" style="border-radius: 50%" /></a>
  <a href="https://github.com/KhaiRyth"><img src="https://avatars.githubusercontent.com/u/273723951" width="80" height="80" alt="@KhaiRyth" style="border-radius: 50%" /></a>
</p>

<p align="center">
  <a href="https://github.com/sponsors/vinhnx"><img src="https://img.shields.io/badge/%E2%9D%A4%20Sponsor-30363D?style=for-the-badge&logo=github-sponsors&logoColor=#EA4AAA" alt="GitHub Sponsors" height="33" /></a>&nbsp;&nbsp;&nbsp;
  <a href="https://buymeacoffee.com/vinhnx"><img src="./resources/screenshots/qr_donate.png" alt="Buy Me a Coffee" width="100" style="border-radius: 12px" /></a>
</p>

## License

First-party code is licensed under **MIT OR Apache-2.0** — choose whichever works best for you. See [LICENSE](LICENSE) for the full Apache-2.0 text; MIT terms are also granted under the same copyright.

Third-party and inspired-by code remains under its original licenses. See [THIRD-PARTY-NOTICES](THIRD-PARTY-NOTICES) for attributions.
