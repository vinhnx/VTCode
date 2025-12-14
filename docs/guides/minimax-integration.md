# MiniMax Integration Guide

VTCode supports MiniMax models through the Anthropic-compatible API. You can use it in two ways:

- Use provider `anthropic` with model `MiniMax-M2` (works today)
- Or use the new provider `minimax` (convenience alias that defaults to the correct base URL and model)

## Overview

MiniMax provides an Anthropic API-compatible endpoint that allows seamless integration with tools built for Anthropic's API. VTCode automatically detects when you're using the MiniMax-M2 model and routes requests to the appropriate endpoint.

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
default_model = "MiniMax-M2"
```

or using the `minimax` provider alias:

```toml
[agent]
provider = "minimax"
# default_model is optional; defaults to MiniMax-M2
default_model = "MiniMax-M2"
api_key_env = "ANTHROPIC_API_KEY"
```

### 4. Start Using MiniMax

Run VTCode and it will automatically use the MiniMax-M2 model through the Anthropic-compatible API:

```bash
vtcode
```

## Configuration Options

### Basic Configuration

```toml
[agent]
provider = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"
default_model = "MiniMax-M2"
```

### Custom Base URL Override

If you need to override the base URL (for example, to use a proxy or different endpoint):

```bash
export ANTHROPIC_BASE_URL=https://your-custom-endpoint.com/anthropic/v1
```

Or set it directly in your configuration if your setup supports it.

## Supported Features

The MiniMax-M2 model through the Anthropic-compatible API supports:

- Text generation
- Streaming responses
- Tool calling (function calling)
- System prompts
- Temperature control
- Max tokens configuration
- Reasoning content (thinking blocks)

## Limitations

According to MiniMax's documentation, the following features are not currently supported:

- Image input (`type="image"`)
- Document input (`type="document"`)
- Some Anthropic-specific parameters may be ignored (e.g., `top_k`, `stop_sequences`, `service_tier`)

## Example Usage

### Simple Query

```bash
vtcode ask "Explain machine learning in simple terms"
```

### With Tool Calling

The MiniMax-M2 model supports function calling just like Claude models. VTCode's built-in tools (file operations, terminal commands, etc.) work seamlessly.

### Streaming Mode

Streaming is fully supported and works automatically when enabled in your configuration.

## Temperature Range

Note that MiniMax requires temperature values in the range (0.0, 1.0]. Values outside this range will return an error. The recommended value is 1.0.

## API Endpoint

VTCode automatically routes MiniMax-M2 requests to:
```
https://api.minimax.io/anthropic/v1/messages
```

The base URL is `https://api.minimax.io/anthropic/v1`, and VTCode appends `/messages` automatically.

This is handled transparently - you don't need to configure anything special unless you want to override the base URL via environment variable.

### Droid-style custom model example

You can mirror Droid's custom model config using VTCode's dot-config (`~/.vtcode/config.toml`). Add a `minimax` provider entry with base URL and API key:

```toml
[providers]
  [providers.minimax]
  api_key = "${ANTHROPIC_API_KEY}"
  base_url = "https://api.minimax.io/anthropic/v1"
  model = "MiniMax-M2"
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
1. Ensure you're using the exact model name: `MiniMax-M2` (case-sensitive)
2. Check that your API key has access to the MiniMax-M2 model

### Temperature Errors

If you see temperature-related errors:
1. Ensure temperature is in the range (0.0, 1.0]
2. The default temperature in VTCode should work fine

## Comparison with Claude Models

| Feature | Claude Models | MiniMax-M2 |
|---------|--------------|------------|
| Text Generation |  |  |
| Streaming |  |  |
| Tool Calling |  |  |
| System Prompts |  |  |
| Image Input |  |  |
| Document Input |  |  |
| Reasoning Effort |  |  |
| Prompt Caching |  | ? |

## Related Documentation

- [Anthropic Provider Documentation](./anthropic-provider.md)
- [Model Configuration](../config/models.md)
- [Tool Calling Guide](./tool-calling.md)

## Additional Resources

- [MiniMax Official Documentation](https://www.minimax.chat/docs)
- [MiniMax Anthropic API Compatibility Guide](https://www.minimax.chat/docs/guides/anthropic-api)
- [MiniMax-M2 Function Calling Guide](https://www.minimax.chat/docs/guides/function-call)
