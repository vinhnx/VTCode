# Hugging Face Provider Update Summary

## Overview
Updated the Hugging Face Inference Providers implementation to align with the official Chat Completion API documentation.

## Changes Made

### 1. Model List Expansion (`vtcode-config/src/constants.rs`)

#### Added Recommended Conversational LLMs:
- **google/gemma-2-2b-it** - Text-generation model trained to follow instructions
- **Qwen/Qwen3-Coder-480B-A35B-Instruct** - Powerful coding model
- **Qwen/Qwen3-4B-Thinking-2507** - Small model with reasoning capabilities
- **Qwen/Qwen2.5-7B-Instruct-1M** - Strong conversational model with 1M context
- **Qwen/Qwen2.5-Coder-32B-Instruct** - Code generation specialist
- **deepseek-ai/DeepSeek-R1** - Powerful reasoning-based open LLM
- **zai-org/GLM-4.5** - Powerful text generation model

#### Added Recommended VLM:
- **zai-org/GLM-4.5V** - Cutting-edge reasoning vision language model

#### Updated Default Model:
- Changed from `openai/gpt-oss-20b` to `openai/gpt-oss-120b` (more capable flagship model)

### 2. Documentation Updates (`docs/models.json`)

Added comprehensive metadata for all new models including:
- Model IDs and display names
- Descriptions matching official documentation
- Reasoning and tool calling capabilities
- Input/output modalities (text, image for VLMs)
- Context window sizes
- Structured output support flags
- Vision capabilities for VLMs

### 3. Responses API Migration (Beta)

Successfully migrated Hugging Face Inference Providers to use the **Responses API** (`/v1/responses`) by default.

#### Enhanced Features:
- **Built-in Tool Orchestration**: Unified interface for model interactions, including tool execution.
- **Explicit Provider Selection**: Support for model suffixes (e.g., `:groq`, `:fal-ai`, `:nebius`) for targeted routing.
- **Reasoning Controls**: Full support for `reasoning.effort` on compatible models.
- **Event-Driven Streaming**: Precise semantic events for incremental UI updates.
- **Structured Output Reliability**: Automatic injection of JSON instructions for schema-validated outputs.
- **Remote MCP Support**: Capability to leverage Model Context Protocol tools via the Hugging Face router.

### 4. Provider Implementation Refinements (`vtcode-core/src/llm/providers/openai.rs`)

- **Enabled Responses API by default** for all Hugging Face router requests.
- **Removed GLM tool restrictions** for Responses API calls.
- **Preserved model suffixes** to allow manual provider selection.
- **Added automatic JSON instruction injection** for structured output/tool calls.

## Testing
- Verified model ID propagation (including suffixes).
- Confirmed correct routing to `/v1/responses`.
- Verified compilation and baseline functionality.

## Model Capabilities

### Reasoning Models (11 total):
All models support reasoning capabilities except `google/gemma-2-2b-it`:
- Qwen3-Coder-480B-A35B-Instruct
- GPT-OSS 120B
- GLM-4.5, GLM-4.6, GLM-4.7
- Qwen3-4B-Thinking-2507
- DeepSeek R1, DeepSeek V3.2
- Kimi K2 Thinking
- GLM-4.5V (VLM)

### Tool Calling:
All models support tool calling and structured output.

### Vision Support:
- **GLM-4.5V** - Accepts both text and image inputs

## Context Windows

- **1M tokens**: Qwen2.5-7B-Instruct-1M
- **131K tokens**: Most models (GPT-OSS, DeepSeek, GLM, Qwen Coder, Kimi)
- **32K tokens**: Qwen3-4B-Thinking-2507
- **8K tokens**: Gemma 2 2B IT

## Testing

Verified compilation:
```bash
cargo check --package vtcode-config  # ✓ Success
cargo check --package vtcode-core    # ✓ Success
```

## References

- [Chat Completion API Documentation](https://huggingface.co/docs/inference-providers/tasks/chat-completion)
- [Recommended Models](https://huggingface.co/docs/inference-providers/tasks/chat-completion#recommended-models)
- [API Playground](https://huggingface.co/playground)
