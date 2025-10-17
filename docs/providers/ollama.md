# Ollama Provider Guide

Ollama is a local AI model runner that allows you to run LLMs directly on your machine without requiring internet access or API keys. VT Code integrates with Ollama to provide local AI capabilities for coding tasks, and also supports the hosted [Ollama Cloud](https://docs.ollama.com/cloud) service when you prefer managed infrastructure.

## Prerequisites

- Ollama installed and running locally ([download](https://ollama.com/download))
- At least one model pulled locally (e.g., `ollama pull llama3:8b`)

## Installation and Setup

1. **Install Ollama**: Download from [ollama.com](https://ollama.com/download) and follow platform-specific instructions
2. **Start Ollama server**: Run `ollama serve` in a terminal
3. **Pull a model**: Choose and download a model to use:
   ```bash
   # Popular coding models
   ollama pull llama3:8b
   ollama pull codellama:7b
   ollama pull mistral:7b
   ollama pull qwen3:1.7b
   ollama pull deepseek-coder:6.7b
   ollama pull phind-coder:34b
   
   # List available local models
   ollama list
   ```

## Configuration

### Environment Variables

- `OLLAMA_BASE_URL` (optional): Custom Ollama endpoint (default: `http://localhost:11434`)
- `OLLAMA_API_KEY` (optional): Required when targeting Ollama Cloud (`https://ollama.com`)

### VT Code Configuration

Set up `vtcode.toml` in your project root:

```toml
[agent]
provider = "ollama"                    # Ollama provider
default_model = "llama3:8b"           # Any locally available model
# Note: API key only required when using Ollama Cloud

[tools]
default_policy = "prompt"             # Safety: "allow", "prompt", or "deny"

[tools.policies]
read_file = "allow"                   # Always allow file reading
write_file = "prompt"                 # Prompt before modifications
run_terminal_cmd = "prompt"           # Prompt before commands
```

## Using Custom Ollama Models

VT Code supports custom Ollama models through the interactive model picker or directly via CLI:

```bash
# Using the interactive model picker (select "custom-ollama")
vtcode

# Direct CLI usage with custom model
vtcode --provider ollama --model mistral:7b ask "Review this code"
vtcode --provider ollama --model codellama:7b ask "Explain this function"
vtcode --provider ollama --model gpt-oss-20b ask "Help with this implementation"
```

### Cloud coding models

Ollama's latest hosted releases, showcased in the [New coding models & integrations blog post](https://ollama.com/blog/coding-models), are available directly from VT Code:

- `glm-4.6:cloud` for the GLM 4.6 coding model
- `qwen3-coder:480b-cloud` for the hosted 480B Qwen3 coder
- `qwen3-coder:480b` and `qwen3-coder:30b` when you have the VRAM to run them locally

To target Ollama Cloud, provide your API key and point the provider at `https://ollama.com`:

```bash
export OLLAMA_API_KEY="your_cloud_key"

vtcode --provider ollama \
  --model glm-4.6:cloud \
  --base-url https://ollama.com \
  ask "Generate a Typescript CRUD backend"
```

VT Code automatically injects the `Authorization: Bearer` header whenever an API key is configured, so the same command works for streaming, tool-calling, and JSON request workflows.

## Tool Calling

Ollama supports structured tool calling with the same schema used by OpenAI-compatible APIs. When invoking VT Code via JSON,
include your `tools` array and optional `tool_choice` directive to guide the model:

```json
{
  "model": "llama3:8b",
  "messages": [
    {"role": "user", "content": "What's the weather in Seattle?"}
  ],
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "get_weather",
        "description": "Fetch the latest forecast",
        "parameters": {
          "type": "object",
          "properties": {"location": {"type": "string"}},
          "required": ["location"]
        }
      }
    }
  ],
  "tool_choice": "required"
}
```

`tool_choice` accepts `"auto"`, `"none"`, `"required"`, or a specific function descriptor (`{"type":"function","function":{"name":"..."}}`). VT Code forwards these values to Ollama so you can disable tool usage, force a tool call, or pin a particular function when needed.

### Reasoning traces

If you enable reasoning (`reasoning_effort = "medium"` or similar in `vtcode.toml`), VT Code automatically sets Ollama's `think` flag. Streaming sessions show the model's intermediate reasoning as separate "thinking" updates while still emitting the final assistant response as normal completion tokens.

## OpenAI OSS Models Support

VT Code includes support for OpenAI's open-source models that can be run via Ollama:

- `gpt-oss-20b`: Open-source 20B parameter model from OpenAI

To use these models:

```bash
# Pull the model first
ollama pull gpt-oss-20b

# Use in VT Code
vtcode --provider ollama --model gpt-oss-20b ask "Code review this function"
```

## Troubleshooting

### Common Issues

1. **"Connection refused" errors**: Ensure Ollama server is running (`ollama serve`)
2. **Model not found**: Ensure the requested model has been pulled (`ollama pull MODEL_NAME`)
3. **Performance issues**: Consider model size - larger models require more RAM
4. **Memory errors**: For large models like gpt-oss-120b, ensure sufficient RAM (64GB+ recommended)

### Testing Ollama Connection

Verify Ollama is working correctly:

```bash
# Test basic Ollama functionality
ollama run llama3:8b

# Test via API call
curl http://localhost:11434/api/tags
```

## Performance Notes

- Local models don't require internet connection
- Performance varies significantly based on model size and local hardware
- Larger models (30B+) require substantial RAM (32GB+) for reasonable performance
- Smaller models (7B-13B) work well on consumer hardware with 16GB+ RAM

## Sharing models with Droid

The blog's ["Usage with Droid"](https://ollama.com/blog/coding-models#usage-with-droid) example configures Factory AI's Droid CLI against Ollama's OpenAI-compatible `/v1` endpoint. When you want VT Code and Droid to share the same local or proxied models, keep the VT Code `base_url` pointed at the root Ollama host (for example `http://localhost:11434`). VT Code talks directly to `/api/chat`, so omitting the `/v1` suffix avoids conflicting with Droid's compatibility shim while still letting both tools pull models like `glm-4.6:cloud` from the same server.

## Using Ollama Cloud

Ollama Cloud exposes the same API as the local runtime. To enable it in VT Code:

1. Generate an API key from the [Ollama Cloud dashboard](https://docs.ollama.com/cloud).
2. Set `OLLAMA_API_KEY` in your shell (or configure `providers.ollama.api_key` in `vtcode.toml`).
3. Point the base URL to the hosted service:

```toml
[providers.ollama]
base_url = "https://ollama.com"
api_key = "${OLLAMA_API_KEY}"
```

All tool-calling and streaming capabilities work identically in Cloud mode. The provider automatically forwards structured tool definitions and handles tool call responses, so existing workflows continue to function whether you run Ollama locally or in the cloud.