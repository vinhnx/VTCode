# VT Code Documentation Map

This document serves as an index of all VT Code documentation. When users ask questions about VT Code itself (capabilities, features, configuration, etc.), this file provides the complete catalog of available documentation sources.

## Quick Reference

**Core Questions**: Can VT Code do X? | How does VT Code Y work? | What's VT Code's Z feature?

**Documentation Retrieval**: When users ask about VT Code capabilities, fetch relevant sections from the files listed below based on the topic area.

## Documentation Categories

### Getting Started & Overview

- **File**: `docs/user-guide/getting-started.md`
  - **Content**: Installation, quick start, configuration basics, first session setup
  - **Topics**: Prerequisites, API setup, basic usage, terminal interface, troubleshooting
  - **User Questions**: "How do I install VT Code?", "How do I get started?", "What can VT Code do?", "What are VT Code's main features?"

- **File**: `docs/ARCHITECTURE.md`
  - **Content**: Modular trait-based architecture, core components, design principles
  - **Topics**: Tool system, traits, mode-based execution, adding new tools, plugin architecture
  - **User Questions**: "How is VT Code designed?", "What tools are available?", "Can I extend VT Code?"

### Tools & Functionality

- **File**: `docs/tools/TOOL_SPECS.md`
  - **Content**: Complete tool specifications and capabilities
  - **Topics**: File operations, search tools, command execution, cache system
  - **User Questions**: "What tools does VT Code have?", "How do file operations work?", "Can VT Code search code?", "What search capabilities exist?", "How does the tool system work?"

- **File**: `docs/vtcode_indexer.md`
  - **Content**: Workspace indexing and context management
  - **Topics**: Project analysis, context generation, file discovery
  - **User Questions**: "How does VT Code understand my project?", "Can VT Code analyze code?", "How does workspace indexing work?", "What programming languages are supported?"

- **File**: `docs/CUSTOM_TOOLS.md`
  - **Content**: Guide for developing custom tools and extensions
  - **Topics**: Tool development patterns, trait implementation, integration patterns
  - **User Questions**: "Can I add custom tools?", "How do I create my own VT Code extensions?", "What APIs are available for tool development?"

### Security & Safety

- **File**: `docs/security/SECURITY_MODEL.md`
  - **Content**: Comprehensive security architecture and threat model
  - **Topics**: Execution policies, sandbox integration, credential handling, workspace isolation
  - **User Questions**: "Is VT Code safe to use?", "What security features does it have?", "How does sandboxing work?", "What permissions does VT Code need?"

- **File**: `docs/security/SECURITY_QUICK_REFERENCE.md`
  - **Content**: Quick security reference guide
  - **Topics**: Security best practices, policy configuration, approval workflows
  - **User Questions**: "How do I configure security?", "What commands are allowed?", "How do I set up approval workflows?"

### LLM Providers & Models

- **File**: `docs/PROVIDER_GUIDES.md`
  - **Content**: LLM provider integration guides
  - **Topics**: OpenAI, Anthropic, Gemini, DeepSeek, xAI, OpenRouter integration
  - **User Questions**: "What LLM providers does VT Code support?", "How do I configure different models?", "Which LLM provider should I choose?"

- **File**: `docs/models.json`
  - **Content**: Complete model specifications and metadata
  - **Topics**: Model capabilities, context limits, pricing, vendor-specific features
  - **User Questions**: "What models are available?", "Which model should I use?", "What are the model capabilities?", "How do model capabilities compare?"

- **File**: `docs/selection-guide/MODEL_SELECTION.md`
  - **Content**: Comprehensive model selection guide by use case
  - **Topics**: Code generation, analysis, reasoning, cost optimization, speed vs quality
  - **User Questions**: "Which model is best for code generation?", "What model for debugging?", "Which model offers best value?", "How do I choose between models?"

### Configuration & Customization

