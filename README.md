<h1 align="center">VT Code</h1>

<p align="center"><code>npm install -g @vinhnx/vtcode</code></p>
<p align="center"><code>npx vtcode</code></p>
<p align="center"><code>cargo install vtcode</code></p>
<p align="center"><code>brew install vtcode</code></p>
<p align="center">
  <a href="./docs/guides/mcp-integration.md">
    <img src="https://img.shields.io/badge/agent%20client%20protocol-black?style=for-the-badge&logo=zedindustries" alt="zed"/>
  </a> <a href="./docs/guides/zed-acp.md">
  <img src="https://img.shields.io/badge/model%20context%20protocol-black?style=for-the-badge&logo=modelcontextprotocol" alt="MCP"/>
</a>
</p>

<p align="center">
  <a href="https://open-vsx.org/extension/nguyenxuanvinh/vtcode-companion" target="_blank">
    <img src="https://img.shields.io/badge/Available%20on-Open%20VSX-4CAF50?style=for-the-badge&logo=opensearch&logoColor=white" alt="Open VSX Registry"/>
  </a>
  <a href="https://marketplace.visualstudio.com/items?itemName=nguyenxuanvinh.vtcode-companion" target="_blank">
    <img src="https://custom-icon-badges.demolab.com/badge/Visual%20Studio%20Code-0078d7.svg?style=for-the-badge&logo=vsc&logoColor=white&label=Install" alt="VS Code Extension"/>
  </a>
</p>

<p align="center">
<a href="https://crates.io/crates/vtcode">
  <img src="https://img.shields.io/badge/crates-io-%23000000.svg?e&logo=rust&logoColor=white" alt="crates.io"/>
</a>
  <a href="https://docs.rs/vtcode">
    <img src="https://img.shields.io/badge/docs-rs-%23000000.svg?e&logo=rust&logoColor=white" alt="docs.rs"/>
  </a>
  <a href="https://crates.io/crates/vtcode">
    <img src="https://img.shields.io/crates/v/vtcode?color=fc8d62&label=crates.io" alt="Crates.io Version"/>
  </a>
  <a href="https://github.com/vinhnx/vtcode/releases">
    <img src="https://img.shields.io/github/v/release/vinhnx/vtcode?color=orange&label=Release" alt="GitHub Release"/>
  </a>
  <a href="https://deepwiki.com/vinhnx/vtcode"><img src="https://deepwiki.com/badge.svg" alt="Ask DeepWiki"></a>
</p>

<p align="center"><strong>VT Code</strong> is a Rust-based terminal coding agent with semantic code intelligence via Tree-sitter and ast-grep. Supports multiple LLM providers with automatic failover and efficient context management.</p>

<h6 align="center">> Development blog: <a href="https://buymeacoffee.com/vinhnx/vt-code">Lessons from Building VT Code: An Open-Source CLI AI Coding Agent</a><</h6>

<p align="center">
  <img src="resources/vhs/demo.gif" alt="VT Code demo" width="80%" />
</p>

---

## Quickstart

### Installing and running VT Code

Install globally with your preferred package manager. If you use Cargo:

```shell
cargo install vtcode
```

