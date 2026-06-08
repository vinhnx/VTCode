<p align="center">
  <img src="./resources/logo/vt_code_adaptive.svg" alt="VT Code"/>
</p>

<p align="center">
  <a href="./docs/skills/SKILLS_GUIDE.md"><img src="https://img.shields.io/badge/Agent%20Skills-BFB38F?style=flat-square" alt="Skills" /></a>
  <a href="./docs/guides/zed-acp.md"><img src="https://img.shields.io/badge/ACP-Zed-383B73?style=flat-square&logo=zedindustries" alt="Zed ACP" /></a>
  <a href="./docs/guides/mcp-integration.md"><img src="https://img.shields.io/badge/MCP-A63333?style=flat-square&logo=modelcontextprotocol" alt="MCP" /></a>
  <a href="https://github.com/vinhnx/vtcode/releases"><img src="https://img.shields.io/github/v/release/vinhnx/vtcode?style=flat-square&color=171C26&label=Release" alt="GitHub Release" /></a>
  <a href="https://github.com/vinhnx/vtcode/graphs/contributors"><img src="https://img.shields.io/github/contributors/vinhnx/vtcode?style=flat-square&logo=github&label=Contributors&color=171C26" alt="Contributors" /></a>
  <a href="https://github.com/sponsors/vinhnx"><img src="https://img.shields.io/github/sponsors/vinhnx?style=flat-square&logo=github&label=Sponsors&color=EA4AAA" alt="Sponsors" /></a>
  <a href="https://deepwiki.com/vinhnx/VTCode"><img src="https://deepwiki.com/badge.svg" alt="Ask DeepWiki" /></a>
</p>

<p align="center">
  <img src="./resources/gif/vtcode.gif" alt="VT Code demo" width="40%" />&nbsp;&nbsp;&nbsp;&nbsp;<img src="./resources/screenshots/vtcode-01237.png" alt="VT Code screenshot" width="46%" />
  <br><em>Secure, open, universal.</em>
</p>

## What is VT Code?

VT Code is a local-first coding agent built in Rust, the only one with defense-in-depth security gating, broad LLM provider support, open protocols (Open Responses, A2A, MCP, ATIF), an extensible skill framework, delegated subagents, and rich tooling for long-running autonomous workflows.

## Features

<table style="border: none; border-collapse: collapse;">
  <tr><td style="border: none;"><strong>Area</strong></td><td style="border: none;"><strong>What VT Code provides</strong></td></tr>
  <tr><td style="border: none;">Agent runtime</td><td style="border: none;">Interactive TUI with slash commands, streaming, session management; <code>ask</code>/<code>exec</code> CLI modes; session resume</td></tr>
  <tr><td style="border: none;">Model providers</td><td style="border: none;">Anthropic, OpenAI, Gemini, DeepSeek, GitHub Copilot, OpenRouter, Ollama, LM Studio. 21+ providers supported</td></tr>
  <tr><td style="border: none;">Coding tools</td><td style="border: none;">Safe file operations, ripgrep search, fuzzy file discovery, code intelligence, project indexing, terminal execution</td></tr>
  <tr><td style="border: none;">Extensibility</td><td style="border: none;">Agent Skills, MCP client/server, lifecycle hooks, subagents, custom providers, Zed ACP, VS Code, Claude Code</td></tr>
  <tr><td style="border: none;">Safety</td><td style="border: none;">Restricted shell sandbox, tool guardrails, subprocess isolation, full audit logging</td></tr>
  <tr><td style="border: none;">Protocols</td><td style="border: none;">Open Responses, Agent2Agent (A2A), ATIF trajectory export, Anthropic Messages API compatibility</td></tr>
</table>

## Quick start

### Installing and running VT Code

```shell
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash   # macOS / Linux
```

```shell
cargo install vtcode   # via cargo
```

```shell
brew install vtcode    # via Homebrew
```

Then run `vtcode` to get started.

### Common commands

```shell
vtcode                        # launch interactive TUI
vtcode ask "explain Rc vs Arc"  # one-shot question, no tools
vtcode exec "refactor main.rs" # headless task with full tool access
vtcode review                  # review uncommitted changes
vtcode --resume                # pick up the last session
```

## Documentation