- **File**: `docs/config/CONFIGURATION_PRECEDENCE.md`
  - **Content**: Advanced configuration options and precedence rules
  - **Topics**: TOML configuration, policy settings, lifecycle hooks, onboarding
  - **User Questions**: "How do I configure VT Code?", "What configuration options exist?", "How do I customize VT Code?"

- **File**: `docs/config/TOOLS_CONFIG.md`
  - **Content**: Tool-specific configuration settings
  - **Topics**: Tool policies, execution modes, cache settings
  - **User Questions**: "How do I configure tools?", "What are the policy options?", "How do I customize tool behavior?"

- **File**: `docs/config/ONBOARDING_SETUP.md`
  - **Content**: Complete onboarding and first-run setup
  - **Topics**: API key setup, provider configuration, security setup, feature enabling
  - **User Questions**: "How do I set up VT Code for the first time?", "How do I configure API keys?", "What do I need to get started?"

### User Workflows & Commands

- **File**: `docs/user-guide/commands.md`
  - **Content**: Available commands and slash commands
  - **Topics**: CLI commands, interactive mode, slash commands, session management
  - **User Questions**: "What commands are available?", "How do I use slash commands?", "What interactive features exist?"

- **File**: `docs/user-guide/interactive-mode.md`
  - **Content**: Interactive session usage and features
  - **Topics**: Chat sessions, context management, workflow patterns
  - **User Questions**: "How do interactive sessions work?", "What workflow patterns exist?", "How do I use VT Code in interactive mode?"

- **File**: `docs/workflows/PRODUCTIVITY_PATTERNS.md`
  - **Content**: Productivity workflows and best practices
  - **Topics**: Development workflows, debugging patterns, code review workflows
  - **User Questions**: "How can I be more productive with VT Code?", "What are the best workflows?", "How do I use VT Code for code review?"

### Performance & Optimization

- **File**: `docs/research/prompt_caching.md`
  - **Content**: Prompt caching and optimization techniques
  - **Topics**: Performance optimization, context engineering, cache strategies
  - **User Questions**: "How does VT Code optimize performance?", "What advanced features exist?", "How can I speed up VT Code?"

- **File**: `docs/performance/OPTIMIZATION_GUIDE.md`
  - **Content**: Comprehensive performance optimization guide
  - **Topics**: Speed optimization, memory usage, context management, model selection for performance
  - **User Questions**: "How can I optimize VT Code performance?", "What affects VT Code speed?", "How do I reduce response times?"

### Advanced Features & Research

- **File**: `docs/improved_system_prompts.md`
  - **Content**: Advanced system prompt engineering
  - **Topics**: Prompt optimization, context injection, workflow automation
  - **User Questions**: "How do system prompts work?", "Can I customize behavior?", "How do I optimize prompts?"

- **File**: `docs/advanced/AGENT_COORDINATION.md`
  - **Content**: Multi-agent system coordination and orchestration
  - **Topics**: Agent types, coordination patterns, task delegation, workflow orchestration
  - **User Questions**: "How does VT Code coordinate multiple agents?", "What agent types exist?", "How do I use agent orchestration?"

- **File**: `docs/advanced/CONTEXT_ENGINEERING.md`
  - **Content**: Advanced context management and engineering
  - **Topics**: Context optimization, memory management, conversation flow, context injection
  - **User Questions**: "How does VT Code manage context?", "What is context engineering?", "How do I optimize conversation context?"

### Development & Testing

- **File**: `docs/development/README.md`
  - **Content**: Development setup and contribution guidelines
  - **Topics**: Build process, testing, code standards, contribution workflow
  - **User Questions**: "How do I contribute to VT Code?", "How do I build from source?"

- **File**: `docs/development/testing.md`
  - **Content**: Testing strategies and frameworks
  - **Topics**: Unit testing, integration testing, test coverage, CI/CD
  - **User Questions**: "How is VT Code tested?", "What testing approach is used?"

### Troubleshooting & Fixes

