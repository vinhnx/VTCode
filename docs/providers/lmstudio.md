# LM Studio Provider Guide

LM Studio exposes both a native REST API and OpenAI-compatible HTTP server so you can run local or LAN-hosted models without changing your existing API integration. VT Code uses the OpenAI-compatible endpoints for inference, which means streaming, tool calling, structured outputs work seamlessly while keeping inference on your own hardware.

## Prerequisites

- LM Studio 0.3.6+ installed on your machine ([download](https://lmstudio.ai/download))
- Enable the Developer HTTP server via the LM Studio desktop app or CLI
  ([docs](https://lmstudio.ai/docs/developer))
- At least one model downloaded inside LM Studio (e.g., Qwen 3 8B, DeepSeek R1 Qwen3 8B,
  GPT-OSS 20B, Llama 3.1 8B, Qwen 2.5 7B, or Gemma 3 12B)

## Installation and Setup

1. **Install LM Studio**: Follow the platform-specific installer from
   [lmstudio.ai/download](https://lmstudio.ai/download).
2. **Download a model**: In the app, open the "Models" tab and pull one of the
   supported open models (Qwen 3 8B, DeepSeek R1 Qwen3 8B, GPT-OSS 20B, Llama 3.1
   8B, Qwen 2.5 7B, Gemma 3 12B, or any other compatible model hosted in the LM Studio
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
    # Native REST API
    curl http://localhost:1234/api/v0/models

    # OpenAI-compatible API
    curl http://localhost:1234/v1/models
    ```

    The response lists every model LM Studio currently exposes through the API.

## Configuration

### Environment Variables

- `LMSTUDIO_BASE_URL` (optional): Override the API endpoint (defaults to
  `http://localhost:1234/v1`). Useful when the server runs on another port or host.
- `LMSTUDIO_API_KEY` (optional): Set when you enable authentication in the LM Studio
  server. Leave unset for local testing without authentication.

### VT Code Configuration

Configure `vtcode.toml` in your workspace to point at LM Studio:

```toml
[agent]
provider = "lmstudio"                     # LM Studio provider
default_model = "lmstudio-community/Qwen3-8B"

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

LM Studio provides multiple API surfaces:

### OpenAI-Compatible Endpoints (`/v1/*`)

Used by VT Code for maximum compatibility:

- **`POST /v1/chat/completions`**: Standard OpenAI chat completions (streaming, tool calling, structured output)
- **`POST /v1/embeddings`**: Generate embeddings
- **`GET /v1/models`**: List loaded models (or all downloaded models when JIT is enabled)

### Native REST API (`/api/v0/*`)

Enhanced API with richer metadata:

- **`GET /api/v0/models`**: List all downloaded models with metadata (type, publisher, arch, quantization, context length)
- **`GET /api/v0/models/{model}`**: Get detailed model info
- **`POST /api/v0/chat/completions`**: Chat completions with enhanced stats (tokens/sec, time-to-first-token)
- **`POST /api/v0/models/load`**: Load a model into memory
- **`POST /api/v0/models/unload`**: Unload a model from memory

VT Code currently uses the OpenAI-compatible endpoints. Set `LMSTUDIO_USE_NATIVE_API=true` to use the native REST API for model listing.

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

| Feature                  | `/v1/chat/completions` | `/api/v0/chat/completions` |
| ------------------------ | ---------------------- | -------------------------- |
| Streaming                | v                      | v                          |
| Tool calling             | v                      | v                          |
| Structured output        | v                      | v                          |
| Enhanced stats           | x                      | v                          |
| Model info in response   | x                      | v                          |

VT Code forwards tool definitions, function-calling metadata, and JSON schema expectations so models can call tools or produce structured output. Streaming is enabled by default, and you will see incremental tokens in the TUI just as you would with remote OpenAI deployments.

Because the provider shares the OpenAI surface area, features such as `parallel_tool_calls`, reasoning effort flags, and JSON Schema validation behave consistently—subject to the capabilities of the model you are running locally.

### Key LM Studio Features

- **Tool Calling** (since 0.3.6): Native tool use via OpenAI-compatible API for Qwen, Llama 3.1+, Mistral models
- **Structured Output**: JSON schema enforcement via `response_format` (GGUF uses grammar-based sampling, MLX uses Outlines)
- **Reasoning Content** (since 0.3.9): Separate `reasoning_content` field for DeepSeek R1 models (enable in App Settings > Developer)
- **Idle TTL**: Set time-to-live for JIT-loaded models to auto-unload after inactivity
- **Auto-Evict**: Automatically unload previous JIT models when loading new ones
- **JIT Loading**: On-demand model loading when enabled (default: on)

## Troubleshooting

1. **Connection refused**: Ensure the LM Studio server is running and that
   `LMSTUDIO_BASE_URL` points to the correct host/port. Default is `http://localhost:1234`.
2. **Model not found**: Confirm the model appears in the LM Studio catalog and that the
   server exposes it via `GET /v1/models` or `GET /api/v0/models`.
3. **401 Unauthorized**: Provide the configured API key through `LMSTUDIO_API_KEY` if
   authentication is enabled.
4. **Slow responses**: Local inference speed depends on your hardware and the model
   size. Consider using smaller models (Qwen 2.5 7B, Gemma 3 12B) for faster iteration.
5. **Tool payload errors**: Check the LM Studio server logs to ensure your runtime
   supports the tools and structured outputs you are invoking.
6. **Server not starting**: Run `lms server start` from the command line to see detailed error messages.
7. **Model download fails**: Use `lms get <model>` to download models directly via CLI.

## Additional Resources

- [LM Studio Developer Docs](https://lmstudio.ai/docs/developer)
- [OpenAI-Compatible Endpoints](https://lmstudio.ai/docs/developer/openai-compat)
- [REST API Reference](https://lmstudio.ai/docs/developer/rest)
- [Tool Use Documentation](https://lmstudio.ai/docs/developer/tools)
- [TTL and Auto-Evict](https://lmstudio.ai/docs/developer/ttl-and-auto-evict)
- [Structured Output](https://lmstudio.ai/docs/developer/structured-output)