<table style="border: none; border-collapse: collapse;">
  <tr><td style="border: none;"><strong>Topic</strong></td><td style="border: none;"><strong>Covers</strong></td></tr>
  <tr><td style="border: none;"><a href="./README.md#quick-start">Interactive TUI</a></td><td style="border: none;">Session modes, slash commands (<code>/model</code>, <code>/review</code>, <code>/mcp</code>, <code>/skills</code>, <code>/theme</code>, <code>/compact</code>, <code>/schedule</code>)</td></tr>
  <tr><td style="border: none;"><a href="./docs/guides/full_auto_mode.md">Autonomous agent</a></td><td style="border: none;">Full-auto CLI, plan-build-evaluate harness, subagents, scheduled tasks</td></tr>
  <tr><td style="border: none;"><a href="./docs/providers/PROVIDER_GUIDES.md">Providers</a></td><td style="border: none;">Setup guides for all 21 providers</td></tr>
  <tr><td style="border: none;"><a href="./docs/config/CONFIG_FIELD_REFERENCE.md">Configuration</a></td><td style="border: none;"><code>vtcode.toml</code>, tool config, lifecycle hooks</td></tr>
  <tr><td style="border: none;"><a href="./docs/skills/SKILLS_GUIDE.md">Agent Skills</a></td><td style="border: none;">Creating, loading, and sharing skills</td></tr>
  <tr><td style="border: none;"><a href="./docs/guides/mcp-integration.md">MCP Integration</a></td><td style="border: none;">Client and server modes</td></tr>
  <tr><td style="border: none;"><a href="./docs/guides/zed-acp.md">Editor guides</a></td><td style="border: none;">Zed ACP, VS Code, Claude Code</td></tr>
  <tr><td style="border: none;"><a href="./docs/safety/SAFETY_ARCHITECTURE.md">Safety</a></td><td style="border: none;">Shell sandbox, security hardening, threat model</td></tr>
  <tr><td style="border: none;"><a href="./docs/protocols/OPEN_RESPONSES.md">Protocols</a></td><td style="border: none;">Open Responses, ATIF, A2A, Anthropic Messages API</td></tr>
</table>

## Providers

VT Code supports 21 LLM providers out of the box, plus any OpenAI-compatible API via `[[custom_providers]]`.

### Xiaomi MiMo V2.5 Series

<p align="center">
  <a href="https://platform.xiaomimimo.com"><img src="./resources/screenshots/xiaomi_mi_promo.png" alt="Xiaomi MiMo V2.5" width="400" /></a>
</p>

<p align="center"><em>Proud partner of the <a href="https://platform.xiaomimimo.com">Xiaomi MiMo Orbit Program</a></em></p>

Xiaomi MiMo V2.5 Pro ships as the default model in VT Code, available both natively and through OpenRouter. It brings a 1M-token context window, deep reasoning, and strong agentic performance to every session.

<p align="center">
  <a href="https://openrouter.ai/xiaomi/mimo-v2.5-pro">
    <img src="./resources/screenshots/xiaomi-mimo.png" alt="Xiaomi MiMo V2.5 Pro on OpenRouter" width="300" />
  </a>
</p>

<table style="border: none; border-collapse: collapse;">
  <tr><td style="border: none;"><strong>Provider</strong></td><td style="border: none;"><strong>Model ID</strong></td><td style="border: none;"><strong>Context</strong></td></tr>
  <tr><td style="border: none;"><a href="https://openrouter.ai/xiaomi/mimo-v2.5-pro">OpenRouter</a></td><td style="border: none;"><code>xiaomi/mimo-v2.5-pro</code></td><td style="border: none;">1M tokens</td></tr>
  <tr><td style="border: none;">OpenRouter</td><td style="border: none;"><code>xiaomi/mimo-v2.5</code></td><td style="border: none;">1M tokens</td></tr>
  <tr><td style="border: none;"><a href="https://platform.xiaomimimo.com/docs/en-US/welcome">Xiaomi MiMo</a></td><td style="border: none;"><code>mimo-v2.5-pro</code></td><td style="border: none;">1M tokens</td></tr>
  <tr><td style="border: none;">Xiaomi MiMo</td><td style="border: none;"><code>mimo-v2.5</code></td><td style="border: none;">1M tokens</td></tr>
</table>

