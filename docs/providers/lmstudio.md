# LM Studio Provider Guide

LM Studio exposes an OpenAI-compatible HTTP server so you can run local or LAN-hosted
models without changing your existing API integration. VT Code reuses its OpenAI
adapter to talk to LM Studio, which means streaming, tool calling, and structured
outputs work the same way while keeping inference on your own hardware.

## Prerequisites

- LM Studio installed on your machine ([download](https://lmstudio.ai/download))
- Enable the Developer HTTP server via the LM Studio desktop app or CLI
  ([docs](https://lmstudio.ai/docs/developer/core/server))
- At least one model downloaded inside LM Studio (e.g., Meta Llama 3.1 8B or 3 8B,
  Qwen2.5 7B, Gemma 2 2B/9B IT, or Phi-3.1 Mini 4K)

## Installation and Setup

1. **Install LM Studio**: Follow the platform-specific installer from
   [lmstudio.ai/download](https://lmstudio.ai/download).
2. **Download a model**: In the app, open the "Models" tab and pull one of the
   supported open models (Meta Llama 3.1 8B, Meta Llama 3 8B, Qwen2.5 7B, Gemma 2
   2B/9B IT, Phi-3.1 Mini 4K, or any other compatible model hosted in the LM Studio
   catalog).
3. **Start the Developer server**:
   - **GUI**: From the "Developer" panel, enable the OpenAI-compatible server and
     confirm the port (defaults to `1234`).
   - **CLI**: Run `lmstudio server start --port 1234 --openai` to launch the server
     manually. Append `--host 0.0.0.0` to expose it to other machines on your network.
4. **Verify the server**: Send a quick health check:
   ```bash
   curl http://localhost:1234/v1/models
   ```
   The response lists every model LM Studio currently exposes through the API.

## Configuration

### Environment Variables

- `LMSTUDIO_BASE_URL` (optional): Override the API endpoint (defaults to
  `http://localhost:1234/v1`). Useful when the server runs on another port or host.
- `LMSTUDIO_API_KEY` (optional): Set when you enable key-based auth in the LM Studio
  server. Leave unset for local testing without authentication.

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
run_terminal_cmd = "prompt"
```

You can also override the provider and model via CLI:

```bash
vtcode --provider lmstudio --model lmstudio-community/qwen2.5-7b-instruct
```

## Using Custom LM Studio Models

The `/model` picker now lists LM Studio's default catalog so you can select a model
without typing IDs manually. Choose "Custom LM Studio model" to enter any other model
ID exposed by the LM Studio server.

When you sideload a GGUF or add a local GGML/ONNX pipeline through LM Studio, make sure
it appears under the server's `GET /v1/models` response. Once listed, VT Code can target
it by passing the exact model ID via CLI or configuration.

## Tool Calling, Structured Output, and Streaming

LM Studio's OpenAI-compatible stack supports the Chat Completions, Responses, and
Embeddings APIs ([docs](https://lmstudio.ai/docs/developer/openai-compat)). VT Code
forwards tool definitions, function-calling metadata, and JSON schema expectations so
models can call tools or produce structured output exactly like the hosted OpenAI
provider. Streaming is enabled by default, and you will see incremental tokens in the
TUI just as you would with remote OpenAI deployments.

Because the provider shares the OpenAI surface area, features such as
`parallel_tool_calls`, reasoning effort flags, and JSON Schema validation behave
consistently—subject to the capabilities of the model you are running locally.

## Troubleshooting

1. **Connection refused**: Ensure the LM Studio server is running and that
   `LMSTUDIO_BASE_URL` points to the correct host/port.
2. **Model not found**: Confirm the model appears in the LM Studio catalog and that the
   server exposes it via `GET /v1/models`.
3. **401 Unauthorized**: Provide the configured API key through `LMSTUDIO_API_KEY` if
   authentication is enabled.
4. **Slow responses**: Local inference speed depends on your hardware and the model
   size. Consider using smaller models (Gemma 2 2B, Qwen2.5 7B) for faster iteration.
5. **Tool payload errors**: Check the LM Studio server logs to ensure your runtime
   supports the tools and structured outputs you are invoking.

Refer to the official [LM Studio developer docs](https://lmstudio.ai/docs/developer)
for deeper configuration details, including structured output schemas, tool
registration, and server customization.
