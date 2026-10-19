# MiniMax Integration Guide

VT Code supports MiniMax models through the Anthropic-compatible API. You can use it in two ways:

- Use provider `anthropic` with model `MiniMax-M3.1-Flash` (recommended) or `MiniMax-M3`
- Or use the new provider `minimax` (convenience alias that defaults to the correct base URL and model)

## Overview

MiniMax provides an Anthropic API-compatible endpoint that allows seamless integration with tools built for Anthropic's
API. VT Code automatically detects when you're using MiniMax models and routes requests to the appropriate endpoint.
When using the `minimax` provider alias, VT Code sends requests through the Anthropic Messages API (`/v1/messages`)
rather than OpenAI-style `/chat/completions`.

## Quick Start

### 1. Get Your MiniMax API Key

Sign up at [MiniMax](https://www.minimax.chat/) and obtain your API key.

### 2. Configure Environment Variable

Set your MiniMax API key as the Anthropic API key:

```bash
export ANTHROPIC_API_KEY=your_minimax_api_key_here
```

### 3. Configure vtcode.toml

Update your `vtcode.toml` to use the MiniMax model:

```toml
[agent]
provider = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"
default_model = "MiniMax-M3.1-Flash"
```

or using the `minimax` provider alias:

```toml
[agent]
provider = "minimax"
# default_model is optional; defaults to MiniMax-M3
default_model = "MiniMax-M3.1-Flash"
api_key_env = "ANTHROPIC_API_KEY"
```

### 4. Start Using MiniMax

Run VT Code and it will automatically use the MiniMax-M3.1-Flash model (the latest MiniMax model) through the
Anthropic-compatible API:

```bash
vtcode
```

## Configuration Options

### Basic Configuration

```toml
[agent]
provider = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"
default_model = "MiniMax-M3.1-Flash"
```

### Custom Base URL Override

If you need to override the base URL (for example, to use a proxy or different endpoint):

```bash
export ANTHROPIC_BASE_URL=https://your-custom-endpoint.com/anthropic/v1
```

Or set it directly in your configuration if your setup supports it.

## Supported Features

The MiniMax-M3.1-Flash model through the Anthropic-compatible API supports:

- Text generation
- Streaming responses
- Tool calling (function calling)
- System prompts
- Temperature control
- Max tokens configuration
- Reasoning content (thinking blocks)

## Limitations

The M3.1-Flash endpoint supports text, image, and video inputs. The following features remain unsupported or ignored:

- Document input (`type="document"`)
- Some Anthropic-specific parameters may be ignored (e.g., `top_k`, `stop_sequences`, `service_tier`)
- Thinking cannot be disabled; `thinking: {"type": "disabled"}` returns HTTP 400

## Example Usage

### Simple Query

```bash
vtcode ask "Explain machine learning in simple terms"
```

### With Tool Calling

The MiniMax-M3.1-Flash model supports function calling just like Claude models. VT Code's built-in tools (file operations,
terminal commands, etc.) work seamlessly.

### Streaming Mode

Streaming is fully supported and works automatically when enabled in your configuration.

## Temperature Range

Note that MiniMax accepts temperature values in the range [0, 2]. Values outside this range will return an error.
The recommended value is 1.0.

## API Endpoint

VT Code automatically routes MiniMax-M3.1-Flash requests to:

```text
https://api.minimax.io/anthropic/v1/messages
```

The base URL is `https://api.minimax.io/anthropic/v1`, and VT Code appends `/messages` automatically.

This is handled transparently - you don't need to configure anything special unless you want to override the base URL
via environment variable.

### Droid-style custom model example

You can mirror Droid's custom model config using VT Code's user config (`vtcode.toml` in the canonical platform config
directory). Add a `minimax` provider entry with base URL and API key:

```toml
[providers]
  [providers.minimax]
  api_key = "${ANTHROPIC_API_KEY}"
  base_url = "https://api.minimax.io/anthropic/v1"
  model = "MiniMax-M3.1-Flash"
  enabled = true
```

Notes:

- The `minimax` provider is a convenience wrapper over Anthropic compatibility.
- If you prefer, configure under `anthropic` instead with the same base URL and model.

## Troubleshooting

### Authentication Errors

If you see authentication errors, verify:

1. Your API key is correctly set in the environment variable
2. The API key is valid and active
3. You're using `ANTHROPIC_API_KEY` as the environment variable name

### Model Not Found

If you see "model not found" errors:

1. Ensure you're using the exact model name: `MiniMax-M3.1-Flash` (case-sensitive)
2. Check that your API key has access to the MiniMax-M3.1-Flash model
3. For the previous generation, use `MiniMax-M3`

### Temperature Errors

If you see temperature-related errors:

1. Ensure temperature is in the range (0.0, 1.0]
2. The default temperature in VT Code should work fine

## Comparison with Claude Models

| Feature          | Claude Models | MiniMax-M3.1-Flash |
| ---------------- | ------------- | ------------------ |
| Text Generation  | ✓             | ✓                  |
| Streaming        | ✓             | ✓                  |
| Tool Calling     | ✓             | ✓                  |
| System Prompts   | ✓             | ✓                  |
| Image/Video Input| ✓             | ✓                  |
| Document Input   | ✓             | ✗                  |
| Reasoning Effort | ✓             | ✓ (`low`–`max`)    |
| Prompt Caching   | ✓             | ?                  |

## Related Documentation

- [Provider Guides](../providers/PROVIDER_GUIDES.md)
- [Configuration Guide](../config/config.md)

## Additional Resources

- [MiniMax Official Documentation](https://www.minimax.chat/docs)
- [MiniMax Anthropic API Compatibility Guide](https://www.minimax.chat/docs/guides/anthropic-api)
- [MiniMax M3.1 Flash announcement](https://www.minimax.io/blog/minimax-m31f)
