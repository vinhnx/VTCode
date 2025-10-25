<p align="center"><code>cargo install vtcode</code><br />or <code>brew install vinhnx/tap/vtcode</code></p>

<p align="center"><strong>VT Code</strong> is a Rust-based terminal coding agent with semantic code intelligence via Tree-sitter and ast-grep.
</br>
</br>Supports multiple LLM providers with automatic failover and efficient context management.</p>

<p align="center">
  <img src="resources/vhs/demo.gif" alt="VT Code demo" width="80%" />
</p>

---

[![humaneval pass@1](docs/benchmarks/reports/benchmark_badge.svg)](docs/benchmarks) [![security](https://img.shields.io/badge/security-hardened-green?style=flat-square)](docs/SECURITY_MODEL.md)

[![MCP](https://img.shields.io/badge/model%20context%20protocol-black?style=for-the-badge&logo=modelcontextprotocol)](https://github.com/vinhnx/vtcode/blob/main/docs/guides/mcp-integration.md) [![zed](https://img.shields.io/badge/agent%20client%20protocol-black?style=for-the-badge&logo=zedindustries)](https://agentclientprotocol.com/overview/agents)

[![crates.io](https://img.shields.io/crates/v/vtcode.svg?style=flat-square&label=crates.io&logo=rust)](https://crates.io/crates/vtcode) [![docs.rs](https://img.shields.io/docsrs/vtcode.svg?style=flat-square&label=docs.rs&logo=docsdotrs)](https://docs.rs/vtcode) [![npm](https://img.shields.io/npm/v/vtcode.svg?style=flat-square&label=npm&logo=npm)](https://www.npmjs.com/package/vtcode)

## Quickstart

### Installing and running VT Code

Install globally with your preferred package manager. If you use Cargo:

```shell
cargo install vtcode
```

Alternatively, if you use Homebrew:

```shell
brew install vinhnx/tap/vtcode
```

Or if you prefer NPM:

```shell
npm install -g vtcode
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

VT Code supports a rich set of configuration options, with preferences stored in `vtcode.toml`. For full configuration options, see [Configuration](./docs/config/CONFIGURATION_PRECEDENCE.md).

---

### Key Features

-   **Security First**: Multi-layered security model with execution policy, sandbox integration, and argument injection protection
-   **Multi-Provider AI**: OpenAI, Anthropic, xAI, DeepSeek, Gemini, Z.AI, Moonshot AI, OpenRouter, Ollama (local)
-   **Code Intelligence**: Tree-sitter parsers for Rust, Python, JavaScript/TypeScript, Go, Java, Swift
-   **Smart Tools**: Built-in code analysis, file operations, terminal commands, and refactoring
-   **Editor Integration**: Native support for Zed IDE via Agent Client Protocol (ACP)
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
-   [**Agent Client Protocol (ACP)**](./docs/guides/mcp-integration.md)
-   [**Zed Integration**](./docs/guides/zed-acp.md)
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

## Support VT Code Development

I build VT Code in my free time as a passion [project](https://github.com/vinhnx/vtcode?tab=readme-ov-file#technical-motivation) to research and explore how coding agents work in practice. If you find VT Code useful, please consider supporting my work with a coffee via
[BuyMeACoffee](https://www.buymeacoffee.com/vinhnx):

[![BuyMeACoffee](https://raw.githubusercontent.com/pachadotdev/buymeacoffee-badges/main/bmc-black.svg)](https://www.buymeacoffee.com/vinhnx)

[![QR Code](resources/screenshots/qr_donate.png)](https://buymeacoffee.com/vinhnx)

Your support means the world to me, thank you!

---

## License

This repository is licensed under the [MIT License](LICENSE).