- **File**: `docs/fixes/mcp_broken_pipe_fix.md`
  - **Content**: MCP integration troubleshooting
  - **Topics**: MCP protocol, connection issues, debugging
  - **User Questions**: "How does MCP integration work?", "What MCP tools are available?"

- **File**: `docs/ide/troubleshooting.md`
  - **Content**: IDE integration troubleshooting
  - **Topics**: VS Code extension, integration issues, setup problems
  - **User Questions**: "How do I use VT Code with my IDE?", "What IDE integrations exist?"

### Editor Integrations

- **File**: `docs/guides/zed-acp.md`
  - **Content**: Zed Agent Client Protocol setup, including Agent Server Extension packaging
  - **Topics**: ACP bridge configuration, Zed-specific environment settings, extension manifest layout, release packaging, local testing
  - **User Questions**: "How do I run VT Code inside Zed?", "Can I ship VT Code as a Zed extension?", "What ACP settings does VT Code require?"
- **Directory**: `zed-extension/`
  - **Content**: Ready-to-publish Zed extension manifest and icon
  - **Topics**: Agent Server Extension packaging, release asset wiring, checksum management
  - **User Questions**: "Where is the Zed extension manifest?", "How do I update checksums for a new release?"

### Implementation & Updates

- **File**: `docs/IMPLEMENTATION_COMPLETE.md`
  - **Content**: Implementation status and completed features
  - **Topics**: Feature completion, development milestones, roadmap
  - **User Questions**: "What features are implemented?", "What's the development status?"

- **File**: `docs/SELF_UPDATE_IMPLEMENTATION.md`
  - **Content**: Self-update mechanisms and implementation
  - **Topics**: Automatic updates, version management, deployment
  - **User Questions**: "How do updates work?", "Can VT Code self-update?"

## Enhanced Trigger Questions

### Core Capabilities & Features
- "What can VT Code do?"
- "What are VT Code's main features?"
- "How does VT Code compare to other AI coding tools?"
- "What makes VT Code unique?"
- "Can VT Code handle multiple programming languages?"
- "Does VT Code support real-time collaboration?"

### Getting Started & Setup
- "How do I install VT Code?"
- "How do I get started with VT Code?"
- "How do I set up VT Code for the first time?"
- "What do I need to get started?"
- "How do I configure API keys?"
- "Which LLM provider should I choose?"
- "How do I configure VT Code for my workflow?"

### Tools & Functionality
- "What tools does VT Code have?"
- "How do file operations work?"
- "Can VT Code search code?"
- "What search capabilities exist?"
- "How does the tool system work?"
- "Can I add custom tools?"
- "How do I create my own VT Code extensions?"
- "What APIs are available for tool development?"
- "How does workspace indexing work?"
- "What programming languages are supported?"

### LLM Providers & Models
- "What LLM providers does VT Code support?"
- "How do I configure different models?"
- "Which LLM provider should I choose?"
- "What models are available?"
- "Which model is best for code generation?"
- "What model for debugging?"
- "Which model offers best value?"
- "How do model capabilities compare?"
- "How do I choose between models?"

### Security & Configuration
- "Is VT Code safe to use?"
- "What security features does it have?"
- "How does sandboxing work?"
- "What permissions does VT Code need?"
- "How do I configure security?"
- "What commands are allowed?"
- "How do I set up approval workflows?"
- "How do I customize VT Code?"
- "What configuration options exist?"
- "How do I configure tools?"
- "What are the policy options?"
- "How do I customize tool behavior?"

### Workflows & Commands
- "What commands are available?"
- "How do I use slash commands?"
- "What interactive features exist?"
- "How do interactive sessions work?"
- "What workflow patterns exist?"
- "How do I use VT Code in interactive mode?"
- "How can I be more productive with VT Code?"
- "What are the best workflows?"
- "How do I use VT Code for code review?"

### Performance & Optimization
- "How does VT Code optimize performance?"
- "What advanced features exist?"
- "How can I speed up VT Code?"
- "How can I optimize VT Code performance?"
- "What affects VT Code speed?"
- "How do I reduce response times?"

