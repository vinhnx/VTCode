# MiniMax Quick Start Guide

Get started with MiniMax-M2 in VTCode in under 2 minutes.

## Step 1: Get Your API Key

Sign up at [MiniMax](https://www.minimax.chat/) and get your API key.

## Step 2: Set Environment Variable

```bash
export ANTHROPIC_API_KEY=your_minimax_api_key_here
```

## Step 3: Configure VTCode

Create or edit `vtcode.toml`:

```toml
[agent]
provider = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"
default_model = "MiniMax-M2"
```

## Step 4: Run VTCode

```bash
vtcode
```

That's it! You're now using MiniMax-M2.

## Quick Test

Try this command:

```bash
vtcode ask "Explain what Rust ownership is in simple terms"
```

## What's Supported?

✅ Text generation  
✅ Streaming responses  
✅ Tool calling  
✅ Reasoning (thinking blocks)  
✅ System prompts  
✅ Temperature control  

❌ Image input  
❌ Document input  

## Need Help?

- Full guide: [MiniMax Integration Guide](./guides/minimax-integration.md)
- Examples: [MiniMax Usage Examples](./examples/minimax-usage.md)
- Config: [Example Configuration](./examples/minimax-config.toml)

## Switching Models

You can easily switch between models:

```bash
# Use MiniMax
vtcode --model MiniMax-M2 ask "your question"

# Use Claude
vtcode --model claude-sonnet-4-5 ask "your question"

# Use GPT-5
vtcode --model gpt-5 ask "your question"
```

## Common Issues

**Authentication Error?**
- Check your API key: `echo $ANTHROPIC_API_KEY`
- Make sure it's a valid MiniMax API key

**Model Not Found?**
- Use exact name: `MiniMax-M2` (case-sensitive)

**Temperature Error?**
- MiniMax requires 0.0 < temperature <= 1.0
- Default should work fine

## Next Steps

1. Read the [full integration guide](./guides/minimax-integration.md)
2. Try the [usage examples](./examples/minimax-usage.md)
3. Explore [configuration options](./config/CONFIGURATION_PRECEDENCE.md)

Happy coding with MiniMax-M2! 🚀
