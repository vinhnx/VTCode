# Ollama Provider Guide

Ollama can serve models locally on your machine or proxy larger releases through the Ollama Cloud service. VT Code integrates with both deployment modes so you can keep lightweight workflows offline while bursting to the cloud for heavier jobs.

## Prerequisites

- Ollama installed and running locally ([download](https://ollama.com/download))
- Optional: Ollama Cloud account with an [API key](https://ollama.com/settings/keys) for remote models
- At least one model pulled locally or in your cloud workspace (e.g., `ollama pull llama3:8b` or `ollama pull gpt-oss:120b-cloud`)

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

- `OLLAMA_BASE_URL` (optional): Custom Ollama endpoint (defaults to `http://localhost:11434`). Set to `https://ollama.com` to send requests directly to Ollama Cloud.
- `OLLAMA_API_KEY` (optional): Required when connecting to Ollama Cloud. Not needed for purely local workloads.

### VT Code Configuration

Set up `vtcode.toml` in your project root:

```toml
[agent]
provider = "ollama"                    # Ollama provider
default_model = "llama3:8b"           # Any locally available model
# Note: API key only required when targeting Ollama Cloud

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
vtcode --provider ollama --model gpt-oss:120b-cloud ask "Plan this large migration"
```

The `/model` picker now lists the core Ollama catalog so you can choose them without typing IDs:

- `gpt-oss:20b` (local)
- `gpt-oss:120b-cloud`
- `deepseek-v3.1:671b-cloud`
- `kimi-k2:1t-cloud`
- `qwen3:1.7b`
- `qwen3-coder:480b-cloud`
- `glm-4.6:cloud`

These entries appear beneath the Ollama provider section alongside the "Custom Ollama model" option.

## OpenAI OSS Models Support

VT Code includes support for OpenAI's open-source models that can be run via Ollama locally or through the cloud preview:

- `gpt-oss-20b`: Open-source 20B parameter model from OpenAI (local)
- `gpt-oss:120b-cloud`: Cloud-hosted 120B parameter model managed by Ollama

To use these models:

```bash
# Pull the model first (local or cloud)
ollama pull gpt-oss-20b
ollama pull gpt-oss:120b-cloud

# Use in VT Code
vtcode --provider ollama --model gpt-oss-20b ask "Code review this function"
vtcode --provider ollama --model gpt-oss:120b-cloud ask "Assist with this architecture review"
```

## Tool calling and web search integration

Ollama's API exposes OpenAI-compatible [tool calling](https://docs.ollama.com/capabilities/tool-calling) as well as the [web search](https://docs.ollama.com/capabilities/web-search) helpers. VT Code now forwards tool definitions to Ollama and surfaces any `tool_calls` responses from the model. A typical workflow looks like this:

1. Define tools in `vtcode.toml` (or via slash commands) with JSON schemas that match your functions. For example, expose `web_search` and `web_fetch` so the agent can call Ollama's hosted knowledge tools.
2. The agent will stream back `tool_calls` with structured arguments. VT Code automatically routes each call to the configured tool runner and includes the results as `tool` messages in the follow-up request.
3. Ollama's responses can include multiple tools per turn. VT Code enforces `tool_call_id` requirements for reliability while still letting the model decide when to call a tool.

Because the provider now understands these payloads you can mix Ollama's native utilities with your existing MCP toolchain.

## Thinking traces and streaming

Thinking-capable models such as `gpt-oss` and `qwen3` emit a dedicated `thinking` channel ([docs](https://docs.ollama.com/capabilities/thinking)). Set the reasoning effort to `medium` or `high` (e.g., `vtcode --reasoning high`) or configure `reasoning_effort = "high"` in `vtcode.toml` and VT Code forwards the appropriate `think` parameter (`low`/`medium`/`high` for GPT-OSS, boolean for Qwen). During streaming runs you will now see separate "Reasoning" lines followed by the final answer tokens so you can inspect or hide the trace as needed.

Ollama continues to support incremental streaming ([docs](https://docs.ollama.com/capabilities/streaming)), and VT Code uses it by default. Combine reasoning with streaming to watch the model deliberate before it produces the final response.

## Using Ollama Cloud directly

When you have an Ollama API key you can target the managed endpoint without running a local server:

```bash
export OLLAMA_API_KEY="sk-..."
export OLLAMA_BASE_URL="https://ollama.com"

vtcode --provider ollama --model gpt-oss:120b-cloud ask "Summarize this spec"
```

VT Code automatically attaches the bearer token to requests when the API key is present.

## Troubleshooting

### Common Issues

1. **"Connection refused" errors**: Ensure Ollama server is running (`ollama serve`) or that `OLLAMA_BASE_URL` points to a reachable endpoint
2. **Model not found**: Ensure the requested model has been pulled (`ollama pull MODEL_NAME`)
3. **Unauthorized (401) errors**: Set `OLLAMA_API_KEY` when targeting Ollama Cloud
4. **Performance issues**: Consider model size - larger models require more RAM
5. **Memory errors**: For large local models like gpt-oss-120b, ensure sufficient RAM (64GB+ recommended)

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