Alternatively, if you use [Homebrew](https://brew.sh/):

```shell
brew install vtcode
```

Or install via npm from GitHub Packages:

```shell
npm install -g @vinhnx/vtcode
```

Or install via npm from npmjs.com (alternative package name):

```shell
npm install -g vtcode-bin
```

Or run directly with npx without installation:

```shell
npx @vinhnx/vtcode
```

Then simply run `vtcode` to get started:

```shell
vtcode
```

### Using VT Code with your preferred provider

Set your API key environment variable and run VT Code:

```bash
# Set your API key
export OPENAI_API_KEY="your_api_key_here"

# Launch VT Code
vtcode
```

### Available providers

VT Code supports multiple providers including OpenAI, Anthropic, xAI, DeepSeek, Gemini, Z.AI, Moonshot AI, OpenRouter, and Ollama (local).

Set your preferred API key environment variable:

```bash
export OPENAI_API_KEY="sk-..."           # OpenAI
export ANTHROPIC_API_KEY="sk-ant-..."    # Anthropic
export GEMINI_API_KEY="AIza..."          # Google Gemini
export XAI_API_KEY="xai-..."             # xAI
export DEEPSEEK_API_KEY="sk-..."         # DeepSeek
export ZAI_API_KEY="zai-..."             # Z.AI
export MOONSHOT_API_KEY="sk-..."         # Moonshot AI
export OPENROUTER_API_KEY="sk-or-..."    # OpenRouter
```

### Agent Client Protocol (ACP)

VT Code can integrate with code editors like Zed. To configure ACP, refer to the [ACP docs](./docs/guides/zed-acp.md).

### Configuration

VT Code supports a rich set of configuration options, with preferences stored in `vtcode.toml`. Key configuration features include:

-   **Lifecycle Hooks**: Execute shell commands in response to agent events - see [Lifecycle Hooks Guide](./docs/guides/lifecycle-hooks.md)
-   **Tool Policies**: Control which tools are allowed, prompted, or denied
-   **Security Settings**: Configure human-in-the-loop approval and workspace boundaries
-   **Performance Tuning**: Adjust context limits, timeouts, and caching behavior

For full configuration options, see [Configuration](./docs/config/CONFIGURATION_PRECEDENCE.md).

---

### Key Features

-   **Security First**: Multi-layered security model with execution policy, sandbox integration, and argument injection protection
-   **Multi-Provider AI**: OpenAI, Anthropic, xAI, DeepSeek, Gemini, Z.AI, Moonshot AI, OpenRouter, MiniMax, Ollama (local)
-   **Code Intelligence**: Tree-sitter parsers for Rust, Python, JavaScript/TypeScript, Go, Java, Swift
-   **Smart Tools**: Built-in code analysis, file operations, terminal commands, and refactoring
-   **Editor Integration**: Native support for Zed IDE via Agent Client Protocol (ACP)
-   **Lifecycle Hooks**: Execute custom shell commands in response to agent events for context enrichment, policy enforcement, and automation ([docs](./docs/guides/lifecycle-hooks.md))
-   **Semantic Search**: AST-based search capabilities with ast-grep integration
-   **Context Management**: Advanced token budget tracking and context curation
-   **TUI Interface**: Rich terminal user interface with real-time streaming

### Security & Safety

VT Code implements a **defense-in-depth security model** to protect against prompt injection and argument injection attacks:

-   **Execution Policy**: Command allowlist with per-command argument validation
-   **Workspace Isolation**: All operations confined to workspace boundaries
-   **Sandbox Integration**: Optional Anthropic sandbox runtime for network commands
-   **Human-in-the-Loop**: Configurable approval system for sensitive operations
-   **Audit Trail**: Comprehensive logging of all command executions

See [Security Model](./docs/SECURITY_MODEL.md) for details.

---

### Docs & Examples

-   [**Getting started**](./docs/user-guide/getting-started.md)
    -   [Interactive mode](./docs/user-guide/interactive-mode.md)
    -   [Command line interface](./docs/user-guide/commands.md)
    -   [Custom prompts](./docs/user-guide/custom-prompts.md)
    -   [Configuration](./docs/config/CONFIGURATION_PRECEDENCE.md)
-   [**Context Engineering**](./docs/context_engineering.md)
    -   [Token budget management](./docs/context_engineering_implementation.md#token-budget-tracking--attention-management)
    -   [Dynamic context curation](./docs/context_engineering_implementation.md#phase-2-dynamic-context-curation)
-   [**Code Intelligence**](./docs/user-guide/tree-sitter-integration.md)
    -   [AST-Grep tools](./docs/AST_GREP_TOOLS_ASSESSMENT_UPDATED.md)
    -   [Semantic search](./docs/development/command-failure-handling.md)
-   [**Agent Client Protocol (ACP)**](./docs/guides/zed-acp.md)
-   [**Zed Integration**](./docs/guides/zed-acp.md) - Agent Client Protocol Integration. VT Code is fully [capable ACP agent](https://agentclientprotocol.com/overview/agents), works with [ACP Clients](https://agentclientprotocol.com/overview/clients), for example [Zed](https://zed.dev/).
-   [**Lifecycle Hooks**](./docs/guides/lifecycle-hooks.md) - Execute shell commands in response to agent events, enabling context enrichment, policy enforcement, and automation
-   [**Custom Prompts**](./docs/user-guide/custom-prompts.md)
-   [**Exec Mode**](./docs/user-guide/exec-mode.md)
-   [**Development**](./docs/development/README.md)
    -   [Testing](./docs/development/testing.md)
    -   [CI/CD](./docs/development/ci-cd.md)
-   [**Architecture**](./docs/ARCHITECTURE.md)
-   [**Security**](./docs/SECURITY_MODEL.md)
    -   [Security Model](./docs/SECURITY_MODEL.md)
    -   [Security Audit](./docs/SECURITY_AUDIT.md)
    -   [Tool Policies](./docs/vtcode_tools_policy.md)

---

## Visual Studio Code Extension

VT Code is available as an VS Code extension.

  <a href="https://marketplace.visualstudio.com/items?itemName=nguyenxuanvinh.vtcode-companion" target="_blank">
    <img src="https://custom-icon-badges.demolab.com/badge/Visual%20Studio%20Code-0078d7.svg?style=for-the-badge&logo=vsc&logoColor=white&label=Install" alt="VS Code Extension"/>
  </a>

The original VTCode extension for Visual Studio Code with full semantic code understanding and AI assistance.

VT Code is also compatible with other VS Code-compatible editors:

  <a href="https://open-vsx.org/extension/nguyenxuanvinh/vtcode-companion" target="_blank">
    <img src="https://img.shields.io/badge/Available%20on-Open%20VSX-4CAF50?style=for-the-badge&logo=opensearch&logoColor=white" alt="Open VSX Registry"/>
  </a>

Compatible with Cursor, Windsurf, and other VS Code-compatible editors through the Open VSX registry.

For installation instructions and download links for other IDEs, visit our [IDE Downloads](./docs/ide/downloads.md) page. For troubleshooting, see the [IDE Integration Troubleshooting Guide](./docs/ide/troubleshooting.md).

---

## Support VT Code Development

I build VT Code in my free time as a passion [project](https://github.com/vinhnx/vtcode?tab=readme-ov-file#technical-motivation) to research and explore how coding agents work in practice. If you find VT Code useful, please consider supporting my work with a coffee via
[BuyMeACoffee](https://www.buymeacoffee.com/vinhnx):

[![BuyMeACoffee](https://raw.githubusercontent.com/pachadotdev/buymeacoffee-badges/main/bmc-black.svg)](https://www.buymeacoffee.com/vinhnx)

[![QR Code](resources/screenshots/qr_donate.png)](https://buymeacoffee.com/vinhnx)

Your support means the world to me, thank you!

---

## License

This repository is licensed under the [MIT License](LICENSE).