### Advanced Features
- "How do system prompts work?"
- "Can I customize behavior?"
- "How do I optimize prompts?"
- "How does VT Code coordinate multiple agents?"
- "What agent types exist?"
- "How do I use agent orchestration?"
- "How does VT Code manage context?"
- "What is context engineering?"
- "How do I optimize conversation context?"

### Development & Integration
- "How do I contribute to VT Code?"
- "How do I build from source?"
- "How is VT Code tested?"
- "What testing approach is used?"
- "How does MCP integration work?"
- "What MCP tools are available?"
- "How do I use VT Code with my IDE?"
- "What IDE integrations exist?"

### Updates & Maintenance
- "What features are implemented?"
- "What's the development status?"
- "How do updates work?"
- "Can VT Code self-update?"

## VT Code Feature Categories

### Core Capabilities

- **Multi-LLM Provider Support**: OpenAI, Anthropic, Google, DeepSeek, xAI, OpenRouter, Moonshot AI
- **Terminal Interface**: Modern TUI with mouse support and streaming output
- **Workspace Management**: Automatic project indexing and context generation
- **Tool System**: Modular, extensible tool architecture with 12+ built-in tools
- **Security**: Enterprise-grade safety with sandboxing and policy controls

### Advanced Features

- **Context Engineering**: Intelligent conversation context management
- **Tree-sitter Integration**: Code parsing for 6+ programming languages
- **MCP Protocol**: Model Context Protocol integration for enhanced capabilities
- **PTY Integration**: Full pseudo-terminal support for interactive programs
- **Agent Coordination**: Multi-agent workflow support (Orchestrator, Explorer, Coder)

### Configuration & Customization

- **TOML Configuration**: Comprehensive configuration system
- **Lifecycle Hooks**: Event-driven automation and context enrichment
- **Tool Policies**: Granular permission and execution controls
- **Session Management**: Persistent sessions with resume/continue capabilities
- **Theme System**: Customizable ANSI themes and color schemes

### Performance & Optimization

- **Prompt Caching**: Intelligent caching system for faster responses
- **Context Optimization**: Smart context management to reduce token usage
- **Memory Efficiency**: Optimized for resource-constrained environments
- **Streaming Support**: Real-time streaming for better user experience

## Enhanced Documentation Response Pattern

When users ask questions about VT Code itself:

1. **Pattern Recognition**: Identify if the question matches trigger patterns
2. **Documentation Fetch**: Use the documentation map to locate relevant sections
3. **Contextual Response**: Provide specific, current information from documentation
4. **Follow-up Suggestions**: Recommend additional resources for deeper exploration
5. **Practical Examples**: Include concrete examples when applicable

### Response Guidelines

- **Current Information**: Always reference the documentation map for up-to-date details
- **Specific Answers**: Address the user's exact question with relevant documentation
- **Progressive Disclosure**: Start with essential info, then suggest deeper resources
- **Practical Guidance**: Include actionable steps and configuration examples
- **Related Topics**: Suggest related documentation based on the user's question

## Additional Resources

### External Documentation

- **Repository**: https://github.com/vinhnx/vtcode
- **Crate**: https://crates.io/crates/vtcode
- **VS Code Extension**: Open VSX and VS Code Marketplace
- **API Keys**: Setup guides for each provider

### Community & Support

- **GitHub Issues**: Bug reports and feature requests
- **GitHub Discussions**: Community support and questions
- **Contributing**: See development documentation

### Tool Ecosystem

- **Custom Tools**: API documentation for tool development
- **MCP Integration**: Model Context Protocol implementation
- **IDE Extensions**: VS Code and other IDE integrations

---

**Note**: This enhanced documentation map is designed for VT Code's self-documentation system. When users ask questions about VT Code itself, the system should fetch this document and use it to provide accurate, up-to-date information about VT Code's capabilities and features. The expanded trigger questions and response patterns ensure comprehensive coverage of user questions and consistent, helpful responses.
