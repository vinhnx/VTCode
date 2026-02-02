# Anthropic API Compatibility

VT Code provides compatibility with the [Anthropic Messages API](https://docs.anthropic.com/en/api/messages) to help connect existing applications to VT Code, including tools like Claude Code.

## Overview

The Anthropic API compatibility server allows existing applications that expect the Anthropic Messages API to seamlessly connect to VT Code. This enables:

- Integration with Claude Code and other Anthropic-compatible tools
- Support for Anthropic's rich feature set including tool calling, streaming, and vision
- Multi-provider LLM support through VT Code's unified interface

## Quick Links

- [Reducing Latency with Claude](./anthropic-latency.md)
- [Anthropic API Documentation](https://docs.anthropic.com/en/api/messages)

## Getting Started

### Prerequisites

Make sure you have the `anthropic-api` feature enabled when building VT Code:

```bash
# Build with Anthropic API support
cargo build --features anthropic-api
```

### Starting the Server

```bash
# Start the Anthropic API server
vtcode anthropic-api --port 11434 --host 127.0.0.1

# Or with default settings (port 11434, host 127.0.0.1)
vtcode anthropic-api
```

### Environment Variables

To use with Claude Code or other Anthropic-compatible tools, set these environment variables:

```bash
export ANTHROPIC_AUTH_TOKEN=ollama  # required but ignored
export ANTHROPIC_BASE_URL=http://localhost:11434
export ANTHROPIC_API_KEY=ollama  # required but ignored
```

## API Endpoints

### `/v1/messages`

The main endpoint that mirrors Anthropic's Messages API:

```bash
curl -X POST http://localhost:11434/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: ollama" \
  -H "anthropic-version: 2023-06-01" \
  -d '{
    "model": "claude-haiku-4-5",
    "max_tokens": 1024,
    "messages": [
      {
        "role": "user",
        "content": "Hello, how are you?"
      }
    ]
  }'
```

## Supported Features

### Messages

- Text content blocks
- Image content blocks (base64 encoded)
- Tool use blocks
- Tool result blocks
- Thinking blocks

### Streaming

- Full Server-Sent Events (SSE) support
- Streaming events: `message_start`, `content_block_start`, `content_block_delta`, `message_delta`, `message_stop`
- Real-time response streaming

### Tool Calling

- Tool definition and registration
- Parallel tool execution
- Tool result handling
- Complex tool chains

### Vision

- Image content support
- Base64-encoded image processing
- Multimodal inputs

### Other Features

- System prompts
- Multi-turn conversations
- Temperature and sampling controls
- Stop sequences
- Token usage reporting

## Configuration

The Anthropic API server uses VT Code's standard configuration system. The server will use the same LLM provider and settings configured in your `vtcode.toml` file.

## Using with Claude Code

Claude Code can be configured to use VT Code as its backend:

```bash
ANTHROPIC_AUTH_TOKEN=ollama ANTHROPIC_BASE_URL=http://localhost:11434 ANTHROPIC_API_KEY=ollama claude --model claude-3-5-sonnet
```

Or set the environment variables in your shell profile:

```bash
export ANTHROPIC_AUTH_TOKEN=ollama
export ANTHROPIC_BASE_URL=http://localhost:11434
export ANTHROPIC_API_KEY=ollama
```

Then run Claude Code with any model supported by VT Code:

```bash
# Local models
claude --model claude-3-5-sonnet

# Cloud models
claude --model glm-4-air
claude --model minimax-01
```

## Default Model Names

For tooling that relies on default Anthropic model names such as `claude-3-5-sonnet`, you can create aliases:

```bash
vtcode cp qwen3-coder claude-3-5-sonnet
```

Afterwards, this new model name can be specified in the `model` field:

```bash
curl http://localhost:11434/v1/messages \
    -H "Content-Type: application/json" \
    -d '{
        "model": "claude-haiku-4-5",
        "max_tokens": 1024,
        "messages": [
            {
                "role": "user",
                "content": "Hello!"
            }
        ]
    }'
```

## Differences from Anthropic API

### Behavior Differences

- API key is accepted but not validated
- `anthropic-version` header is accepted but not used
- Token counts are approximations based on the underlying model's tokenizer

### Not Supported

- `/v1/messages/count_tokens` endpoint
- `tool_choice` parameter (forcing specific tool use or disabling tools)
- `metadata` parameter (request metadata like user_id)
- Prompt caching with `cache_control` blocks
- Batches API (`/v1/messages/batches`)
- Citations content blocks
- PDF support with `document` content blocks
- Server-sent errors during streaming (errors return HTTP status instead)

### Partial Support

- Image content: Base64 images supported; URL images not supported
- Extended thinking: Basic support; `budget_tokens` accepted but not enforced

## Troubleshooting

### Common Issues

1. **Connection refused**: Make sure the server is running and the correct port is specified
2. **Authentication errors**: The API key is required by the Anthropic API but not validated by VT Code
3. **Model not found**: Ensure the model is available in VT Code's configuration

### Verifying the Server

Test that the server is running:

```bash
curl -X POST http://localhost:11434/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: ollama" \
  -d '{
    "model": "claude-haiku-4-5",
    "max_tokens": 10,
    "messages": [
      {
        "role": "user",
        "content": "Hi"
      }
    ]
  }'
```

## Security

The Anthropic API compatibility server inherits VT Code's security model:

- All operations are confined to the workspace boundaries
- Tool policies and execution controls apply
- Human-in-the-loop approvals can be configured
