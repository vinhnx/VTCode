# VTCode Zed Extension - Quick Start

Get up and running with the VTCode extension for Zed in 5 minutes.

## 1. Install Prerequisites

### VTCode CLI

Choose your preferred installation method:

```bash
# Option A: With Cargo (recommended)
cargo install vtcode

# Option B: With Homebrew (macOS)
brew install vtcode

# Option C: With NPM
npm install -g vtcode-ai
```

Verify installation:
```bash
vtcode --version
```

### Rust (for development only)

If you plan to build from source:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## 2. Install the Extension

### Option A: From Zed Extension Registry (Recommended)

1. Open Zed
2. Open Extensions panel (Cmd/Ctrl + Shift + X)
3. Search for "vtcode"
4. Click "Install"

### Option B: Install as Dev Extension

```bash
# Clone this repository
git clone https://github.com/vinhnx/vtcode.git
cd vtcode/zed-extension

# In Zed:
# 1. Open Extensions panel (Cmd/Ctrl + Shift + X)
# 2. Click "Install Dev Extension"
# 3. Select the zed-extension directory
```

## 3. Configure VTCode

Create a `vtcode.toml` in your workspace root:

```toml
[ai]
provider = "openai"
model = "gpt5-nano"

[workspace]
analyze_on_startup = false
max_context_tokens = 8000

[security]
human_in_the_loop = true
```

### Configure Your AI Provider

Get API credentials from your chosen provider:

- **Anthropic**: [console.anthropic.com](https://console.anthropic.com)
- **OpenAI**: [platform.openai.com](https://platform.openai.com)
- **Google**: [aistudio.google.com](https://aistudio.google.com)

Set your API key (the CLI will prompt for it, or set the environment variable):

```bash
export ANTHROPIC_API_KEY="your-api-key"
# or
export OPENAI_API_KEY="your-api-key"
```

## 4. First Use

### Test Installation

1. Open a workspace with `vtcode.toml`
2. Open the command palette (Cmd/Ctrl + Shift + P)
3. Search for "vtcode" to see available commands

### Basic Workflow

**Ask a Question:**
1. Open command palette
2. Type "VTCode: Ask the Agent"
3. Enter your question
4. Response appears in the output

**Analyze Your Code:**
1. Highlight code in editor
2. Right-click → "Ask with VTCode"
3. Follow prompts in output

**Edit Configuration:**
1. Open command palette
2. Type "VTCode: Open Configuration"
3. Edit `vtcode.toml` with syntax highlighting
4. Save and your changes apply immediately

## 5. Common Configuration Patterns

### Minimal Setup (OpenAI)

```toml
[ai]
provider = "openai"
model = "gpt-4o"
```

### Full-Featured Setup

```toml
[ai]
provider = "anthropic"
model = "claude-3-5-sonnet-20241022"

[workspace]
analyze_on_startup = false
max_context_tokens = 8000
ignore_patterns = ["node_modules", ".git", "dist"]

[security]
human_in_the_loop = true
allowed_tools = ["read_file", "edit_file", "analyze"]

[llm]
temperature = 0.7
top_p = 0.9
```

### Development Mode (Local Testing)

```toml
[ai]
provider = "anthropic"
model = "claude-3-5-sonnet-20241022"

[workspace]
analyze_on_startup = false
log_level = "debug"
```

## Troubleshooting

### Extension Not Found

- **Issue**: "vtcode" command not found
- **Solution**: Verify installation with `which vtcode` or `vtcode --version`

### Commands Don't Appear

- **Issue**: VTCode commands missing from command palette
- **Solution**: Reload Zed (Cmd/Ctrl + R) or close and reopen

### API Key Errors

- **Issue**: "Invalid API key" or authentication errors
- **Solution**:
  - Check your `vtcode.toml` provider configuration
  - Verify API key is set: `echo $ANTHROPIC_API_KEY` (for Anthropic)
  - Get a new API key from your provider's console

### Configuration Not Loading

- **Issue**: Settings not being applied
- **Solution**:
  - Ensure `vtcode.toml` is in workspace root
  - Check file syntax (should be valid TOML)
  - Reload workspace (close/reopen folder in Zed)

## Next Steps

1. **Read Configuration Guide**: [Full configuration options](extension-features.md)
2. **Check VTCode Documentation**: [Main VTCode repo](https://github.com/vinhnx/vtcode)
3. **Join Community**: Star the repo and share feedback
4. **Contribute**: [Development guide](DEVELOPMENT.md)

## Support

- **GitHub Issues**: [VTCode Issues](https://github.com/vinhnx/vtcode/issues)
- **Documentation**: [VTCode Docs](https://github.com/vinhnx/vtcode#documentation)
- **Discord**: [Join our community](https://discord.com/invite/...)

## Tips & Tricks

### Pro Tips

1. **Keyboard Shortcuts**: Bind VTCode commands to keybindings in Zed
2. **Context**: The more context you provide, the better responses
3. **Iterations**: Use "Ask the Agent" for follow-up questions in a conversation
4. **Analysis**: Use "Analyze Workspace" for large refactoring tasks
5. **Config as Code**: Version control your `vtcode.toml` in git

### Performance Tips

- Keep `max_context_tokens` reasonable (4000-8000)
- Exclude large directories in `ignore_patterns`
- Set `analyze_on_startup = false` for faster startup
- Use appropriate AI models (smaller for speed, larger for quality)

---

**Ready to start?** Open Zed, install the extension, create a `vtcode.toml`, and ask your first question!
