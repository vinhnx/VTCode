<p align="center">
  <img src="./resources/logo/vt_code_adaptive.svg" alt="VT Code" />
</p>

<p align="center">
  <strong>An open-source terminal coding agent with multi-provider LLM support, rich TUI workflows, agent skills, and defense-in-depth shell safety.</strong>
</p>

<p align="center">
  <a href="https://crates.io/crates/vtcode"><img src="https://img.shields.io/crates/v/vtcode?style=flat-square&color=171C26&label=crates.io" alt="Crates.io Version"/></a>&nbsp;
  <a href="https://github.com/vinhnx/vtcode/releases"><img src="https://img.shields.io/github/v/release/vinhnx/vtcode?style=flat-square&color=171C26&label=Release" alt="GitHub Release"/></a>&nbsp;
  <a href="./docs/skills/SKILLS_GUIDE.md"><img src="https://img.shields.io/badge/Agent%20Skills-BFB38F?style=flat-square" alt="Skills"/></a>&nbsp;
  <a href="./docs/guides/zed-acp.md"><img src="https://img.shields.io/badge/ACP-Zed-383B73?style=flat-square&logo=zedindustries" alt="Zed ACP"/></a>&nbsp;
  <a href="./docs/guides/mcp-integration.md"><img src="https://img.shields.io/badge/MCP-A63333?style=flat-square&logo=modelcontextprotocol" alt="MCP"/></a>&nbsp;
  <a href="https://deepwiki.com/vinhnx/VTCode"><img src="https://deepwiki.com/badge.svg" alt="Ask DeepWiki"/></a>
</p>

<p align="center">
  <img src="./resources/gif/vtcode.gif" alt="VT Code demo" />
</p>

## Why VT Code?

VT Code is built for developers who want a capable local-first coding agent without being locked into one model vendor. It combines a streaming terminal UI, safe shell execution, code-aware tools, OAuth support, and open agent protocols in one Rust workspace.

- **Bring your model**: GitHub Copilot, OpenAI, Anthropic, Gemini, DeepSeek, OpenRouter, Z.AI, Moonshot AI, MiniMax, HuggingFace Inference Providers, Ollama, LM Studio, and OpenAI-compatible custom providers.
- **Work in the terminal**: interactive TUI, `ask`/`exec` CLI flows, pipe-friendly stdout/stderr behavior, and rich PTY snapshots powered by Ghostty VT when available.
- **Run safely**: command policy, workspace boundaries, OS sandboxing on macOS/Linux, approval gates, and audit-friendly execution logs.
- **Extend the agent**: Agent Skills, MCP integration, lifecycle hooks, foreground subagents, optional background subprocess agents, and editor integrations through ACP.
- **Export and interoperate**: ATIF trajectory export, Open Responses conformance, A2A support, and Anthropic Messages API compatibility.

## Quick Start

### Install

macOS/Linux recommended installer:

```bash
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash
```

Windows PowerShell best-effort installer:

```powershell
irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex
```

Other options:

```bash
cargo install vtcode
brew install vtcode

# Development/bleeding edge tap
brew tap vinhnx/tap
brew install vinhnx/tap/vtcode
```

The macOS/Linux installer also attempts to install `ripgrep` and `ast-grep` for faster search and semantic code queries. To skip them:

```bash
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash -s -- --without-search-tools
```

Official macOS/Linux release archives bundle `ghostty-vt/` runtime libraries for richer terminal snapshots. Custom installs fall back to the built-in `legacy_vt100` backend when those assets are unavailable.

See the [Installation Guide](./docs/installation/README.md) and [Native Installer Guide](./docs/installation/NATIVE_INSTALLERS.md) for platform notes and troubleshooting.

### Configure a provider

Set an API key for your preferred provider, then launch VT Code:

```bash
export OPENAI_API_KEY="sk-..."
vtcode
```

Provider selection lives in `vtcode.toml`:

```toml
[agent]
provider = "openai"
default_model = "gpt-4.1"
```

For all provider-specific environment variables and examples, see [AI Provider Setup](./docs/providers/PROVIDER_GUIDES.md).

### Use the CLI