Pricing: [Pay-as-you-go](https://platform.xiaomimimo.com/docs/en-US/price/pay-as-you-go) · [Subscription](https://platform.xiaomimimo.com/docs/en-US/price/tokenplan/subscription) · [Quick Access](https://platform.xiaomimimo.com/docs/en-US/price/tokenplan/quick-access) · [Docs](https://platform.xiaomimimo.com/docs/en-US/welcome) · [OpenRouter](https://openrouter.ai/xiaomi/mimo-v2.5-pro)

### Custom providers

```toml
[[custom_providers]]
name = "atlascloud"
display_name = "Atlas Cloud"
base_url = "https://api.atlascloud.ai/v1"
api_key_env = "ATLASCLOUD_API_KEY"
model = "deepseek-ai/DeepSeek-V3-0324"
```

Read: [Provider Guides](./docs/providers/PROVIDER_GUIDES.md).

## Development

```shell
git clone https://github.com/vinhnx/vtcode.git
cd vtcode
./scripts/run-debug.sh
```

Rust stable, edition 2024, MSRV 1.88.

```shell
./scripts/check-dev.sh  # fast quality gate (clippy, fmt, check)
cargo nextest run        # parallel test runner
```

## Contributing

I'd love to have you, bug reports, docs, features, ideas, all welcome. Start with [issues](https://github.com/vinhnx/vtcode/issues) or [good first issues](https://github.com/vinhnx/vtcode/issues?q=is%3Aopen+is%3Aissue+label%3A%22good+first+issue%22). AI agents see [AGENTS.md](./AGENTS.md). Humans see [CONTRIBUTING.md](./docs/CONTRIBUTING.md).

<p align="center">
  <a href="https://github.com/oiwn"><img src="https://avatars.githubusercontent.com/u/398035?s=60" width="40" height="40" alt="@oiwn" title="@oiwn" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/chenrui333"><img src="https://avatars.githubusercontent.com/u/1580956?s=60" width="40" height="40" alt="@chenrui333" title="@chenrui333" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/Sachin-Bhat"><img src="https://avatars.githubusercontent.com/u/25080916?s=60" width="40" height="40" alt="@Sachin-Bhat" title="@Sachin-Bhat" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/leonj1"><img src="https://avatars.githubusercontent.com/u/5171829?s=60" width="40" height="40" alt="@leonj1" title="@leonj1" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/gzsombor"><img src="https://avatars.githubusercontent.com/u/66230?s=60" width="40" height="40" alt="@gzsombor" title="@gzsombor" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/lucaszhu-hue"><img src="https://avatars.githubusercontent.com/u/278269343?s=60" width="40" height="40" alt="@lucaszhu-hue" title="@lucaszhu-hue" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/poelzi"><img src="https://avatars.githubusercontent.com/u/66107?s=60" width="40" height="40" alt="@poelzi" title="@poelzi" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/EvoLinkAI"><img src="https://avatars.githubusercontent.com/u/253253881?s=60" width="40" height="40" alt="@EvoLinkAI" title="@EvoLinkAI" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/gurdasnijor"><img src="https://avatars.githubusercontent.com/u/1755404?s=60" width="40" height="40" alt="@gurdasnijor" title="@gurdasnijor" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/kernitus"><img src="https://avatars.githubusercontent.com/u/2789734?s=60" width="40" height="40" alt="@kernitus" title="@kernitus" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/morler"><img src="https://avatars.githubusercontent.com/u/478444?s=60" width="40" height="40" alt="@morler" title="@morler" style="border-radius: 50%" /></a>&nbsp;
  <a href="https://github.com/uiYzzi"><img src="https://avatars.githubusercontent.com/u/40852301?s=60" width="40" height="40" alt="@uiYzzi" title="@uiYzzi" style="border-radius: 50%" /></a>
  <br><a href="https://github.com/vinhnx/vtcode/graphs/contributors"><img src="https://img.shields.io/github/contributors/vinhnx/vtcode?style=flat-square&logo=github&label=Total%20contributors&color=171C26" alt="Total contributors" /></a>
</p>

## Support

VT Code is a labor of love built in my spare time. If it's helped you ship something or learn something, a [sponsorship](https://github.com/sponsors/vinhnx) would mean the world.

<p align="center">
  <a href="https://github.com/dnhn"><img src="https://avatars.githubusercontent.com/u/2561973" width="60" height="60" alt="@dnhn" style="border-radius: 50%" /></a>
  <a href="https://github.com/codemod"><img src="https://avatars.githubusercontent.com/u/78830094" width="60" height="60" alt="@codemod" style="border-radius: 50%" /></a>
  <a href="https://github.com/coderabbitai"><img src="https://avatars.githubusercontent.com/u/132028505" width="60" height="60" alt="@coderabbitai" style="border-radius: 50%" /></a>
  <a href="https://github.com/KhaiRyth"><img src="https://avatars.githubusercontent.com/u/273723951" width="60" height="60" alt="@KhaiRyth" style="border-radius: 50%" /></a>
</p>

## License

[MIT License](LICENSE).
