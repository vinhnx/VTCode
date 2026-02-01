# LM Studio Provider Guide

LM Studio exposes both a native v1 REST API and OpenAI-compatible HTTP server so you can run local or LAN-hosted models without changing your existing API integration. VT Code supports both the native LM Studio v1 API (introduced in 0.4.0) and OpenAI-compatible endpoints, which means streaming, tool calling, structured outputs, and stateful chats work seamlessly while keeping inference on your own hardware.

## Prerequisites

- LM Studio 0.4.0+ installed on your machine ([download](https://lmstudio.ai/download))
- Enable the Developer HTTP server via the LM Studio desktop app or CLI
  ([docs](https://lmstudio.ai/docs/developer))
- At least one model downloaded inside LM Studio (e.g., Meta Llama 3.1 8B or 3 8B,
  Qwen2.5 7B, Gemma 2 2B/9B IT, or Phi-3.1 Mini 4K)

## Installation and Setup

1. **Install LM Studio**: Follow the platform-specific installer from
   [lmstudio.ai/download](https://lmstudio.ai/download).
2. **Download a model**: In the app, open the "Models" tab and pull one of the
   supported open models (Meta Llama 3.1 8B, Meta Llama 3 8B, Qwen2.5 7B, Gemma 2
   2B/9B IT, Phi-3.1 Mini 4K, or any other compatible model hosted in the LM Studio
   catalog). Alternatively, use the CLI:
   ```bash
   lms get deepseek-r1  # Download by keyword
   lms get <hugging-face-url>  # Download by URL
   ```
3. **Start the Developer server**:
   - **GUI**: From the "Developer" panel, enable the server and confirm the port (defaults to `1234`).
   - **CLI**: Run `lms server start` to launch the server. Append `--host 0.0.0.0` to expose it to other machines on your network.
4. **Verify the server**: Send a quick health check:
   ```bash
   # Native v1 API
   curl http://localhost:1234/api/v1/models
   
   # OpenAI-compatible API (still supported)
   curl http://localhost:1234/v1/models
   ```
   The response lists every model LM Studio currently exposes through the API.

## Configuration

### Environment Variables

- `LMSTUDIO_BASE_URL` (optional): Override the API endpoint (defaults to
  `http://localhost:1234/v1` for OpenAI-compatible endpoints, or `http://localhost:1234/api/v1` for native v1 API). Useful when the server runs on another port or host.
- `LMSTUDIO_API_KEY` (optional): Set when you enable authentication in the LM Studio
  server (introduced in 0.4.0). Leave unset for local testing without authentication.

### VT Code Configuration

Configure `vtcode.toml` in your workspace to point at LM Studio:

```toml
[agent]
provider = "lmstudio"                     # LM Studio provider
default_model = "lmstudio-community/meta-llama-3.1-8b-instruct"

[tools]
default_policy = "prompt"

[tools.policies]
read_file = "allow"
write_file = "prompt"
run_pty_cmd = "prompt"
```

You can also override the provider and model via CLI:

```bash
vtcode --provider lmstudio --model lmstudio-community/qwen2.5-7b-instruct
```

## API Endpoints

LM Studio 0.4.0+ provides multiple API surfaces:

### Native v1 REST API (`/api/v1/*`)

The recommended API for new integrations, offering enhanced features:

- **`POST /api/v1/chat`**: Chat with a model (supports streaming, stateful chats, MCP)
- **`GET /api/v1/models`**: List available models
- **`POST /api/v1/models/load`**: Load a model into memory
- **`POST /api/v1/models/unload`**: Unload a model from memory
- **`POST /api/v1/models/download`**: Download a model
- **`GET /api/v1/models/download/status`**: Check download status

### OpenAI-Compatible Endpoints (`/v1/*`)

Maintained for backward compatibility:

- **`POST /v1/chat/completions`**: Standard OpenAI chat completions
- **`POST /v1/responses`**: Stateful interactions with `previous_response_id`, custom tools, and MCP support
- **`POST /v1/embeddings`**: Generate embeddings
- **`GET /v1/models`**: List models

### Anthropic-Compatible Endpoints (`/v1/*`)

Added in LM Studio 0.4.1:

- **`POST /v1/messages`**: Anthropic Messages API compatibility

VT Code currently uses the OpenAI-compatible endpoints for maximum compatibility. Future versions may migrate to the native v1 API for enhanced features.

## Using Custom LM Studio Models

The `/model` picker now lists LM Studio's default catalog so you can select a model
without typing IDs manually. Choose "Custom LM Studio model" to enter any other model
ID exposed by the LM Studio server.

When you sideload a GGUF or add a local GGML/ONNX pipeline through LM Studio, make sure
it appears under the server's `GET /v1/models` response. Once listed, VT Code can target
it by passing the exact model ID via CLI or configuration.

## Tool Calling, Structured Output, and Streaming

LM Studio's API stack supports multiple inference endpoints with varying capabilities:

### Feature Comparison

| Feature | `/api/v1/chat` | `/v1/responses` | `/v1/chat/completions` | `/v1/messages` |
|---------|----------------|-----------------|------------------------|----------------|
| Streaming | ✅ | ✅ | ✅ | ✅ |
| Stateful chat | ✅ | ✅ | ❌ | ❌ |
| Remote MCPs | ✅ | ✅ | ❌ | ❌ |
| LM Studio MCPs | ✅ | ✅ | ❌ | ❌ |
| Custom tools | ❌ | ✅ | ✅ | ✅ |
| Assistant messages | ❌ | ✅ | ✅ | ✅ |
| Model load events | ✅ | ❌ | ❌ | ❌ |
| Prompt processing events | ✅ | ❌ | ❌ | ❌ |
| Context length control | ✅ | ❌ | ❌ | ❌ |

VT Code forwards tool definitions, function-calling metadata, and JSON schema expectations so models can call tools or produce structured output. Streaming is enabled by default, and you will see incremental tokens in the TUI just as you would with remote OpenAI deployments.

Because the provider shares the OpenAI surface area, features such as `parallel_tool_calls`, reasoning effort flags, and JSON Schema validation behave consistently—subject to the capabilities of the model you are running locally.

### New Features in 0.4.0+

- **Stateful Chats**: Use `previous_response_id` to maintain conversation context across requests
- **MCP via API**: Access Model Context Protocol tools through the API
- **Authentication**: Configure API tokens for secure access
- **Model Management**: Load, unload, and download models programmatically
- **Idle TTL**: Set time-to-live for models loaded via API (auto-evict after inactivity)

## Troubleshooting

1. **Connection refused**: Ensure the LM Studio server is running and that
   `LMSTUDIO_BASE_URL` points to the correct host/port. Default is `http://localhost:1234`.
2. **Model not found**: Confirm the model appears in the LM Studio catalog and that the
   server exposes it via `GET /api/v1/models` or `GET /v1/models`.
3. **401 Unauthorized**: Provide the configured API key through `LMSTUDIO_API_KEY` if
   authentication is enabled (LM Studio 0.4.0+).
4. **Slow responses**: Local inference speed depends on your hardware and the model
   size. Consider using smaller models (Gemma 2 2B, Qwen2.5 7B) for faster iteration.
5. **Tool payload errors**: Check the LM Studio server logs to ensure your runtime
   supports the tools and structured outputs you are invoking.
6. **Server not starting**: Run `lms server start` from the command line to see detailed error messages.
7. **Model download fails**: Use `lms get <model>` to download models directly via CLI.

## Additional Resources

- [LM Studio Developer Docs](https://lmstudio.ai/docs/developer)
- [Native v1 REST API](https://lmstudio.ai/docs/developer/rest)
- [OpenAI-Compatible Endpoints](https://lmstudio.ai/docs/developer/openai-compat)
- [Anthropic-Compatible Endpoints](https://lmstudio.ai/docs/developer/anthropic-compat)
- [API Changelog](https://lmstudio.ai/docs/developer/api-changelog)
- [MCP via API](https://lmstudio.ai/docs/developer/core/mcp)
- [Stateful Chats](https://lmstudio.ai/docs/developer/rest/stateful-chats)