```bash
# Interactive TUI
vtcode

# Pipe generated output while logs stay on stderr
vtcode ask "write a Rust factorial function" > factorial.rs

# Run an agent task non-interactively
vtcode exec "summarize the current git diff"
```

VT Code follows [Command Line Interface Guidelines](https://clig.dev/): primary command output goes to stdout, while logs, metadata, reasoning traces, and prompts go to stderr.

## Feature Tour

### Agent Skills

VT Code supports the [open Agent Skills standard](http://agentskills.io/) so agents can discover and load reusable capabilities from local directories, remote sources, and embedded resources with deterministic precedence.

Read more in the [Agent Skills Guide](./docs/skills/SKILLS_GUIDE.md).

### Subagents and background helpers

VT Code can delegate bounded work to foreground subagents and can run an explicitly configured background subagent as a managed subprocess.

- `/agent` and `/agents active` inspect delegated agents.
- `/subprocesses` or `Alt+S` opens the Local Agents drawer.
- `Ctrl+B` starts or stops the configured default background subagent only after background agents are enabled.

Example configuration:

```toml
[subagents.background]
enabled = true
default_agent = "rust-engineer"
refresh_interval_ms = 2000
auto_restore = true
toggle_shortcut = "ctrl+b"
```

See [Subagents](./docs/user-guide/subagents.md) for usage details.

### Authentication

OAuth 2.0 flows are available for providers such as OpenAI ChatGPT, OpenRouter, and GitHub Copilot. Tokens are stored in OS-native credential stores when possible, with encrypted file fallback when needed.

```bash
vtcode
# Then run: /login copilot
```

See [OAuth Authentication](./docs/guides/oauth-authentication.md) for setup and provider-specific notes.

### Custom OpenAI-compatible providers

Any OpenAI-compatible API can be registered through `[[custom_providers]]`. For example, Atlas Cloud:

```toml
[agent]
provider = "atlascloud"
default_model = "deepseek-ai/deepseek-v4-flash"

[[custom_providers]]
name = "atlascloud"
display_name = "Atlas Cloud"
base_url = "https://api.atlascloud.ai/v1"
api_key_env = "ATLASCLOUD_API_KEY"
model = "deepseek-ai/deepseek-v4-flash"
```

See [Atlas Cloud Integration](./docs/providers/atlascloud.md) for the full setup flow.

### Editor and protocol integrations

VT Code integrates with editors and agent ecosystems through open protocols:

- [Agent Client Protocol / Zed](./docs/guides/zed-acp.md)
- [MCP integration](./docs/guides/mcp-integration.md)
- [Agent2Agent Protocol](./docs/a2a/a2a-protocol.md)
- [Open Responses](./docs/protocols/OPEN_RESPONSES.md)
- [ATIF Trajectory Format](./docs/protocols/ATIF_TRAJECTORY_FORMAT.md)
- Anthropic Messages API compatibility for tools that speak Anthropic-style `/v1/messages`

For local Zed development, prefer a wrapper script that runs `target/debug/vtcode` or `target/release/vtcode` with an explicit `--config` path so Zed does not pick up an older installed binary.

### VS Code and compatible editors

<a href="https://marketplace.visualstudio.com/items?itemName=nguyenxuanvinh.vtcode-companion" target="_blank">
  <img src="https://custom-icon-badges.demolab.com/badge/Visual%20Studio%20Code-0078d7.svg?style=for-the-badge&logo=vsc&logoColor=white&label=Install" alt="VS Code Extension"/>
</a>

<a href="https://open-vsx.org/extension/nguyenxuanvinh/vtcode-companion" target="_blank">
  <img src="https://img.shields.io/badge/Available%20on-Open%20VSX-4CAF50?style=for-the-badge&logo=opensearch&logoColor=white" alt="Open VSX Registry"/>
</a>

The companion extension is available on the Visual Studio Marketplace and Open VSX for Cursor, Windsurf, and other VS Code-compatible editors. See [IDE Downloads](./docs/ide/downloads.md) and [IDE Troubleshooting](./docs/ide/troubleshooting.md).

## Configuration

Most settings live in `vtcode.toml`. Common areas include provider selection, OAuth behavior, tool policies, workspace safety, lifecycle hooks, PTY backend selection, context budgets, and performance tuning.

- [Configuration precedence](./docs/config/CONFIGURATION_PRECEDENCE.md)
- [Lifecycle hooks](./docs/guides/lifecycle-hooks.md)
- [Tool policies](./docs/modules/vtcode_tools_policy.md)
- [Ghostty VT packaging](./docs/development/GHOSTTY_VT_PACKAGING.md)

## Security and Safety

VT Code uses a defense-in-depth model for prompt-injection and argument-injection resistance:

- command allowlists and per-command validation
- workspace isolation
- macOS Seatbelt and Linux Landlock/seccomp sandboxing
- configurable allow/deny/prompt policies for tools and MCP servers
- human approval gates for sensitive operations
- auditable command execution logs

Read the [Security Model](./docs/security/SECURITY_MODEL.md) and [Sandbox Deep Dive](./docs/sandbox/SANDBOX_DEEP_DIVE.md).

## Benchmarks

VT Code has a pending submission to [vercel/next-evals-oss](https://github.com/vercel/next-evals-oss/pull/83), the benchmark behind the [Next.js AI Agent Evaluations leaderboard](https://nextjs.org/evals).

| Agent       | Model                         | Status      | Success Rate | Passed | Avg Duration |
| ----------- | ----------------------------- | ----------- | ------------ | ------ | ------------ |
| **VT Code** | `moonshotai/Kimi-K2.6:novita` | **Pending** | **33%**      | 8/24   | 90.5s        |

```text
Next.js eval success rate
VT Code + Kimi K2.6  ████████░░░░░░░░░░░░░░░░  33%
```

See [benchmark notes](./docs/benchmarks/README.md#nextjs-ai-agent-evaluations) and the formal [eval framework](./evals/README.md).

## Documentation

- [Getting started](./docs/user-guide/getting-started.md)
- [Interactive mode](./docs/user-guide/interactive-mode.md)
- [Commands](./docs/user-guide/commands.md)
- [Exec mode](./docs/user-guide/exec-mode.md)
- [Keyboard shortcuts](./docs/guides/tui-event-handling.md)
- [Context engineering](./docs/context/context_engineering.md)
- [Code intelligence](./docs/user-guide/tree-sitter-integration.md)
- [Development guide](./docs/development/README.md)
- [Architecture](./docs/ARCHITECTURE.md)
- [FAQ](./docs/FAQ.md)

Ask documentation assistants:

- [Google Gemini CodeWiki](https://codewiki.google/github.com/vinhnx/vtcode)
- [Devin DeepWiki](https://deepwiki.com/vinhnx/vtcode)

## Development

```bash
git clone https://github.com/vinhnx/vtcode.git
cd vtcode
./scripts/run-debug.sh
```

Useful checks:

```bash
./scripts/check-dev.sh
./scripts/check-dev.sh --test
./scripts/check-dev.sh --workspace
```

The workspace uses Rust stable, edition 2024, and MSRV 1.88. See [Development Setup](./docs/development/DEVELOPMENT_SETUP.md), [Testing](./docs/development/testing.md), and [CI/CD](./docs/development/ci-cd.md).

## Contributing

Contributions are welcome: bug reports, documentation improvements, features, tests, and issue triage all help. Please read [CONTRIBUTING.md](./docs/CONTRIBUTING.md) and [AGENTS.md](./AGENTS.md) before opening larger changes.

Good starting points:

- [Open issues](https://github.com/vinhnx/vtcode/issues)
- [Good first issues](https://github.com/vinhnx/vtcode/issues?q=is%3Aopen+is%3Aissue+label%3A%22good+first+issue%22)

## Support

VT Code is built in spare time and will stay open source. If it saves you time, you can support development here: [buymeacoffee.com/vinhnx](https://buymeacoffee.com/vinhnx)

<p align="center">
  <img src="./resources/screenshots/qr_donate.png" alt="Buy Me a Coffee QR code" />
</p>

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=vinhnx/vtcode&type=timeline&legend=top-left)](https://www.star-history.com/#vinhnx/vtcode&type=timeline&legend=top-left)

## License

This repository is licensed under the [MIT License](LICENSE).
